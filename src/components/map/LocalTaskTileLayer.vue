<script setup lang="ts">
/**
 * LocalTaskTileLayer
 *
 * 当选中任务时，从本地瓦片存储（SQLite .tiles）读取数据并在地图上叠加显示。
 * 使用 MapLibre 自定义协议 `tilegrab-stored://` 代理每个瓦片的读取请求，
 * 通过 Tauri invoke `get_stored_tile` 从磁盘读取字节。
 *
 * 纯非可视组件——不渲染 DOM，仅操作地图 API。
 */
import { watch, onUnmounted } from "vue";
import maplibregl from "maplibre-gl";
import type { Map as MaplibreMap } from "maplibre-gl";
import { invoke } from "@tauri-apps/api/core";

// ─── Props ────────────────────────────────────────────────────────────────────

const props = defineProps<{
  map: MaplibreMap | null;
  taskId: string | null;
}>();

// ─── 图层 ID ──────────────────────────────────────────────────────────────────

const LAYER_ID = "local-task-tiles";
const SOURCE_ID = "local-task-tiles-src";
const PROTO = "tilegrab-stored";

// ─── 协议注册（全局一次）──────────────────────────────────────────────────────

let protoRegistered = false;

function ensureProtocol() {
  if (protoRegistered) return;
  protoRegistered = true;

  maplibregl.addProtocol(PROTO, async (params) => {
    // URL 格式: tilegrab-stored://taskId/z/x/y
    const raw = params.url.replace(`${PROTO}://`, "");
    const parts = raw.split("/");
    if (parts.length < 4)
      throw new Error("bad tilegrab-stored url: " + params.url);

    const taskId = parts[0];
    const z = parseInt(parts[1]);
    const x = parseInt(parts[2]);
    const y = parseInt(parts[3]);

    try {
      const bytes = await invoke<number[]>("get_stored_tile", {
        taskId,
        z,
        x,
        y,
      });
      return { data: new Uint8Array(bytes).buffer };
    } catch {
      // 404 — 返回 1×1 透明 PNG，避免 MapLibre 报错
      throw new Error("tile not found");
    }
  });
}

// ─── 取消令牌（防止 async 在组件卸载后继续操作已销毁的地图）────────────────────

let generation = 0;

// ─── 图层管理 ─────────────────────────────────────────────────────────────────

function removeLayers(m: MaplibreMap) {
  try {
    if (m.getLayer(LAYER_ID)) m.removeLayer(LAYER_ID);
    if (m.getSource(SOURCE_ID)) m.removeSource(SOURCE_ID);
  } catch {
    /* map already destroyed */
  }
}

interface BackendTask {
  boundsWest: number;
  boundsEast: number;
  boundsSouth: number;
  boundsNorth: number;
  minZoom: number;
  maxZoom: number;
  downloadedTiles: number;
  clipToBounds: boolean;
}

async function addLayer(m: MaplibreMap, taskId: string, gen: number) {
  removeLayers(m);

  let task: BackendTask;
  try {
    task = await invoke<BackendTask>("get_task", { taskId });
  } catch (e) {
    console.error("[LocalTaskTileLayer] get_task failed:", e);
    return;
  }

  // 若在 await 期间组件已卸载或 watch 已重跑，则放弃
  if (gen !== generation) return;

  if (task.downloadedTiles === 0) return;

  ensureProtocol();

  const tileUrl = `${PROTO}://${taskId}/{z}/{x}/{y}`;

  try {
    m.addSource(SOURCE_ID, {
      type: "raster",
      tiles: [tileUrl],
      tileSize: 256,
      minzoom: task.minZoom,
      maxzoom: task.maxZoom,
      bounds: [
        task.boundsWest,
        task.boundsSouth,
        task.boundsEast,
        task.boundsNorth,
      ],
      scheme: "xyz",
    });

    // 插入到范围框图层下方（如有），确保范围框始终在最上方；
    // 若范围框尚未添加，则插入到标注层下方（如有），使标注保持可见
    const OVERLAY_FILL = "task-overlay-fill";
    const insertBefore = m.getLayer(OVERLAY_FILL)
      ? OVERLAY_FILL
      : getFirstLabelLayerId(m);
    m.addLayer(
      {
        id: LAYER_ID,
        type: "raster",
        source: SOURCE_ID,
        paint: { "raster-opacity": 1 },
      },
      insertBefore,
    );
  } catch (e) {
    console.error("[LocalTaskTileLayer] addLayer failed:", e);
  }
}

function getFirstLabelLayerId(m: MaplibreMap): string | undefined {
  for (const layer of m.getStyle().layers ?? []) {
    if (layer.type === "symbol") return layer.id;
  }
  return undefined;
}

// ─── 监听 ─────────────────────────────────────────────────────────────────────

watch(
  () => [props.map, props.taskId] as const,
  ([m, id]) => {
    generation++;
    if (m && id) {
      addLayer(m, id, generation);
    } else if (m) {
      removeLayers(m);
    }
  },
  { immediate: true },
);

onUnmounted(() => {
  generation++; // 令所有挂起的 async 失效
  if (props.map) removeLayers(props.map);
});
</script>

<template><!-- 无可视内容，仅操作地图图层 --></template>
