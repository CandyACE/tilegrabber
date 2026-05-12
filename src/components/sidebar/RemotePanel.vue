<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useI18n } from "vue-i18n";
import {
  Share2, Plus, Trash2, Plug, PlugZap, Loader2, RefreshCw,
  CheckCircle2, XCircle, ChevronRight, AlertCircle,
  Server, Monitor, KeyRound, Copy, Check, ShieldAlert, Power, Users,
  Pencil, Wifi, WifiOff, ArrowRight, Pause, Play,
} from "lucide-vue-next";
import {
  useRemoteStore, connectRemote, disconnectRemote,
  cancelRemoteTask, fetchRemoteTasks, pauseRemoteTask, resumeRemoteTask,
} from "~/composables/useRemoteStore";
import type { RemoteTask } from "~/composables/useRemoteStore";

const { t } = useI18n();

const emit = defineEmits<{ "start-task": [] }>();

interface RemoteServer { id: string; name: string; url: string; token: string; createdAt: number; lastUsedAt?: number; }
interface ServerStatus { running: boolean; port: number; lanUrls: string[]; }
interface ClientInfo { id: string; ip: string; connectedAt: number; }

type PanelMode = "server" | "client";
const mode = ref<PanelMode>((localStorage.getItem("remote.panelMode") as PanelMode) ?? "client");

// Dialog state for mode switch warning
const switchWarningVisible = ref(false);
const pendingSwitchMode = ref<PanelMode | null>(null);

function switchMode(newMode: PanelMode) {
  if (newMode === mode.value) return;
  // Warn before stopping a running server
  if (newMode === "client" && serverStatus.value?.running) {
    pendingSwitchMode.value = newMode;
    switchWarningVisible.value = true;
    return;
  }
  // Warn before disconnecting an active client connection
  if (newMode === "server" && connectedServer.value) {
    pendingSwitchMode.value = newMode;
    switchWarningVisible.value = true;
    return;
  }
  applyModeSwitch(newMode);
}

async function confirmModeSwitch() {
  switchWarningVisible.value = false;
  if (!pendingSwitchMode.value) return;
  if (pendingSwitchMode.value === "client") await stopRemoteServer();
  if (pendingSwitchMode.value === "server") disconnect();
  applyModeSwitch(pendingSwitchMode.value);
  pendingSwitchMode.value = null;
}

function cancelModeSwitch() {
  switchWarningVisible.value = false;
  pendingSwitchMode.value = null;
}

function applyModeSwitch(newMode: PanelMode) {
  mode.value = newMode;
  localStorage.setItem("remote.panelMode", newMode);
}

const serverStatus = ref<ServerStatus | null>(null);
const serverToggling = ref(false);
const serverError = ref("");
const serverPort = ref("8766");
const remoteToken = ref("");
const tokenCopied = ref(false);
const generatingToken = ref(false);
const remoteClients = ref<ClientInfo[]>([]);
let unlistenClients: (() => void) | null = null;

async function loadServerStatus() {
  try { serverStatus.value = await invoke<ServerStatus>("get_remote_server_status"); }
  catch { serverStatus.value = null; }
}

async function loadRemoteSettings() {
  try {
    const s = await invoke<Record<string, string>>("get_all_settings");
    serverPort.value = s["remote.port"] ?? "8766";
    remoteToken.value = s["remote.token"] ?? "";
  } catch {}
}

async function loadClients() {
  try { remoteClients.value = await invoke<ClientInfo[]>("get_remote_clients"); }
  catch { remoteClients.value = []; }
}

async function toggleRemoteServer() {
  serverToggling.value = true;
  serverError.value = "";
  try {
    if (serverStatus.value?.running) {
      await stopRemoteServer();
    } else {
      const port = parseInt(serverPort.value, 10) || 8766;
      await invoke("set_setting", { key: "remote.port", value: String(port) });
      serverStatus.value = await invoke<ServerStatus>("start_remote_server_cmd", { port });
    }
  } catch (e) { serverError.value = String(e); }
  finally { serverToggling.value = false; }
}

async function stopRemoteServer() {
  try {
    await invoke("stop_remote_server_cmd");
    serverStatus.value = { running: false, port: serverStatus.value?.port ?? 8766, lanUrls: [] };
  } catch {}
}

