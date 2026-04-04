<script setup lang="ts">
/**
 * 任务地图覆盖层
 *
 * 当选中某个任务时，在地图上绘制该任务的下载范围框并自动飞入视野。
 * 显示下载百分比标注（通过 watch updateData 保持实时同步）。
 */
import { watch, onUnmounted } from "vue";
import type { Map as MaplibreMap, LngLatBoundsLike } from "maplibre-gl";
import { invoke } from "@tauri-apps/api/core";

// ─── Props ────────────────────────────────────────────────────────────────────

const props = defineProps<{
  map: MaplibreMap | null;
  taskId: string | null;
}>();

// ─── 图层 ID ──────────────────────────────────────────────────────────────────

const SOURCE_ID = "task-overlay-src";
const FILL_ID = "task-overlay-fill";
const LINE_ID = "task-overlay-line";
// 实际多边形轮廓（独立 source，避免影响矩形图层）
const POLY_SOURCE_ID = "task-poly-src";
const POLY_LINE_ID = "task-poly-line";

// ─── 后端数据结构 ─────────────────────────────────────────────────────────────

interface BackendTask {
  boundsWest: number;
  boundsEast: number;
  boundsSouth: number;
  boundsNorth: number;
  downloadedTiles: number;
  totalTiles: number;
  status: string;
  polygonWgs84?: string | null;
}

// ─── 取消令牌（防止 async 在组件卸载后继续操作已销毁的地图）────────────────────

let generation = 0;

// ─── 图层管理 ─────────────────────────────────────────────────────────────────

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
    /* map already destroyed */
  }
}

async function showTask(taskId: string, gen: number, flyTo = true) {
  const m = props.map;
  if (!m) return;
  remove();

  let t: BackendTask;
  try {
    t = await invoke<BackendTask>("get_task", { taskId });
  } catch (e) {
    console.error("[TaskBoundsOverlay] get_task failed:", e);
    return;
  }

  // 若在 await 期间组件已卸载或 watch 已重跑，则放弃
  if (gen !== generation) return;

  const { boundsWest: w, boundsEast: e, boundsSouth: s, boundsNorth: n } = t;

  const geojson: GeoJSON.FeatureCollection = {
    type: "FeatureCollection",
    features: [
      {
        type: "Feature",
        geometry: {
          type: "Polygon",
          coordinates: [
            [
              [w, s],
              [e, s],
              [e, n],
              [w, n],
              [w, s],
            ],
          ],
        },
        properties: {},
      },
    ],
  };

  try {
    m.addSource(SOURCE_ID, { type: "geojson", data: geojson });
    m.addLayer({
      id: FILL_ID,
      type: "fill",
      source: SOURCE_ID,
      paint: { "fill-color": "#3b82f6", "fill-opacity": 0.1 },
    });
    m.addLayer({
      id: LINE_ID,
      type: "line",
      source: SOURCE_ID,
      paint: {
        "line-color": "#3b82f6",
        "line-width": 2,
        "line-dasharray": [5, 3],
      },
    });
    if (flyTo) {
      const bounds: LngLatBoundsLike = [
        [w, s],
        [e, n],
      ];
      m.fitBounds(bounds, { padding: 80, duration: 700, maxZoom: 14 });
    }

    // 实际多边形轮廓（如果有）
    if (t.polygonWgs84) {
      try {
        const pts: [number, number][] = JSON.parse(t.polygonWgs84);
        if (pts && pts.length >= 3) {
          const ring: [number, number][] = [...pts];
          if (
            ring[0][0] !== ring[ring.length - 1][0] ||
            ring[0][1] !== ring[ring.length - 1][1]
          ) {
            ring.push(ring[0]);
          }
          m.addSource(POLY_SOURCE_ID, {
            type: "geojson",
            data: {
              type: "FeatureCollection",
              features: [
                {
                  type: "Feature",
                  geometry: { type: "Polygon", coordinates: [ring] },
                  properties: {},
                },
              ],
            },
          });
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
      } catch {
        /* 多边形解析失败则忽略 */
      }
    }
  } catch (err) {
    console.error("[TaskBoundsOverlay] map op failed:", err);
  }
}

// ─── 监听变化 ─────────────────────────────────────────────────────────────────

watch(
  () => [props.map, props.taskId] as const,
  ([m, id]) => {
    generation++;
    if (m && id) {
      showTask(id, generation);
    } else {
      remove();
    }
  },
  { immediate: true },
);

onUnmounted(() => {
  generation++; // 令所有挂起的 async 失效
  remove();
});
</script>

<template><!-- 无可视内容，仅操作地图图层 --></template>
