<script setup lang="ts">
import { watch, onUnmounted } from "vue";
import maplibregl from "maplibre-gl";
import type { Map as MaplibreMap } from "maplibre-gl";
import { invoke } from "@tauri-apps/api/core";
import type { TileSource } from "~/types/tile-source";

// ─── Props ────────────────────────────────────────────────────────────────────

const props = defineProps<{
  map: MaplibreMap | null;
  source: TileSource | null;
}>();

// ─── 图层 ID / 协议 ───────────────────────────────────────────────────────────

const LAYER_ID = "tile-preview-raster";
const SOURCE_ID = "tile-preview-source";
const BOUNDS_SOURCE_ID = "tile-preview-bounds";
const BOUNDS_FILL_ID = "tile-preview-bounds-fill";
const BOUNDS_LINE_ID = "tile-preview-bounds-line";

// 自定义协议名：通过 Rust 代理携带自定义 headers 的瓦片请求
const TILE_PROTO = "tilegrab-preview";
// MBTiles 专用协议：从本地 .mbtiles 文件读取瓦片
const MBTILES_PROTO = "mbtiles-tile";

// 当前注入的 headers（供协议处理器读取）
let currentHeaders: Record<string, string> = {};
// 当前 MBTiles 文件路径（供协议处理器读取）
let currentMbtilesPath: string = "";
let protoRegistered = false;
let mbtilesProtoRegistered = false;

/** 注册一次自定义协议处理器 */
function ensureProtocol() {
  if (protoRegistered) return;
  protoRegistered = true;
  maplibregl.addProtocol(TILE_PROTO, async (params) => {
    // 将自定义协议 URL 还原为 https:// URL
    const url = params.url.replace(`${TILE_PROTO}://`, "https://");
    try {
      const bytes = await invoke<number[]>("fetch_tile", {
        url,
        headers: currentHeaders,
      });
      return { data: new Uint8Array(bytes).buffer };
    } catch (e) {
      console.error("[TilePreviewLayer] fetch_tile failed:", e, url);
      throw e;
    }
  });
}

/** 注册 MBTiles 瓦片协议处理器（一次性） */
function ensureMbtilesProtocol() {
  if (mbtilesProtoRegistered) return;
  mbtilesProtoRegistered = true;
  maplibregl.addProtocol(MBTILES_PROTO, async (params) => {
    // URL 格式：mbtiles-tile://tile/{z}/{x}/{y}
    const parts = params.url.replace(`${MBTILES_PROTO}://tile/`, "").split("/");
    const z = parseInt(parts[0], 10);
    const x = parseInt(parts[1], 10);
    const y = parseInt(parts[2], 10);
    try {
      const bytes = await invoke<number[]>("fetch_mbtiles_tile", {
        path: currentMbtilesPath,
        z,
        x,
        y,
      });
      return { data: new Uint8Array(bytes).buffer };
    } catch {
      // 该瓦片不存在时返回空（不报错，避免控制台污染）
      return { data: new Uint8Array(0).buffer };
    }
  });
}

// ─── 添加 / 更新预览图层 ──────────────────────────────────────────────────────