async function generateToken() {
  generatingToken.value = true;
  try {
    const token = await invoke<string>("generate_remote_token");
    remoteToken.value = token;
    await invoke("set_setting", { key: "remote.token", value: token });
  } catch (e) { serverError.value = String(e); }
  finally { generatingToken.value = false; }
}

async function copyToken() {
  if (!remoteToken.value) return;
  await navigator.clipboard.writeText(remoteToken.value);
  tokenCopied.value = true;
  setTimeout(() => { tokenCopied.value = false; }, 2000);
}

const servers = ref<RemoteServer[]>([]);
async function loadServers() {
  try { servers.value = await invoke<RemoteServer[]>("list_remote_servers"); }
  catch { servers.value = []; }
}

const showAddForm = ref(false);
const addForm = ref({ name: "", url: "", token: "" });
const addError = ref("");
const addLoading = ref(false);

async function submitAddServer() {
  addError.value = "";
  const url = addForm.value.url.trim();
  const token = addForm.value.token.trim();
  const name = addForm.value.name.trim() || url;
  if (!url || !token) { addError.value = t("remote.addForm.validationError"); return; }
  addLoading.value = true;
  try {
    await invoke("add_remote_server", { server: { name, url, token } });
    addForm.value = { name: "", url: "", token: "" };
    showAddForm.value = false;
    await loadServers();
  } catch (e) { addError.value = String(e); }
  finally { addLoading.value = false; }
}

async function deleteServer(id: string) {
  await invoke("remove_remote_server", { id });
  if (connectedServer.value?.id === id) disconnect();
  if (selectedServerId.value === id) selectedServerId.value = null;
  await loadServers();
}

const { connectedServer, remoteTasks, latencyMs } = useRemoteStore();

// ─── Selected server ──────────────────────────────────────────────────────────
const selectedServerId = ref<string | null>(null);

function selectServer(id: string) {
  selectedServerId.value = selectedServerId.value === id ? null : id;
}

const selectedServer = computed(() => servers.value.find(s => s.id === selectedServerId.value) ?? null);

// ─── Edit server ─────────────────────────────────────────────────────────────
const editingServerId = ref<string | null>(null);
const editForm = ref({ name: "", url: "", token: "" });
const editError = ref("");
const editLoading = ref(false);

function startEdit(server: RemoteServer) {
  editingServerId.value = server.id;
  editForm.value = { name: server.name, url: server.url, token: server.token };
  editError.value = "";
}

function cancelEdit() { editingServerId.value = null; editError.value = ""; }

async function submitEdit() {
  if (!editingServerId.value) return;
  const url = editForm.value.url.trim();
  const token = editForm.value.token.trim();
  const name = editForm.value.name.trim() || url;
  if (!url || !token) { editError.value = t("remote.addForm.validationError"); return; }
  editLoading.value = true;
  try {
    await invoke("update_remote_server", { id: editingServerId.value, server: { name, url, token } });
    editingServerId.value = null;
    await loadServers();
  } catch (e) { editError.value = String(e); }
  finally { editLoading.value = false; }
}

const connectLoading = ref<string | null>(null);
const connectError = ref("");
const tasksLoading = ref(false);

async function connect(server: RemoteServer) {
  connectLoading.value = server.id;
  connectError.value = "";
  try {
    await connectRemote(server);
    selectedServerId.value = server.id; // auto-expand to show task list
  } catch (e) {
    connectError.value = `${t("remote.connectFailed")}: ${String(e)}`;
  } finally { connectLoading.value = null; }
}

function disconnect() {
  disconnectRemote();
  connectError.value = "";
}

async function fetchTasks() {
  if (!connectedServer.value) return;
  tasksLoading.value = true;
  try { await fetchRemoteTasks(connectedServer.value); }
  finally { tasksLoading.value = false; }
}

async function cancelTask(taskId: string) {
  await cancelRemoteTask(taskId);
}
async function pauseTask(taskId: string) {
  await pauseRemoteTask(taskId);
}
async function resumeTask(taskId: string) {
  await resumeRemoteTask(taskId);
}

function progressPct(task: RemoteTask) { return task.total ? Math.round((task.downloaded / task.total) * 100) : 0; }

const statusColor: Record<string, string> = {
  queued: "text-slate-500", downloading: "text-blue-600",
  paused: "text-amber-500",
  completed: "text-green-600", error: "text-red-500", cancelled: "text-slate-400",
};

const expandedTaskId = ref<string | null>(null);
function toggleTaskExpand(id: string) {
  expandedTaskId.value = expandedTaskId.value === id ? null : id;
}

