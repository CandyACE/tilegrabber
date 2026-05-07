<script setup lang="ts">
import { watch, onUnmounted } from "vue";
import maplibregl from "maplibre-gl";
import type { Map as MaplibreMap } from "maplibre-gl";
import { invoke } from "@tauri-apps/api/core";
import type { TileSource } from "~/types/tile-source";
import { gcj02PixelDelta } from "~/lib/gcj02";

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
// GCJ02 纠偏协议：对高德等 GCJ02 来源做像素级拼接纠偏
const GCJ02_PROTO = "tilegrab-gcj02";

// 当前注入的 headers（供协议处理器读取）
let currentHeaders: Record<string, string> = {};
// 当前 MBTiles 文件路径（供协议处理器读取）
let currentMbtilesPath: string = "";
let protoRegistered = false;
let mbtilesProtoRegistered = false;
let gcj02ProtoRegistered = false;

// 取消令牌：防止 async addPreviewLayer 在 source 切换或组件卸载后继续操作地图
let addGen = 0;

// ─── GCJ02 纠偏工具函数 ───────────────────────────────────────────────────────

/**
 * 从瓦片 URL 中解析 (z, x, y)。
 * 支持查询参数格式（高德：?x=...&y=...&z=...）和路径格式（/{z}/{x}/{y}）。
 */
