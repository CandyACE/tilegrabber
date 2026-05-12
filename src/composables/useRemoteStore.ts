import { ref } from "vue";
import { invoke } from "@tauri-apps/api/core";

export interface ConnectedRemoteServer {
  id: string;
  name: string;
  url: string;
  token: string;
}

export interface RemoteTask {
  id: string; name: string; status: string; source: string;
  total: number; downloaded: number; failed: number;
  speed?: number; etaSecs?: number;
  minZoom?: number; maxZoom?: number;
  boundsWest?: number; boundsEast?: number; boundsSouth?: number; boundsNorth?: number;
  sourceConfig?: string; createdAt?: string;
}

// ── Module-level singletons (survive component unmount) ──────────────────────
const connectedServer = ref<ConnectedRemoteServer | null>(null);
const remoteTasks = ref<RemoteTask[]>([]);
const latencyMs = ref<number | null>(null);

let pollTimer: ReturnType<typeof setInterval> | null = null;
let latencyTimer: ReturnType<typeof setInterval> | null = null;
const sseStreams = new Map<string, AbortController>();

// ── Data normalisation ────────────────────────────────────────────────────────
function normalizeTask(t: any): RemoteTask {
  return {
    id: t.id, name: t.name, status: t.status, source: t.source ?? "remote",
    total: t.totalTiles ?? t.total ?? 0,
    downloaded: t.downloadedTiles ?? t.downloaded ?? 0,
    failed: t.failedTiles ?? t.failed ?? 0,
    minZoom: t.minZoom, maxZoom: t.maxZoom,
    boundsWest: t.boundsWest, boundsEast: t.boundsEast,
    boundsSouth: t.boundsSouth, boundsNorth: t.boundsNorth,
    sourceConfig: t.sourceConfig, createdAt: t.createdAt,
  };
}

// ── Latency polling ───────────────────────────────────────────────────────────
export async function measureLatency(server: ConnectedRemoteServer) {
  try {
    const t0 = Date.now();
    await fetch(`${server.url.replace(/\/$/, "")}/remote/tasks`, {
      headers: { Authorization: `Bearer ${server.token}` },
      signal: AbortSignal.timeout(5000),
    });
    latencyMs.value = Date.now() - t0;
  } catch { latencyMs.value = null; }
}

function startLatencyPolling(server: ConnectedRemoteServer) {
  if (latencyTimer) clearInterval(latencyTimer);
  measureLatency(server);
  latencyTimer = setInterval(() => measureLatency(server), 30_000);
}

function stopLatencyPolling() {
  if (latencyTimer) { clearInterval(latencyTimer); latencyTimer = null; }
  latencyMs.value = null;
}

// ── Task fetching ─────────────────────────────────────────────────────────────
export async function fetchRemoteTasks(server: ConnectedRemoteServer) {
  try {
    const resp = await fetch(`${server.url.replace(/\/$/, "")}/remote/tasks`, {
      headers: { Authorization: `Bearer ${server.token}` },
    });
    if (resp.ok) remoteTasks.value = (await resp.json()).map(normalizeTask);
  } catch {}
}

// ── SSE progress stream ───────────────────────────────────────────────────────
function openSse(server: ConnectedRemoteServer, taskId: string) {
  if (sseStreams.has(taskId)) return;
  const ac = new AbortController();
  sseStreams.set(taskId, ac);
  fetch(`${server.url.replace(/\/$/, "")}/remote/tasks/${taskId}/progress`, {
    signal: ac.signal, headers: { Authorization: `Bearer ${server.token}` },
  }).then(async (resp) => {
    if (!resp.ok || !resp.body) return;
    const reader = resp.body.getReader();
    const dec = new TextDecoder();
    let buf = "";
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      buf += dec.decode(value, { stream: true });
      const lines = buf.split("\n"); buf = lines.pop() ?? "";
      for (const line of lines) {
        if (!line.startsWith("data:")) continue;
        try {
          const p = JSON.parse(line.slice(5).trim());
          const idx = remoteTasks.value.findIndex((t) => t.id === taskId);
          if (idx >= 0) {
            remoteTasks.value[idx] = {
              ...remoteTasks.value[idx],
              status: p.status ?? remoteTasks.value[idx].status,
              total: p.total ?? remoteTasks.value[idx].total,
              downloaded: p.downloaded ?? remoteTasks.value[idx].downloaded,
              speed: p.speed, etaSecs: p.eta_secs,
            };
            if (["completed", "error", "cancelled", "paused"].includes(p.status ?? "")) {
              ac.abort(); sseStreams.delete(taskId); return;
            }
          }
        } catch {}
      }
    }
  }).catch(() => { sseStreams.delete(taskId); });
}

// ── Polling ───────────────────────────────────────────────────────────────────
function startPolling(server: ConnectedRemoteServer) {
  if (pollTimer) clearInterval(pollTimer);
  pollTimer = setInterval(async () => {
    await fetchRemoteTasks(server);
    for (const task of remoteTasks.value) {
      if (task.status === "downloading" && !sseStreams.has(task.id)) openSse(server, task.id);
    }
  }, 10_000);
}

function stopPolling() { if (pollTimer) { clearInterval(pollTimer); pollTimer = null; } }
function stopAllSse() { for (const ac of sseStreams.values()) ac.abort(); sseStreams.clear(); }

// ── Public actions ────────────────────────────────────────────────────────────
/** Connect to a remote server. Throws on failure so the caller can surface the error. */
export async function connectRemote(server: ConnectedRemoteServer) {
  const resp = await fetch(`${server.url.replace(/\/$/, "")}/remote/tasks`, {
    headers: { Authorization: `Bearer ${server.token}` },
  });
  if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
  const tasks = (await resp.json()).map(normalizeTask) as RemoteTask[];
  connectedServer.value = server;
  remoteTasks.value = tasks;
  await invoke("update_remote_server", {
    id: server.id, server: { name: server.name, url: server.url, token: server.token },
  }).catch(() => {});
  for (const task of tasks) {
    if (task.status === "downloading") openSse(server, task.id);
  }
  startPolling(server);
  startLatencyPolling(server);
}

export function disconnectRemote() {
  stopPolling(); stopAllSse(); stopLatencyPolling();
  connectedServer.value = null;
  remoteTasks.value = [];
}

export async function cancelRemoteTask(taskId: string) {
  if (!connectedServer.value) return;
  const s = connectedServer.value;
  await fetch(`${s.url.replace(/\/$/, "")}/remote/tasks/${taskId}`, {
    method: "DELETE", headers: { Authorization: `Bearer ${s.token}` },
  }).catch(() => {});
  await fetchRemoteTasks(s);
}

export async function pauseRemoteTask(taskId: string) {
  if (!connectedServer.value) return;
  const s = connectedServer.value;
  await fetch(`${s.url.replace(/\/$/, "")}/remote/tasks/${taskId}/pause`, {
    method: "POST", headers: { Authorization: `Bearer ${s.token}` },
  }).catch(() => {});
  await fetchRemoteTasks(s);
}

export async function resumeRemoteTask(taskId: string) {
  if (!connectedServer.value) return;
  const s = connectedServer.value;
  await fetch(`${s.url.replace(/\/$/, "")}/remote/tasks/${taskId}/resume`, {
    method: "POST", headers: { Authorization: `Bearer ${s.token}` },
  }).catch(() => {});
  await fetchRemoteTasks(s);
}

export function useRemoteStore() {
  return { connectedServer, remoteTasks, latencyMs };
}
