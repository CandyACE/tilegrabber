<script setup lang="ts">
/**
 * MiniMapPreview.vue — 任务瓦片缩略预览
 *
 * 渲染一个不可交互的 MapLibre 小地图，显示给定任务的 bbox + 该任务已下载瓦片图层。
 */
import { ref, onMounted, onBeforeUnmount, watch } from "vue";
import maplibregl, { Map as MaplibreMap } from "maplibre-gl";
import { ensureStoredTileProtocol, STORED_TILE_PROTO } from "~/composables/useStoredTileProtocol";
import type { Bounds } from "~/types/tile-source";

const props = defineProps<{
  taskId: string;
  bounds: Bounds;
  format: string;
  tileSize?: number;
  showBasemap?: boolean;
}>();

const container = ref<HTMLDivElement | null>(null);
let map: MaplibreMap | null = null;

const SOURCE_ID = "preview-tiles";
const LAYER_ID = "preview-tiles-layer";

function mimeFromFormat(fmt: string): string {
  const f = fmt.toLowerCase();
  if (f.includes("jpg") || f.includes("jpeg")) return "image/jpeg";
  if (f.includes("webp")) return "image/webp";
  return "image/png";
}

function mountMap() {
  if (!container.value) return;
  ensureStoredTileProtocol();
  const tileSize =
    Number.isInteger(props.tileSize) &&
    (props.tileSize ?? 0) >= 64 &&
    (props.tileSize ?? 0) <= 4096
      ? props.tileSize!
      : 256;
  if (props.tileSize !== undefined && tileSize !== props.tileSize) {
    console.warn("[MiniMapPreview] 瓦片尺寸无效，已回退到 256", {
      taskId: props.taskId,
      tileSize: props.tileSize,
    });
  }
  console.info("[MiniMapPreview] 初始化任务缩略图", {
    taskId: props.taskId,
    tileSize,
  });

  map = new maplibregl.Map({
    container: container.value,
    style: {
      version: 8,
      sources: props.showBasemap === false
        ? {}
        : {
            bg: {
              type: "raster",
              tiles: ["https://a.tile.openstreetmap.org/{z}/{x}/{y}.png"],
              tileSize: 256,
              attribution: "© OSM",
            },
          },
      layers:
        props.showBasemap === false
          ? []
          : [{ id: "bg", type: "raster", source: "bg" }],
    },
    interactive: false,
    attributionControl: false,
  });

  map.on("load", () => {
    if (!map) return;
    map.addSource(SOURCE_ID, {
      type: "raster",
      tiles: [`${STORED_TILE_PROTO}://${props.taskId}/{z}/{x}/{y}`],
      tileSize,
      scheme: "xyz",
    });
    (map.getSource(SOURCE_ID) as any).type = "raster";
    map.addLayer({
      id: LAYER_ID,
      type: "raster",
      source: SOURCE_ID,
      paint: { "raster-opacity": 0.9 },
    });
    fitToBounds();
  });
}

function fitToBounds() {
  if (!map) return;
  const b = props.bounds;
  map.fitBounds(
    [
      [b.west, b.south],
      [b.east, b.north],
    ],
    { padding: 6, duration: 0, animate: false },
  );
}

onMounted(mountMap);

watch(
  () => [props.taskId, props.bounds.west, props.bounds.east, props.bounds.south, props.bounds.north],
  () => {
    if (!map) return;
    fitToBounds();
    const src = map.getSource(SOURCE_ID) as any;
    if (src && typeof src.setTiles === "function") {
      src.setTiles([`${STORED_TILE_PROTO}://${props.taskId}/{z}/{x}/{y}`]);
    }
  },
);

onBeforeUnmount(() => {
  map?.remove();
  map = null;
});

// suppress unused warning for mime helper (kept for future tile content-type handling)
void mimeFromFormat;
</script>

<template>
  <div ref="container" class="w-full h-full rounded bg-slate-100" />
</template>