function parseTileCoords(url: string): { z: number; x: number; y: number } | null {
  try {
    const u = new URL(url.startsWith("http") ? url : "https://" + url.replace(/^[^/]*\/\//, ""));
    const zs = u.searchParams.get("z");
    const xs = u.searchParams.get("x");
    const ys = u.searchParams.get("y");
    if (zs && xs && ys) {
      const z = parseInt(zs), x = parseInt(xs), y = parseInt(ys);
      if (!isNaN(z) && !isNaN(x) && !isNaN(y)) return { z, x, y };
    }
  } catch { /* fall through */ }

  const m = url.match(/\/(\d+)\/(\d+)\/(\d+)(?:\.\w+)?(?:[?#]|$)/);
  if (m) return { z: +m[1], x: +m[2], y: +m[3] };
  return null;
}

/**
 * 将瓦片 URL 中的 (origX, origY) 替换为 (newX, newY)，z 保持不变。
 */
function replaceTileXY(
  url: string,
  origX: number,
  origY: number,
  origZ: number,
  newX: number,
  newY: number,
): string {
  // 查询参数格式（高德等）
  try {
    const schemeMatch = url.match(/^([a-zA-Z][a-zA-Z0-9+\-.]*:\/\/)/);
    const scheme = schemeMatch?.[1] ?? "";
    const u = new URL("https://" + url.slice(scheme.length).replace(/^\/\//, ""));
    if (u.searchParams.has("x") && u.searchParams.has("y")) {
      u.searchParams.set("x", String(newX));
      u.searchParams.set("y", String(newY));
      return scheme + u.toString().slice("https://".length);
    }
  } catch { /* fall through */ }

  // 路径格式：先替换 x 再替换 y（防止数字碰撞）
  const withNewX = url.replace(
    new RegExp(`(/${origZ}/)${origX}(/${origY}(?:\\.|[?#]|$))`),
    `$1${newX}$2`,
  );
  return withNewX.replace(
    new RegExp(`(/${origZ}/${newX}/)${origY}(?=\\.|[?#]|$)`),
    `$1${newY}`,
  );
}

/**
 * 注册 GCJ02 纠偏协议（一次性）。
 *
 * 原理：
 *   对于 MapLibre 请求的每个 WGS84 瓦片 (z, x, y)，
 *   计算 GCJ02 坐标偏移量 (dx, dy)，从高德取最多 2×2 邻接瓦片，
 *   在 OffscreenCanvas 上按偏移量拼接合成，输出对齐 WGS84 的 PNG。
 *
 * 仅适用于 XYZ (north_to_south=true) 方案的来源。
 */
function ensureGcj02Protocol() {
  if (gcj02ProtoRegistered) return;
  gcj02ProtoRegistered = true;

  maplibregl.addProtocol(GCJ02_PROTO, async (params) => {
    // 还原 URL：协议格式为 tilegrab-gcj02://{scheme}/{host/path}
    const rawGcj = params.url.slice(`${GCJ02_PROTO}://`.length);
    const gcjSlash = rawGcj.indexOf('/');
    const gcjScheme = gcjSlash > 0 ? rawGcj.slice(0, gcjSlash) : "https";
    const url = gcjScheme + "://" + rawGcj.slice(gcjSlash + 1);
    const headers = { ...currentHeaders };

    const coords = parseTileCoords(url);
    if (!coords) {
      // 无法解析坐标，直接代理（降级）
      const bytes = await invoke<number[]>("fetch_tile", { url, headers });
      return { data: new Uint8Array(bytes).buffer };
    }

    const { z, x, y } = coords;
    const { dx, dy } = gcj02PixelDelta(z, x, y);

    // 源瓦片的粗偏移（整数瓦片数）和细偏移（瓦片内像素，0..255）
    const tileOffX = Math.floor(dx / 256);
    const tileOffY = Math.floor(dy / 256);
    const subX = ((dx % 256) + 256) % 256;
    const subY = ((dy % 256) + 256) % 256;

    // 需要合成的 ≤ 2×2 邻接源瓦片（相对于基准偏移）
    const needed: Array<[number, number]> = [[0, 0]];
    if (subX > 0) needed.push([1, 0]);
    if (subY > 0) needed.push([0, 1]);
    if (subX > 0 && subY > 0) needed.push([1, 1]);

    const fetchBitmap = async (i: number, j: number): Promise<ImageBitmap | null> => {
      const tileX = x + tileOffX + i;
      const tileY = y + tileOffY + j;
      const tileUrl = replaceTileXY(url, x, y, z, tileX, tileY);
      try {
        const bytes = await invoke<number[]>("fetch_tile", { url: tileUrl, headers });
        const blob = new Blob([new Uint8Array(bytes)]);
        return await createImageBitmap(blob);
      } catch {
        return null;
      }
    };

    const bitmaps = await Promise.all(needed.map(([i, j]) => fetchBitmap(i, j)));

    // 合成到 256×256 画布
    const canvas = new OffscreenCanvas(256, 256);
    const ctx = canvas.getContext("2d")!;

    for (let k = 0; k < needed.length; k++) {
      const [i, j] = needed[k];
      const bitmap = bitmaps[k];
      if (bitmap) {
        // 瓦片在画布上的绘制起点：(i*256 - subX, j*256 - subY)
        ctx.drawImage(bitmap, i * 256 - subX, j * 256 - subY);
        bitmap.close();
      }
    }

    const resultBlob = await canvas.convertToBlob({ type: "image/png" });
    return { data: await resultBlob.arrayBuffer() };
  });
}

/** 注册一次自定义协议处理器 */
function ensureProtocol() {
  if (protoRegistered) return;
  protoRegistered = true;
  maplibregl.addProtocol(TILE_PROTO, async (params) => {
    // 协议 URL 格式：tilegrab-preview://{scheme}/{host/path}
    // 从中还原完整 HTTP/HTTPS URL
    const raw = params.url.slice(`${TILE_PROTO}://`.length);
    const slashIdx = raw.indexOf('/');
    const scheme = slashIdx > 0 ? raw.slice(0, slashIdx) : "https";
    const url = scheme + "://" + raw.slice(slashIdx + 1);
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

async function addPreviewLayer(map: MaplibreMap, src: TileSource) {
  // 快照当前 generation：若在 await 期间 source 切换或组件卸载，则丢弃本次结果
  const gen = ++addGen;

  try {
    removePreviewLayer(map);

    let tileUrls: string[];
    let scheme = src.north_to_south ? "xyz" : "tms";
    let boundsForOverlay = src.bounds;
    let minZoom = src.min_zoom ?? 0;
    let maxZoom = src.max_zoom ?? 18;

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

      // GCJ02 来源（高德等）：始终走在线实时纠偏协议，确保图层预览覆盖完整数据源范围
      const isGcj02 = src.coord_type === "GCJ02" && src.north_to_south;
      if (isGcj02) {
        ensureGcj02Protocol();
        // 保留原始 scheme：tilegrab-gcj02://{http|https}/{host/path}
        tileUrls = tileUrls.map((u) => u.replace(/^(https?):\/\//, `${GCJ02_PROTO}://$1/`));
      } else {
        ensureProtocol();
        // 保留原始 scheme：tilegrab-preview://{http|https}/{host/path}
        tileUrls = tileUrls.map((u) => u.replace(/^(https?):\/\//, `${TILE_PROTO}://$1/`));
      }
    }

    if (gen !== addGen) return; // stale

    console.log("[TilePreviewLayer] Adding preview layer:", {
      name: src.name,
      kind: src.kind,
      coord_type: src.coord_type,
      urls: tileUrls.slice(0, 2),
      scheme,
    });

    map.addSource(SOURCE_ID, {
      type: "raster",
      tiles: tileUrls,
      tileSize: src.tile_size || 256,
      minzoom: minZoom,
      maxzoom: maxZoom,
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

    addBoundsOverlay(map, boundsForOverlay);
    fitToBounds(map, boundsForOverlay);
  } catch (err) {
    console.error("[TilePreviewLayer] Failed to add preview layer:", err, src);
  }
}

type Bounds = { west: number; east: number; south: number; north: number };

function addBoundsOverlay(map: MaplibreMap, bounds: Bounds) {
  const { west, east, south, north } = bounds;

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

function fitToBounds(map: MaplibreMap, bounds: Bounds) {
  const { west, east, south, north } = bounds;
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
    addGen++; // invalidate any pending async addPreviewLayer
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
  addGen++; // invalidate any pending async addPreviewLayer
  if (props.map) removePreviewLayer(props.map);
});
</script>

<template>
  <!-- 纯逻辑组件，无 DOM 输出 -->
  <slot />
</template>
