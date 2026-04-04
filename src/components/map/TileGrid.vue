<script setup lang="ts">
/**
 * TileGrid.vue — 瓦片网格预览图层
 *
 * 非可视组件：在 MapLibre 地图上渲染指定层级的瓦片网格 GeoJSON。
 * 瓦片数 > 500 时自动降级为只显示范围框。
 */
import { ref, watch, onUnmounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { Map as MaplibreMap } from 'maplibre-gl'
import type { Bounds, CrsType } from '~/types/tile-source'

const SOURCE_ID = "tilegrab-grid-source";
const FILL_LAYER_ID = "tilegrab-grid-fill";
const LINE_LAYER_ID = "tilegrab-grid-line";

const props = defineProps<{
  map: MaplibreMap;
  bounds: Bounds | null;
  zoom: number;
  crs?: CrsType;
  /** 是否显示网格（可由父组件控制） */
  visible?: boolean;
}>();

const loading = ref(false);

// 初始化地图图层
function ensureLayers() {
  const map = props.map;
  if (!map.getSource(SOURCE_ID)) {
    map.addSource(SOURCE_ID, {
      type: "geojson",
      data: { type: "FeatureCollection", features: [] },
    });
  }
  if (!map.getLayer(FILL_LAYER_ID)) {
    map.addLayer({
      id: FILL_LAYER_ID,
      type: "fill",
      source: SOURCE_ID,
      paint: {
        "fill-color": "#0969DA",
        "fill-opacity": 0.04,
      },
    });
  }
  if (!map.getLayer(LINE_LAYER_ID)) {
    map.addLayer({
      id: LINE_LAYER_ID,
      type: "line",
      source: SOURCE_ID,
      paint: {
        "line-color": "#0969DA",
        "line-width": 0.8,
        "line-opacity": 0.6,
        "line-dasharray": [3, 3],
      },
    });
  }
}

function removeLayers() {
  const map = props.map;
  if (map.getLayer(LINE_LAYER_ID)) map.removeLayer(LINE_LAYER_ID);
  if (map.getLayer(FILL_LAYER_ID)) map.removeLayer(FILL_LAYER_ID);
  if (map.getSource(SOURCE_ID)) map.removeSource(SOURCE_ID);
}

function setVisible(v: boolean) {
  const map = props.map;
  const vis = v ? "visible" : "none";
  if (map.getLayer(FILL_LAYER_ID))
    map.setLayoutProperty(FILL_LAYER_ID, "visibility", vis);
  if (map.getLayer(LINE_LAYER_ID))
    map.setLayoutProperty(LINE_LAYER_ID, "visibility", vis);
}

async function updateGrid() {
  const map = props.map;
  if (!map.loaded()) return;

  ensureLayers();

  if (!props.bounds || props.visible === false) {
    const src = map.getSource(SOURCE_ID) as
      | maplibregl.GeoJSONSource
      | undefined;
    src?.setData({ type: "FeatureCollection", features: [] });
    setVisible(false);
    return;
  }

  setVisible(true);
  loading.value = true;

  try {
    const result = await invoke<{ geojson: string; tile_count: number }>(
      "generate_tile_grid",
      {
        bounds: props.bounds,
        zoom: props.zoom,
        crs: props.crs ?? null,
      },
    );

    const src = map.getSource(SOURCE_ID) as
      | maplibregl.GeoJSONSource
      | undefined;
    if (src) {
      src.setData(JSON.parse(result.geojson));
    }
  } catch (e) {
    console.warn("generate_tile_grid failed:", e);
  } finally {
    loading.value = false;
  }
}

// props 变化时刷新
watch(
  [() => props.bounds, () => props.zoom, () => props.crs, () => props.visible],
  () => updateGrid(),
  { immediate: true },
);

onUnmounted(() => {
  try {
    removeLayers();
  } catch {}
});
</script>

<template>
  <!-- 非可视组件 -->
  <slot />
</template>
