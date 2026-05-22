<script setup lang="ts">
/**
 * FailedTilesLayer
 *
 * 当用户在 TaskDetail 选择某个 zoom 层级查看失败瓦片时，
 * 在地图上以红色透明矩形高亮所有失败瓦片位置。
 *
 * 数据源：useFailedTilesView() 共享 ref；后端命令 list_failed_tiles。
 */
import { watch, onUnmounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type { Map as MaplibreMap } from "maplibre-gl";
import { useFailedTilesView } from "~/composables/useFailedTilesView";

const props = defineProps<{ map: MaplibreMap | null }>();

const SOURCE_ID = "tilegrab-failed-src";
const FILL_LAYER = "tilegrab-failed-fill";
const LINE_LAYER = "tilegrab-failed-line";

interface FailedTileCoord {
  x: number;
  y: number;
}

const { view } = useFailedTilesView();
let generation = 0;

function tileBoundsWgs84(z: number, x: number, y: number) {
  const n = Math.pow(2, z);
  const west = (x / n) * 360 - 180;
  const east = ((x + 1) / n) * 360 - 180;
  const north =
    (Math.atan(Math.sinh(Math.PI * (1 - (2 * y) / n))) * 180) / Math.PI;
  const south =
    (Math.atan(Math.sinh(Math.PI * (1 - (2 * (y + 1)) / n))) * 180) / Math.PI;
  return { west, east, north, south };
}

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
      paint: { "fill-color": "#ef4444", "fill-opacity": 0.35 },
    });
  }
  if (!m.getLayer(LINE_LAYER)) {
    m.addLayer({
      id: LINE_LAYER,
      type: "line",
      source: SOURCE_ID,
      paint: {
        "line-color": "#b91c1c",
        "line-width": 1,
        "line-opacity": 0.9,
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

async function loadAndRender(
  m: MaplibreMap,
  taskId: string,
  zoom: number,
  gen: number,
) {
  let coords: FailedTileCoord[] = [];
  try {
    coords = await invoke<FailedTileCoord[]>("list_failed_tiles", {
      taskId,
      zoom,
    });
  } catch (e) {
    console.error("[FailedTilesLayer] list_failed_tiles failed:", e);
    return;
  }
  if (gen !== generation) return; // 异步回调过期

  ensureLayers(m);
  let minLng = Infinity,
    maxLng = -Infinity,
    minLat = Infinity,
    maxLat = -Infinity;
  const features: GeoJSON.Feature[] = coords.map((c) => {
    const b = tileBoundsWgs84(zoom, c.x, c.y);
    if (b.west < minLng) minLng = b.west;
    if (b.east > maxLng) maxLng = b.east;
    if (b.south < minLat) minLat = b.south;
    if (b.north > maxLat) maxLat = b.north;
    return {
      type: "Feature",
      geometry: {
        type: "Polygon",
        coordinates: [
          [
            [b.west, b.north],
            [b.east, b.north],
            [b.east, b.south],
            [b.west, b.south],
            [b.west, b.north],
          ],
        ],
      },
      properties: {},
    };
  });
  const src = m.getSource(SOURCE_ID) as maplibregl.GeoJSONSource | undefined;
  src?.setData({ type: "FeatureCollection", features });

  // 一键缩放到失败瓦片区
  if (features.length > 0 && isFinite(minLng) && isFinite(maxLng)) {
    try {
      m.fitBounds(
        [
          [minLng, minLat],
          [maxLng, maxLat],
        ],
        { padding: 80, maxZoom: zoom + 1, duration: 600 },
      );
    } catch (e) {
      console.warn("[FailedTilesLayer] fitBounds failed:", e);
    }
  }
}

watch(
  () => [props.map, view.value] as const,
  ([m, v]) => {
    generation++;
    if (!m) return;
    if (!v) {
      removeLayers(m);
      return;
    }
    loadAndRender(m, v.taskId, v.zoom, generation);
  },
  { immediate: true, deep: true },
);

onUnmounted(() => {
  generation++;
  if (props.map) removeLayers(props.map);
});
</script>

<template><!-- 无可视内容，仅操作地图图层 --></template>