function getSourceName(sourceConfig?: string): string {
  if (!sourceConfig) return "—";
  try { const s = JSON.parse(sourceConfig); return s.name ?? s.urlTemplate ?? "—"; } catch { return "—"; }
}

function formatCoord(v?: number, fixed = 4): string {
  return v !== undefined ? v.toFixed(fixed) : "—";
}

function formatEta(secs: number): string {
  if (secs < 60) return `${Math.round(secs)}s`;
  if (secs < 3600) return `${Math.floor(secs / 60)}m${Math.round(secs % 60)}s`;
  return `${Math.floor(secs / 3600)}h${Math.floor((secs % 3600) / 60)}m`;
}

onMounted(async () => {
  await Promise.all([loadServerStatus(), loadRemoteSettings(), loadServers()]);
  unlistenClients = await listen<number>("remote:clients-changed", loadClients);
  if (mode.value === "server") await loadClients();
});

onUnmounted(() => { unlistenClients?.(); });

watch(mode, async (m) => { if (m === "server") { await loadServerStatus(); await loadClients(); } });
</script>

<template>
  <div class="flex flex-col h-full overflow-y-auto">

    <!-- Mode switch warning dialog -->
    <Teleport to="body">
      <Transition name="dialog-fade">
        <div v-if="switchWarningVisible" class="fixed inset-0 z-50 flex items-center justify-center p-4">
          <div class="absolute inset-0 bg-black/40 backdrop-blur-sm" @click="cancelModeSwitch" />
          <div class="relative bg-white rounded-2xl shadow-2xl w-full max-w-sm p-6 flex flex-col gap-4">
            <div class="flex items-start gap-3">
              <div class="shrink-0 w-9 h-9 rounded-full bg-amber-100 flex items-center justify-center">
                <AlertCircle :size="18" class="text-amber-500" />
              </div>
              <div class="flex flex-col gap-1">
                <p class="text-sm font-semibold text-slate-800">{{ t('remote.switchDialog.title') }}</p>
                <p class="text-xs text-slate-500 leading-relaxed">{{ pendingSwitchMode === 'client' ? t('remote.switchDialog.bodyToClient') : t('remote.switchDialog.bodyToServer') }}</p>
              </div>
            </div>
            <div class="flex justify-end gap-2">
              <button @click="cancelModeSwitch" class="px-4 py-2 rounded-lg text-xs font-medium text-slate-600 hover:bg-slate-100 transition-colors">{{ t('remote.switchDialog.cancel') }}</button>
              <button @click="confirmModeSwitch" class="px-4 py-2 rounded-lg text-xs font-semibold bg-amber-500 text-white hover:bg-amber-600 transition-colors">{{ t('remote.switchDialog.confirm') }}</button>
            </div>
          </div>
        </div>
      </Transition>
    </Teleport>

    <div class="flex flex-col gap-4 w-full max-w-2xl mx-auto px-6 py-6 text-sm">

      <div class="flex flex-col gap-3">
        <div class="flex items-center gap-2">
          <Share2 :size="15" class="text-slate-400" />
          <h2 class="font-semibold text-slate-700 text-sm">{{ t('remote.title') }}</h2>
        </div>
        <div class="flex items-center gap-1 p-0.5 rounded-xl bg-slate-100 self-start">
          <button @click="switchMode('server')" class="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium transition-all" :class="mode === 'server' ? 'bg-white text-slate-700 shadow-sm' : 'text-slate-500 hover:text-slate-700'">
            <Server :size="12" />{{ t('remote.modeServer') }}
          </button>
          <button @click="switchMode('client')" class="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium transition-all" :class="mode === 'client' ? 'bg-white text-slate-700 shadow-sm' : 'text-slate-500 hover:text-slate-700'">
            <Monitor :size="12" />{{ t('remote.modeClient') }}
          </button>
        </div>
      </div>

      <!-- SERVER MODE -->
      <template v-if="mode === 'server'">

        <div class="rounded-xl border bg-white overflow-hidden" style="border-color: var(--color-border-subtle)">
          <div class="p-4 flex flex-col gap-4">
            <div class="flex items-center justify-between gap-4">
              <div class="flex flex-col gap-0.5">
                <span class="text-xs font-semibold text-slate-700">{{ t('remote.server.title') }}</span>
                <span class="text-[11px] text-slate-400">{{ t('remote.server.hint') }}</span>
              </div>
              <button @click="toggleRemoteServer" :disabled="serverToggling" class="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-semibold transition-colors disabled:opacity-60" :class="serverStatus?.running ? 'bg-red-50 text-red-600 border border-red-200 hover:bg-red-100' : 'bg-green-500 text-white hover:bg-green-600'">
                <Loader2 v-if="serverToggling" :size="12" class="animate-spin" />
                <Power v-else :size="12" />
                {{ serverToggling ? t('remote.server.toggling') : serverStatus?.running ? t('remote.server.stop') : t('remote.server.start') }}
              </button>
            </div>
            <div class="flex items-center justify-between gap-4">
              <div class="flex flex-col gap-0.5">
                <span class="text-xs font-medium text-slate-700">{{ t('remote.server.port') }}</span>
                <span class="text-[11px] text-slate-400">{{ t('remote.server.portHint') }}</span>
              </div>
              <input v-model="serverPort" type="number" min="1024" max="65535" :disabled="serverStatus?.running" class="w-20 shrink-0 rounded-md border px-2 py-1 text-xs text-center tabular-nums focus:outline-none focus:ring-1 focus:ring-blue-400 disabled:opacity-50 disabled:bg-slate-50" style="border-color: var(--color-border-subtle)" />
            </div>
            <div v-if="serverError" class="rounded-lg bg-red-50 border border-red-200 px-3 py-2 flex items-start gap-2">
              <AlertCircle :size="12" class="text-red-400 shrink-0 mt-px" />
              <span class="text-[11px] text-red-600">{{ serverError }}</span>
            </div>
          </div>
        </div>

        <div v-if="serverStatus?.running" class="rounded-xl border bg-green-50 border-green-200 overflow-hidden">
          <div class="p-4 flex flex-col gap-3">
            <div class="flex items-center gap-2">
              <span class="w-2 h-2 rounded-full bg-green-500 animate-pulse shrink-0"></span>
              <span class="text-xs font-semibold text-green-800">{{ t('remote.server.running') }}</span>
              <span class="text-[11px] text-green-600 font-mono ml-auto">:{{ serverStatus.port }}</span>
            </div>
            <div v-if="serverStatus.lanUrls.length" class="flex flex-col gap-1.5">
              <span class="text-[11px] text-green-700 font-medium">{{ t('remote.server.lanAddress') }}</span>
              <div v-for="url in serverStatus.lanUrls" :key="url" class="font-mono text-[11px] text-green-800 bg-green-100 rounded-lg px-3 py-1.5 select-all break-all">{{ url }}</div>
            </div>
            <div class="flex items-start gap-1.5 text-[10px] text-green-700">
              <ShieldAlert :size="11" class="shrink-0 mt-px" />
              {{ t('remote.server.firewallHint', { port: serverStatus.port }) }}
            </div>
          </div>
        </div>

        <div class="rounded-xl border bg-white overflow-hidden" style="border-color: var(--color-border-subtle)">
          <div class="flex items-center gap-2 px-4 py-2.5 border-b" style="border-color: var(--color-border-subtle)">
            <KeyRound :size="13" class="text-slate-400 shrink-0" />
            <span class="text-xs font-semibold text-slate-600">{{ t('remote.server.tokenTitle') }}</span>
          </div>
          <div class="p-4 flex flex-col gap-3">
            <div class="rounded-lg bg-slate-50 border px-3 py-2 font-mono text-[11px] break-all select-all" :class="remoteToken ? 'text-slate-700 border-slate-200' : 'text-slate-400 border-dashed border-slate-200 italic'">{{ remoteToken || t('remote.server.tokenEmpty') }}</div>
            <div class="flex items-center gap-2">
              <button @click="copyToken" :disabled="!remoteToken" class="flex items-center gap-1 px-2.5 py-1.5 rounded-lg text-[11px] font-medium transition-colors disabled:opacity-40" :class="tokenCopied ? 'bg-green-50 text-green-600 border border-green-200' : 'bg-slate-50 text-slate-600 border border-slate-200 hover:bg-slate-100'">
                <Check v-if="tokenCopied" :size="11" /><Copy v-else :size="11" />
                {{ tokenCopied ? t('remote.server.copied') : t('remote.server.copy') }}
              </button>
              <button @click="generateToken" :disabled="generatingToken" class="flex items-center gap-1 px-2.5 py-1.5 rounded-lg text-[11px] font-medium bg-blue-500 text-white hover:bg-blue-600 disabled:opacity-60 transition-colors">
                <Loader2 v-if="generatingToken" :size="11" class="animate-spin" /><RefreshCw v-else :size="11" />
                {{ generatingToken ? t('remote.server.generating') : t('remote.server.generate') }}
              </button>
            </div>
            <span class="text-[11px] text-slate-400">{{ t('remote.server.tokenHint') }}</span>
          </div>
        </div>

        <div class="rounded-xl border bg-white overflow-hidden" style="border-color: var(--color-border-subtle)">
          <div class="flex items-center gap-2 px-4 py-2.5 border-b" style="border-color: var(--color-border-subtle)">
            <Users :size="13" class="text-slate-400 shrink-0" />
            <span class="text-xs font-semibold text-slate-600">{{ t('remote.server.clients') }}</span>
            <span v-if="remoteClients.length" class="text-[10px] px-1.5 py-0.5 rounded-full bg-blue-100 text-blue-600 font-medium leading-none">{{ remoteClients.length }}</span>
          </div>
          <div v-if="!remoteClients.length" class="px-4 py-6 text-center text-xs text-slate-400">{{ t('remote.server.noClients') }}</div>
          <div v-else class="divide-y" style="border-color: var(--color-border-subtle)">
            <div v-for="client in remoteClients" :key="client.id" class="flex items-center justify-between px-4 py-2.5 text-[11px]">
              <span class="font-mono text-slate-600">{{ client.ip }}</span>
              <span class="text-slate-400">{{ new Date(client.connectedAt * 1000).toLocaleTimeString() }}</span>
            </div>
          </div>
        </div>

        <p class="text-[11px] text-slate-400 text-center px-4">{{ t('remote.server.switchHint') }}</p>

      </template>

      <!-- CLIENT MODE -->
      <template v-else>

        <div class="flex justify-end">
          <button @click="showAddForm = !showAddForm" class="flex items-center gap-1 px-2.5 py-1 rounded-lg text-xs font-medium bg-blue-500 text-white hover:bg-blue-600 transition-colors">
            <Plus :size="12" />{{ t('remote.addServer') }}
          </button>
        </div>

        <Transition name="fade-in">
          <div v-if="showAddForm" class="rounded-xl border bg-white p-4 flex flex-col gap-3" style="border-color: var(--color-border-subtle)">
            <p class="text-xs font-semibold text-slate-600">{{ t('remote.addForm.title') }}</p>
            <div class="flex flex-col gap-1.5">
              <label class="text-[11px] text-slate-500">{{ t('remote.addForm.name') }}</label>
              <input v-model="addForm.name" :placeholder="t('remote.addForm.namePlaceholder')" class="w-full px-2.5 py-1.5 rounded-md bg-slate-50 border border-slate-200 text-xs focus:outline-none focus:ring-1 focus:ring-blue-500/60" />
            </div>
            <div class="flex flex-col gap-1.5">
              <label class="text-[11px] text-slate-500">{{ t('remote.addForm.url') }} <span class="text-red-400">*</span></label>
              <input v-model="addForm.url" placeholder="http://192.168.1.100:8766" class="w-full px-2.5 py-1.5 rounded-md bg-slate-50 border border-slate-200 text-xs font-mono focus:outline-none focus:ring-1 focus:ring-blue-500/60" />
            </div>
            <div class="flex flex-col gap-1.5">
              <label class="text-[11px] text-slate-500">{{ t('remote.addForm.token') }} <span class="text-red-400">*</span></label>
              <input v-model="addForm.token" placeholder="tg_xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx" class="w-full px-2.5 py-1.5 rounded-md bg-slate-50 border border-slate-200 text-xs font-mono focus:outline-none focus:ring-1 focus:ring-blue-500/60" />
            </div>
            <div v-if="addError" class="flex items-center gap-1.5 text-[11px] text-red-500">
              <AlertCircle :size="11" class="shrink-0" />{{ addError }}
            </div>
            <div class="flex items-center gap-2 pt-1">
              <button @click="submitAddServer" :disabled="addLoading" class="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium bg-blue-500 text-white hover:bg-blue-600 disabled:opacity-60 transition-colors">
                <Loader2 v-if="addLoading" :size="11" class="animate-spin" /><Plus v-else :size="11" />{{ t('remote.addForm.save') }}
              </button>
              <button @click="showAddForm = false; addError = ''" class="px-3 py-1.5 rounded-lg text-xs font-medium text-slate-600 hover:bg-slate-100 transition-colors">{{ t('remote.addForm.cancel') }}</button>
            </div>
          </div>
        </Transition>

        <!-- Error banner -->
        <div v-if="connectError" class="rounded-lg bg-red-50 border border-red-200 px-3 py-2 flex items-start gap-2">
          <XCircle :size="13" class="text-red-400 shrink-0 mt-px" />
          <span class="text-xs text-red-600">{{ connectError }}</span>
        </div>

        <!-- Server list (merged with task list when connected) -->
        <div class="rounded-xl border bg-white overflow-hidden" style="border-color: var(--color-border-subtle)">
          <div v-if="!servers.length" class="px-4 py-8 flex flex-col items-center gap-2 text-slate-400">
            <WifiOff :size="22" class="opacity-40" />
            <span class="text-xs">{{ t('remote.noServers') }}</span>
          </div>
          <div v-else class="divide-y" style="border-color: var(--color-border-subtle)">
            <template v-for="server in servers" :key="server.id">

              <!-- Server row -->
              <div
                class="flex items-center gap-3 px-4 py-3 cursor-pointer hover:bg-slate-50 transition-colors"
                :class="selectedServerId === server.id ? 'bg-blue-50/40' : ''"
                @click="selectServer(server.id)"
              >
                <div class="shrink-0">
                  <div v-if="connectedServer?.id === server.id" class="size-2.5 rounded-full bg-green-500 ring-2 ring-green-200 animate-pulse" />
                  <div v-else class="size-2.5 rounded-full bg-slate-300 ring-2 ring-slate-100" />
                </div>
                <div class="flex-1 min-w-0">
                  <div class="font-medium text-slate-700 truncate text-xs">{{ server.name }}</div>
                  <div class="text-[11px] text-slate-400 font-mono truncate">{{ server.url }}</div>
                </div>
                <!-- Latency badge when connected -->
                <div v-if="connectedServer?.id === server.id" class="flex items-center gap-1 text-[11px] shrink-0 font-mono" :class="latencyMs === null ? 'text-slate-400' : latencyMs < 100 ? 'text-green-600' : latencyMs < 500 ? 'text-amber-500' : 'text-red-500'">
                  <Wifi :size="11" />{{ latencyMs !== null ? `${latencyMs}ms` : '—' }}
                </div>
                <ChevronRight :size="13" class="shrink-0 text-slate-300 transition-transform" :class="selectedServerId === server.id ? 'rotate-90' : ''" />
              </div>

              <!-- Expanded: CONNECTED — task list inline -->
              <Transition name="fade-in">
                <div v-if="selectedServerId === server.id && connectedServer?.id === server.id && editingServerId !== server.id" class="border-t" style="border-color: var(--color-border-subtle)">
                  <!-- Control bar -->
                  <div class="flex items-center gap-2 px-4 py-2.5 bg-slate-50 border-b" style="border-color: var(--color-border-subtle)">
                    <button @click.stop="disconnect" class="flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg text-[11px] font-medium text-red-600 bg-red-50 border border-red-200 hover:bg-red-100 transition-colors">
                      <Plug :size="11" />{{ t('remote.disconnect') }}
                    </button>
                    <span class="text-[11px] text-slate-400 ml-auto">{{ t('remote.taskList') }}</span>
                    <button @click.stop="fetchTasks()" :disabled="tasksLoading" class="p-1 rounded-md text-slate-400 hover:text-slate-600 hover:bg-white transition-colors" :title="t('remote.refresh')">
                      <RefreshCw :size="12" :class="tasksLoading ? 'animate-spin' : ''" />
                    </button>
                  </div>
                  <!-- Tasks -->
                  <div v-if="!remoteTasks.length" class="px-4 py-6 text-center text-xs text-slate-400">{{ t('remote.noTasks') }}</div>
                  <div v-else class="divide-y" style="border-color: var(--color-border-subtle)">
                    <div v-for="task in remoteTasks" :key="task.id" class="flex flex-col">
                      <!-- Task row -->
                      <div class="px-4 py-3 flex flex-col gap-1.5 cursor-pointer hover:bg-slate-50 transition-colors" @click.stop="toggleTaskExpand(task.id)">
                        <div class="flex items-center justify-between gap-2">
                          <div class="flex items-center gap-1.5 min-w-0">
                            <ChevronRight :size="11" class="shrink-0 text-slate-300 transition-transform" :class="expandedTaskId === task.id ? 'rotate-90' : ''" />
                            <span class="text-xs font-medium text-slate-700 truncate">{{ task.name }}</span>
                          </div>
                          <div class="flex items-center gap-1 shrink-0">
                            <span :class="['text-[11px] font-medium', statusColor[task.status] ?? 'text-slate-500']">{{ t(`remote.status.${task.status}`, task.status) }}</span>
                            <button v-if="task.status === 'downloading'" @click.stop="pauseTask(task.id)" :title="t('remote.pauseTask')" class="p-0.5 rounded text-slate-400 hover:text-amber-500 transition-colors"><Pause :size="13" /></button>
                            <button v-if="task.status === 'paused'" @click.stop="resumeTask(task.id)" :title="t('remote.resumeTask')" class="p-0.5 rounded text-slate-400 hover:text-blue-500 transition-colors"><Play :size="13" /></button>
                            <button v-if="['queued','downloading','paused'].includes(task.status)" @click.stop="cancelTask(task.id)" :title="t('remote.cancelTask')" class="p-0.5 rounded text-slate-400 hover:text-red-500 transition-colors"><XCircle :size="13" /></button>
                          </div>
                        </div>
                        <!-- Progress bar -->
                        <div v-if="['downloading','paused','completed'].includes(task.status)" class="flex flex-col gap-1">
                          <div class="flex items-center gap-2">
                            <div class="flex-1 h-1.5 rounded-full bg-slate-100 overflow-hidden">
                              <div class="h-full rounded-full transition-all duration-300" :class="task.status === 'completed' ? 'bg-green-500' : task.status === 'paused' ? 'bg-amber-400' : 'bg-blue-500'" :style="{ width: `${progressPct(task)}%` }" />
                            </div>
                            <span class="text-[10px] text-slate-400 shrink-0 w-8 text-right font-mono">{{ progressPct(task) }}%</span>
                          </div>
                          <div v-if="task.status === 'downloading'" class="flex items-center gap-2 text-[10px] text-slate-400">
                            <span v-if="task.speed">{{ task.speed.toFixed(1) }} t/s</span>
                            <span class="tabular-nums">{{ task.downloaded.toLocaleString() }} / {{ task.total.toLocaleString() }}</span>
                            <span v-if="task.etaSecs" class="ml-auto">ETA {{ formatEta(task.etaSecs) }}</span>
                          </div>
                        </div>
                      </div>
                      <!-- Task detail expand -->
                      <Transition name="fade-in">
                        <div v-if="expandedTaskId === task.id" class="border-t px-4 py-3 bg-slate-50 flex flex-col gap-2" style="border-color: var(--color-border-subtle)">
                          <div class="grid grid-cols-2 gap-x-4 gap-y-1.5 text-[11px]">
                            <div class="text-slate-400">图层</div>
                            <div class="text-slate-700 font-mono truncate">{{ getSourceName(task.sourceConfig) }}</div>
                            <div class="text-slate-400">层级</div>
                            <div class="text-slate-700 font-mono">Z{{ task.minZoom }} – Z{{ task.maxZoom }}</div>
                            <div class="text-slate-400">瓦片总数</div>
                            <div class="text-slate-700 font-mono tabular-nums">{{ task.total.toLocaleString() }}</div>
                            <div class="text-slate-400">已下载</div>
                            <div class="text-slate-700 font-mono tabular-nums">{{ task.downloaded.toLocaleString() }}</div>
                            <template v-if="task.failed > 0">
                              <div class="text-slate-400">失败</div>
                              <div class="text-red-500 font-mono tabular-nums">{{ task.failed.toLocaleString() }}</div>
                            </template>
                            <div class="text-slate-400">范围</div>
                            <div class="text-slate-700 font-mono text-[10px] leading-tight">
                              {{ formatCoord(task.boundsWest) }},{{ formatCoord(task.boundsSouth) }}<br/>{{ formatCoord(task.boundsEast) }},{{ formatCoord(task.boundsNorth) }}
                            </div>
                            <div class="text-slate-400">创建时间</div>
                            <div class="text-slate-500 text-[10px]">{{ task.createdAt ? new Date(task.createdAt).toLocaleString() : '—' }}</div>
                          </div>
                        </div>
                      </Transition>
                    </div>
                  </div>
                </div>
              </Transition>

              <!-- Expanded: NOT CONNECTED — action bar -->
              <Transition name="fade-in">
                <div v-if="selectedServerId === server.id && connectedServer?.id !== server.id && editingServerId !== server.id" class="px-4 py-3 bg-slate-50 border-t flex flex-col gap-2" style="border-color: var(--color-border-subtle)">
                  <div class="flex items-center gap-2">
                    <button @click.stop="connect(server)" :disabled="connectLoading === server.id" class="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-[11px] font-medium text-blue-600 bg-blue-50 border border-blue-200 hover:bg-blue-100 disabled:opacity-60 transition-colors">
                      <Loader2 v-if="connectLoading === server.id" :size="11" class="animate-spin" /><PlugZap v-else :size="11" />{{ t('remote.connect') }}
                    </button>
                    <button @click.stop="startEdit(server)" class="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-[11px] font-medium text-slate-600 bg-white border border-slate-200 hover:bg-slate-100 transition-colors">
                      <Pencil :size="11" />{{ t('remote.edit') }}
                    </button>
                    <button @click.stop="deleteServer(server.id)" class="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-[11px] font-medium text-red-500 bg-white border border-slate-200 hover:bg-red-50 hover:border-red-200 transition-colors ml-auto">
                      <Trash2 :size="11" />{{ t('remote.delete') }}
                    </button>
                  </div>
                </div>
              </Transition>

              <!-- Edit form -->
              <Transition name="fade-in">
                <div v-if="editingServerId === server.id" class="px-4 py-3 bg-slate-50 border-t flex flex-col gap-3" style="border-color: var(--color-border-subtle)">
                  <p class="text-[11px] font-semibold text-slate-600">{{ t('remote.addForm.editTitle') }}</p>
                  <input v-model="editForm.name" :placeholder="t('remote.addForm.namePlaceholder')" class="w-full px-2.5 py-1.5 rounded-md bg-white border border-slate-200 text-xs focus:outline-none focus:ring-1 focus:ring-blue-500/60" />
                  <input v-model="editForm.url" placeholder="http://192.168.1.100:8766" class="w-full px-2.5 py-1.5 rounded-md bg-white border border-slate-200 text-xs font-mono focus:outline-none focus:ring-1 focus:ring-blue-500/60" />
                  <input v-model="editForm.token" placeholder="tg_xxxxxxxx..." class="w-full px-2.5 py-1.5 rounded-md bg-white border border-slate-200 text-xs font-mono focus:outline-none focus:ring-1 focus:ring-blue-500/60" />
                  <div v-if="editError" class="flex items-center gap-1.5 text-[11px] text-red-500"><AlertCircle :size="11" class="shrink-0" />{{ editError }}</div>
                  <div class="flex items-center gap-2">
                    <button @click.stop="submitEdit" :disabled="editLoading" class="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium bg-blue-500 text-white hover:bg-blue-600 disabled:opacity-60 transition-colors">
                      <Loader2 v-if="editLoading" :size="11" class="animate-spin" /><Check v-else :size="11" />{{ t('remote.addForm.save') }}
                    </button>
                    <button @click.stop="cancelEdit" class="px-3 py-1.5 rounded-lg text-xs font-medium text-slate-600 hover:bg-slate-100 transition-colors">{{ t('remote.addForm.cancel') }}</button>
                  </div>
                </div>
              </Transition>

            </template>
          </div>
        </div>

        <!-- Submit new task button -->
        <button
          v-if="connectedServer"
          @click="emit('start-task')"
          class="w-full flex items-center justify-center gap-2 py-3 rounded-xl text-xs font-semibold text-blue-600 bg-blue-50 border border-blue-200 hover:bg-blue-100 transition-colors"
        >
          <Plus :size="14" />{{ t('remote.submitNewTask') }}
          <ArrowRight :size="13" class="ml-1 opacity-60" />
        </button>

      </template>

    </div>
  </div>
</template>

<style scoped>
.fade-in-enter-active { transition: opacity 0.2s ease, transform 0.15s ease; }
.fade-in-enter-from { opacity: 0; transform: translateY(-4px); }
.dialog-fade-enter-active { transition: opacity 0.15s ease; }
.dialog-fade-leave-active { transition: opacity 0.1s ease; }
.dialog-fade-enter-from,
.dialog-fade-leave-to { opacity: 0; }
</style>
