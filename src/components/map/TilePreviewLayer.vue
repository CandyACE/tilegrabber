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

// 当前注入的 headers（供协议处理器读取）
let currentHeaders: Record<string, string> = {};
let protoRegistered = false;

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

// ─── 添加 / 更新预览图层 ──────────────────────────────────────────────────────

function addPreviewLayer(map: MaplibreMap, src: TileSource) {
  try {
    removePreviewLayer(map);

    // 展开 {s} 子域名为多个 URL
    let tileUrls: string[];
    if (src.subdomains.length > 0) {
      tileUrls = src.subdomains.map((s) =>
        src.url_template.replace(/\{s\}/g, s),
      );
    } else {
      tileUrls = [src.url_template];
    }

    // CRS 判断
    const scheme = src.north_to_south ? "xyz" : "tms";

    console.log("[TilePreviewLayer] Adding preview layer:", {
      name: src.name,
      urls: tileUrls.slice(0, 2),
      scheme,
      tileSize: src.tile_size,
      bounds: src.bounds,
      headers: Object.keys(src.headers ?? {}),
    });

    // 所有请求统一走 Rust 后端代理，避免 WebView 请求被防火墙/CSP 拦截
    currentHeaders = { ...(src.headers ?? {}) };
    ensureProtocol();
    // 将 https:// 或 http:// 替换为自定义协议前缀
    tileUrls = tileUrls.map((u) =>
      u.replace(/^https?:\/\//, `${TILE_PROTO}://`),
    );

    // 添加 raster 数据源
    map.addSource(SOURCE_ID, {
      type: "raster",
      tiles: tileUrls,
      tileSize: src.tile_size || 256,
      minzoom: src.min_zoom ?? 0,
      maxzoom: src.max_zoom ?? 18,
      scheme,
      attribution: src.attribution ?? "",
    });

    // 添加渲染图层（置于道路标注下方）
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

    // 添加范围框
    addBoundsOverlay(map, src);

    // 飞到范围
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
