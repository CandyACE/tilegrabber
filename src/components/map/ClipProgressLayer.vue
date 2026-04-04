<script setup lang="ts">
/**
 * ClipProgressLayer
 *
 * 当任务正在执行精确裁剪（status === 'processing'）时，
 * 在地图上以紫色高亮显示每批正在裁剪的瓦片。
 * 监听 `tilegrab-clip-tiles` 事件，接收后立即更新图层。
 */
import { watch, onUnmounted } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { Map as MaplibreMap } from "maplibre-gl";

const props = defineProps<{
  map: MaplibreMap | null;
  taskId: string | null;
  taskStatus: string | null;
}>();

const SOURCE_ID = "tilegrab-clip-src";
const FILL_LAYER = "tilegrab-clip-fill";
const LINE_LAYER = "tilegrab-clip-line";

interface ClipTile {
  west: number;
  east: number;
  south: number;
  north: number;
}

interface ClipTilesPayload {
  task_id: string;
  tiles: ClipTile[];
}

let clipPool: ClipTile[] = [];
let tickTimer: ReturnType<typeof setInterval> | null = null;
let unlistenClip: UnlistenFn | null = null;

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
        "fill-color": "#8250df",
        "fill-opacity": 0.5,
      },
    });
  }
  if (!m.getLayer(LINE_LAYER)) {
    m.addLayer({
      id: LINE_LAYER,
      type: "line",
      source: SOURCE_ID,
      paint: {
        "line-color": "#6e40c9",
        "line-width": 1,
        "line-opacity": 0.85,
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

function tileToFeature(t: ClipTile): GeoJSON.Feature {
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
      features: clipPool.map(tileToFeature),
    });
  }
}

// ─── 事件监听 ─────────────────────────────────────────────────────────────────

async function startListening(taskId: string, m: MaplibreMap) {
  if (unlistenClip) {
    unlistenClip();
    unlistenClip = null;
  }
  unlistenClip = await listen<ClipTilesPayload>(
    "tilegrab-clip-tiles",
    (event) => {
      if (event.payload.task_id !== taskId) return;
      clipPool = event.payload.tiles.map((t) => ({ ...t }));
      ensureLayers(m);
    },
  );
}

function stopListening() {
  if (unlistenClip) {
    unlistenClip();
    unlistenClip = null;
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
    stopTick();
    stopListening();
    clipPool = [];
    if (m) tick();

    if (!m || !id) {
      if (m) removeLayers(m);
      return;
    }

    if (status === "processing") {
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
  stopTick();
  stopListening();
  clipPool = [];
  if (props.map) removeLayers(props.map);
});
</script>

<template><!-- 无可视内容，仅操作地图图层 --></template>
