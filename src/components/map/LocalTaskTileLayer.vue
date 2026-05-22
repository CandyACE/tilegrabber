<script setup lang="ts">
/**
 * LocalTaskTileLayer
 *
 * 当选中任务时，从本地瓦片存储（SQLite .tiles）读取数据并在地图上叠加显示。
 * 使用 MapLibre 自定义协议 `tilegrab-stored://` 代理每个瓦片的读取请求，
 * 通过 Tauri invoke `get_stored_tile` 从磁盘读取字节。
 *
 * 支持栅格（png/jpg/webp/terrain）与矢量（pbf/mvt）两种瓦片：
 * - 栅格：MapLibre `type: raster` 数据源 + raster 图层
 * - 矢量：`type: vector` 数据源 + 自动生成的「骨架样式」（fill/line/circle/symbol）。
 *         骨架样式来自首块瓦片的内省（discover source-layer 列表）。
 *
 * 纯非可视组件——不渲染 DOM，仅操作地图 API。
 */
import { watch, onUnmounted } from "vue";
import type { Map as MaplibreMap, LayerSpecification } from "maplibre-gl";
import { invoke } from "@tauri-apps/api/core";
import { ensureStoredTileProtocol, STORED_TILE_PROTO } from "~/composables/useStoredTileProtocol";
import { introspectMvtLayers } from "~/composables/useMvtStyleIntrospector";

// ─── Props ────────────────────────────────────────────────────────────────────

const props = defineProps<{
  map: MaplibreMap | null;
  taskId: string | null;
}>();

// ─── 图层 ID ──────────────────────────────────────────────────────────────────

const SOURCE_ID = "local-task-tiles-src";
const RASTER_LAYER_ID = "local-task-tiles";
// 矢量骨架图层使用前缀 + 序号生成唯一 ID
const VECTOR_LAYER_PREFIX = "local-task-vec-";

// ─── 取消令牌 ─────────────────────────────────────────────────────────────────

let generation = 0;
let vectorLayerIds: string[] = [];

// ─── 图层管理 ─────────────────────────────────────────────────────────────────

