<script setup lang="ts">
/**
 * DownloadProgressLayer
 *
 * 当任务正在下载时，在地图上实时闪烁显示每批刚下载完成的瓦片：
 * - 瓦片下载完成 → 在地图上以绿色高亮显示，持续约 1 秒后自动消失
 * - 未下载的瓦片不显示
 *
 * 仅在 status === 'downloading' 时激活；暂停/完成后清除图层。
 */
import { watch, onUnmounted } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { Map as MaplibreMap } from "maplibre-gl";

const props = defineProps<{
  map: MaplibreMap | null;
  taskId: string | null;
  taskStatus: string | null;
}>();

const SOURCE_ID = "tilegrab-flash-src";
const FILL_LAYER = "tilegrab-flash-fill";
const LINE_LAYER = "tilegrab-flash-line";

interface FlashTile {
  west: number;
  east: number;
  south: number;
  north: number;
}

interface TileFlashPayload {
  task_id: string;
  tiles: Array<{ west: number; east: number; south: number; north: number }>;
}

let generation = 0;
let flashPool: FlashTile[] = [];
let tickTimer: ReturnType<typeof setInterval> | null = null;
let unlistenFlash: UnlistenFn | null = null;

// ─── 图层管理 ─────────────────────────────────────────────────────────────────

function ensureLayers(m: MaplibreMap) {
  if (!m.getSource(SOURCE_ID)) {
    m.addSource(SOURCE_ID, {
      type: "geojson",
      data: { type: "FeatureCollection", features: [] },
    });
  }
  if (!m.getLayer(FILL_LAYER)) {
    m.addLayer({
      id: FILL_LAYER,
      type: "fill",
      source: SOURCE_ID,
      paint: {
        "fill-color": "#22c55e",
        "fill-opacity": 0.55,
      },
    });
  }
  if (!m.getLayer(LINE_LAYER)) {
    m.addLayer({
      id: LINE_LAYER,
      type: "line",
      source: SOURCE_ID,
      paint: {
        "line-color": "#16a34a",
        "line-width": 1,
        "line-opacity": 0.8,
      },
    });
  }
}

function removeLayers(m: MaplibreMap) {
  try {
    if (m.getLayer(LINE_LAYER)) m.removeLayer(LINE_LAYER);
    if (m.getLayer(FILL_LAYER)) m.removeLayer(FILL_LAYER);
    if (m.getSource(SOURCE_ID)) m.removeSource(SOURCE_ID);
  } catch {
    /* map already destroyed */
  }
}

// ─── GeoJSON 更新 ────────────────────────────────────────────────────────────

function tileToFeature(t: FlashTile): GeoJSON.Feature {
  return {
    type: "Feature",
    geometry: {
      type: "Polygon",
      coordinates: [
        [
          [t.west, t.north],
          [t.east, t.north],
          [t.east, t.south],
          [t.west, t.south],
          [t.west, t.north],
        ],
      ],
    },
    properties: {},
  };
}

function tick() {
  const m = props.map;
  if (!m) return;

  const src = m.getSource(SOURCE_ID) as maplibregl.GeoJSONSource | undefined;
  if (src) {
    src.setData({
      type: "FeatureCollection",
      features: flashPool.map(tileToFeature),
    });
  }
}

// ─── 事件监听 ─────────────────────────────────────────────────────────────────

async function startListening(taskId: string, m: MaplibreMap) {
  if (unlistenFlash) {
    unlistenFlash();
    unlistenFlash = null;
  }
  unlistenFlash = await listen<TileFlashPayload>(
    "tilegrab-tile-flash",
    (event) => {
      if (event.payload.task_id !== taskId) return;
      // 每批新瓦片替换旧 pool，让上一批一直可见直到新批到来
      flashPool = event.payload.tiles.map((tile) => ({ ...tile }));
      ensureLayers(m);
    },
  );
}

function stopListening() {
  if (unlistenFlash) {
    unlistenFlash();
    unlistenFlash = null;
  }
}

function startTick() {
  if (tickTimer) clearInterval(tickTimer);
  tickTimer = setInterval(tick, 80);
}

function stopTick() {
  if (tickTimer) {
    clearInterval(tickTimer);
    tickTimer = null;
  }
}

// ─── 监听 ─────────────────────────────────────────────────────────────────────

watch(
  () => [props.map, props.taskId, props.taskStatus] as const,
  ([m, id, status]) => {
    generation++;
    stopTick();
    stopListening();
    flashPool = [];
    if (m) tick(); // 清空图层内容

    if (!m || !id) {
      if (m) removeLayers(m);
      return;
    }

    if (status === "downloading") {
      ensureLayers(m);
      startTick();
      startListening(id, m);
    } else {
      removeLayers(m);
    }
  },
  { immediate: true },
);

onUnmounted(() => {
  generation++;
  stopTick();
  stopListening();
  flashPool = [];
  if (props.map) removeLayers(props.map);
});
</script>

<template><!-- 无可视内容，仅操作地图图层 --></template>
