<script setup lang="ts">
/**
 * BoundsRectOverlay.vue — 在地图上显示导入区域的轮廓（多边形优先，回退到矩形）
 * 非可视组件，通过 watch 管理 MapLibre 图层生命周期。
 */
import { watch, onUnmounted } from "vue";
import type { Map as MaplibreMap } from "maplibre-gl";
import type { Bounds } from "~/types/tile-source";

const props = defineProps<{
  map: MaplibreMap | null;
  bounds: Bounds | null;
  /** 实际多边形顶点 [lng, lat][]，有值时优先绘制真实轮廓而非矩形包围盒 */
  polygon?: [number, number][] | null;
}>();

const SOURCE_ID = "imported-bounds-src";
const FILL_ID = "imported-bounds-fill";
const LINE_ID = "imported-bounds-line";
// 多边形精确轮廓（独立 source）
const POLY_SOURCE_ID = "imported-poly-src";
const POLY_LINE_ID = "imported-poly-line";

function remove() {
  const m = props.map;
  if (!m) return;
  try {
    if (m.getLayer(POLY_LINE_ID)) m.removeLayer(POLY_LINE_ID);
    if (m.getSource(POLY_SOURCE_ID)) m.removeSource(POLY_SOURCE_ID);
    if (m.getLayer(LINE_ID)) m.removeLayer(LINE_ID);
    if (m.getLayer(FILL_ID)) m.removeLayer(FILL_ID);
    if (m.getSource(SOURCE_ID)) m.removeSource(SOURCE_ID);
  } catch {
    /* map 可能已销毁 */
  }
}

function show(bounds: Bounds) {
  const m = props.map;
  if (!m) return;
  remove();

  // 始终用矩形包围盒作为填充底图
  const { west: w, east: e, south: s, north: n } = bounds;
  const rectRing: [number, number][] = [
    [w, s],
    [e, s],
    [e, n],
    [w, n],
    [w, s],
  ];

  const rectGeojson: GeoJSON.FeatureCollection = {
    type: "FeatureCollection",
    features: [
      {
        type: "Feature",
        geometry: { type: "Polygon", coordinates: [rectRing] },
        properties: {},
      },
    ],
  };

  try {
    m.addSource(SOURCE_ID, { type: "geojson", data: rectGeojson });
    m.addLayer({
      id: FILL_ID,
      type: "fill",
      source: SOURCE_ID,
      paint: { "fill-color": "#10b981", "fill-opacity": 0.12 },
    });
    m.addLayer({
      id: LINE_ID,
      type: "line",
      source: SOURCE_ID,
      paint: {
        "line-color": "#10b981",
        "line-width": 2,
        "line-dasharray": [5, 3],
      },
    });

    // 如果有实际多边形，额外绘制精确轮廓
    if (props.polygon && props.polygon.length >= 3) {
      const ring: [number, number][] = [...props.polygon];
      const first = ring[0];
      const last = ring[ring.length - 1];
      if (first[0] !== last[0] || first[1] !== last[1]) {
        ring.push(first);
      }
      const polyGeojson: GeoJSON.FeatureCollection = {
        type: "FeatureCollection",
        features: [
          {
            type: "Feature",
            geometry: { type: "Polygon", coordinates: [ring] },
            properties: {},
          },
        ],
      };
      m.addSource(POLY_SOURCE_ID, { type: "geojson", data: polyGeojson });
      m.addLayer({
        id: POLY_LINE_ID,
        type: "line",
        source: POLY_SOURCE_ID,
        paint: {
          "line-color": "#f59e0b",
          "line-width": 2,
        },
      });
    }

    m.fitBounds(
      [
        [w, s],
        [e, n],
      ],
      { padding: 60, duration: 700 },
    );
  } catch {
    /* 忽略 */
  }
}

watch(
  () => [props.map, props.bounds, props.polygon] as const,
  ([m, b]) => {
    if (!m) return;
    b ? show(b) : remove();
  },
  { immediate: true },
);

onUnmounted(() => remove());
</script>

<template>
  <slot />
</template>