function removeLayers(m: MaplibreMap) {
  try {
    if (m.getLayer(RASTER_LAYER_ID)) m.removeLayer(RASTER_LAYER_ID);
    for (const id of vectorLayerIds) {
      if (m.getLayer(id)) m.removeLayer(id);
    }
    vectorLayerIds = [];
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
  sourceConfig: string;
}

function isVectorFormat(fmt: string | undefined): boolean {
  if (!fmt) return false;
  const f = fmt.toLowerCase();
  return f === "pbf" || f === "mvt";
}

function getFirstLabelLayerId(m: MaplibreMap): string | undefined {
  for (const layer of m.getStyle().layers ?? []) {
    if (layer.type === "symbol") return layer.id;
  }
  return undefined;
}

function pickInsertBefore(m: MaplibreMap): string | undefined {
  const OVERLAY_FILL = "task-overlay-fill";
  return m.getLayer(OVERLAY_FILL) ? OVERLAY_FILL : getFirstLabelLayerId(m);
}

async function addRasterLayer(m: MaplibreMap, taskId: string, task: BackendTask) {
  const tileUrl = `${STORED_TILE_PROTO}://${taskId}/{z}/{x}/{y}`;
  m.addSource(SOURCE_ID, {
    type: "raster",
    tiles: [tileUrl],
    tileSize: 256,
    minzoom: task.minZoom,
    maxzoom: task.maxZoom,
    bounds: [task.boundsWest, task.boundsSouth, task.boundsEast, task.boundsNorth],
    scheme: "xyz",
  });
  m.addLayer(
    {
      id: RASTER_LAYER_ID,
      type: "raster",
      source: SOURCE_ID,
      paint: { "raster-opacity": 1 },
    },
    pickInsertBefore(m),
  );
}

async function addVectorLayer(m: MaplibreMap, taskId: string, task: BackendTask) {
  const tileUrl = `${STORED_TILE_PROTO}://${taskId}/{z}/{x}/{y}`;
  m.addSource(SOURCE_ID, {
    type: "vector",
    tiles: [tileUrl],
    minzoom: task.minZoom,
    maxzoom: task.maxZoom,
    bounds: [task.boundsWest, task.boundsSouth, task.boundsEast, task.boundsNorth],
    scheme: "xyz",
  });

  // 内省一块瓦片以发现 source-layer 列表
  let layers: { id: string; hasPolygon: boolean; hasLine: boolean; hasPoint: boolean }[] = [];
  try {
    layers = await introspectMvtLayers(taskId, task.minZoom, task.maxZoom);
  } catch (e) {
    console.warn("[LocalTaskTileLayer] introspect MVT failed:", e);
  }

  if (layers.length === 0) {
    // 兜底：未能内省时，加一个万能 fill/line/circle，sourceLayer 留空字符串
    // 实际 MapLibre 会忽略 sourceLayer 缺失，这里改为加 3 个常见名称兜底
    layers = [
      { id: "default", hasPolygon: true, hasLine: true, hasPoint: true },
    ];
  }

  const before = pickInsertBefore(m);
  const palette = [
    "#cbd5e1", "#bfdbfe", "#fde68a", "#fecaca", "#bbf7d0",
    "#ddd6fe", "#fbcfe8", "#fed7aa", "#a5f3fc", "#e9d5ff",
  ];

  let idx = 0;
  for (const lyr of layers) {
    const color = palette[idx % palette.length];
    if (lyr.hasPolygon) {
      const id = `${VECTOR_LAYER_PREFIX}fill-${idx}`;
      const spec: LayerSpecification = {
        id,
        type: "fill",
        source: SOURCE_ID,
        "source-layer": lyr.id,
        filter: ["==", ["geometry-type"], "Polygon"],
        paint: {
          "fill-color": color,
          "fill-opacity": 0.45,
          "fill-outline-color": "#475569",
        },
      };
      m.addLayer(spec, before);
      vectorLayerIds.push(id);
    }
    if (lyr.hasLine || lyr.hasPolygon) {
      const id = `${VECTOR_LAYER_PREFIX}line-${idx}`;
      const spec: LayerSpecification = {
        id,
        type: "line",
        source: SOURCE_ID,
        "source-layer": lyr.id,
        filter: ["!=", ["geometry-type"], "Point"],
        paint: {
          "line-color": "#334155",
          "line-width": 0.6,
          "line-opacity": 0.75,
        },
      };
      m.addLayer(spec, before);
      vectorLayerIds.push(id);
    }
    if (lyr.hasPoint) {
      const id = `${VECTOR_LAYER_PREFIX}circle-${idx}`;
      const spec: LayerSpecification = {
        id,
        type: "circle",
        source: SOURCE_ID,
        "source-layer": lyr.id,
        filter: ["==", ["geometry-type"], "Point"],
        paint: {
          "circle-radius": 2.5,
          "circle-color": "#0f172a",
          "circle-stroke-color": "#ffffff",
          "circle-stroke-width": 1,
        },
      };
      m.addLayer(spec, before);
      vectorLayerIds.push(id);
    }
    idx++;
  }
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

  if (gen !== generation) return;
  if (task.downloadedTiles === 0) return;

  ensureStoredTileProtocol();

  // 解析任务源配置以获取 format
  let format = "png";
  try {
    const cfg = JSON.parse(task.sourceConfig);
    if (typeof cfg?.format === "string") format = cfg.format;
  } catch {
    /* 沿用默认 png */
  }

  try {
    if (isVectorFormat(format)) {
      await addVectorLayer(m, taskId, task);
    } else {
      await addRasterLayer(m, taskId, task);
    }
  } catch (e) {
    console.error("[LocalTaskTileLayer] addLayer failed:", e);
  }
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
  generation++;
  if (props.map) removeLayers(props.map);
});
</script>

<template><!-- 无可视内容，仅操作地图图层 --></template>