function addPreviewLayer(map: MaplibreMap, src: TileSource) {
  try {
    removePreviewLayer(map);

    let tileUrls: string[];
    const scheme = src.north_to_south ? "xyz" : "tms";

    if (src.kind === "mbtilefile") {
      // MBTiles：本地文件，通过自定义协议读取
      currentMbtilesPath = src.url_template;
      ensureMbtilesProtocol();
      tileUrls = [`${MBTILES_PROTO}://tile/{z}/{x}/{y}`];
    } else {
      // 展开 {s} 子域名为多个 URL
      if (src.subdomains.length > 0) {
        tileUrls = src.subdomains.map((s) =>
          src.url_template.replace(/\{s\}/g, s),
        );
      } else {
        tileUrls = [src.url_template];
      }

      // 所有 HTTP 请求统一走 Rust 后端代理，避免 WebView 请求被防火墙/CSP 拦截
      currentHeaders = { ...(src.headers ?? {}) };
      ensureProtocol();
      tileUrls = tileUrls.map((u) =>
        u.replace(/^https?:\/\//, `${TILE_PROTO}://`),
      );
    }

    console.log("[TilePreviewLayer] Adding preview layer:", {
      name: src.name,
      kind: src.kind,
      urls: tileUrls.slice(0, 2),
      scheme,
    });

    map.addSource(SOURCE_ID, {
      type: "raster",
      tiles: tileUrls,
      tileSize: src.tile_size || 256,
      minzoom: src.min_zoom ?? 0,
      maxzoom: src.max_zoom ?? 18,
      scheme,
      attribution: src.attribution ?? "",
    });

    const labelLayerId = getFirstLabelLayerId(map);
    map.addLayer(
      {
        id: LAYER_ID,
        type: "raster",
        source: SOURCE_ID,
        paint: { "raster-opacity": 1 },
      },
      labelLayerId,
    );

    addBoundsOverlay(map, src);
    fitToBounds(map, src);
  } catch (err) {
    console.error("[TilePreviewLayer] Failed to add preview layer:", err, src);
  }
}

function addBoundsOverlay(map: MaplibreMap, src: TileSource) {
  const { west, east, south, north } = src.bounds;

  const geojson: GeoJSON.FeatureCollection = {
    type: "FeatureCollection",
    features: [
      {
        type: "Feature",
        geometry: {
          type: "Polygon",
          coordinates: [
            [
              [west, south],
              [east, south],
              [east, north],
              [west, north],
              [west, south],
            ],
          ],
        },
        properties: {},
      },
    ],
  };

  map.addSource(BOUNDS_SOURCE_ID, { type: "geojson", data: geojson });

  // 半透明填充
  map.addLayer({
    id: BOUNDS_FILL_ID,
    type: "fill",
    source: BOUNDS_SOURCE_ID,
    paint: { "fill-color": "#3B82F6", "fill-opacity": 0.08 },
  });

  // 边框
  map.addLayer({
    id: BOUNDS_LINE_ID,
    type: "line",
    source: BOUNDS_SOURCE_ID,
    paint: {
      "line-color": "#3B82F6",
      "line-width": 2,
      "line-dasharray": [4, 2],
    },
  });
}

function removePreviewLayer(map: MaplibreMap) {
  for (const id of [LAYER_ID, BOUNDS_FILL_ID, BOUNDS_LINE_ID]) {
    if (map.getLayer(id)) map.removeLayer(id);
  }
  for (const id of [SOURCE_ID, BOUNDS_SOURCE_ID]) {
    if (map.getSource(id)) map.removeSource(id);
  }
  // 清空代理 headers
  currentHeaders = {};
}

function fitToBounds(map: MaplibreMap, src: TileSource) {
  const { west, east, south, north } = src.bounds;
  if (west < east && south < north) {
    // 如果当前相机中心已在图层范围内，无需飞行
    const center = map.getCenter();
    if (
      center.lng >= west &&
      center.lng <= east &&
      center.lat >= south &&
      center.lat <= north
    ) {
      return;
    }
    map.fitBounds(
      [west, south, east, north] as [number, number, number, number],
      {
        padding: 60,
        duration: 800,
      },
    );
  }
}

function getFirstLabelLayerId(map: MaplibreMap): string | undefined {
  const layers = map.getStyle()?.layers ?? [];
  return layers.find(
    (l) =>
      l.type === "symbol" && (l.id.includes("label") || l.id.includes("place")),
  )?.id;
}

// ─── 响应式监听 ───────────────────────────────────────────────────────────────

watch(
  () => [props.map, props.source] as const,
  ([map, src]) => {
    if (!map) return;
    console.log("[TilePreviewLayer] source changed:", src?.name ?? "null");
    if (src) {
      addPreviewLayer(map, src);
    } else {
      removePreviewLayer(map);
    }
  },
  { immediate: true },
);

onUnmounted(() => {
  if (props.map) removePreviewLayer(props.map);
});
</script>

<template>
  <!-- 纯逻辑组件，无 DOM 输出 -->
  <slot />
</template>
