<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch } from "vue";
import { useI18n } from "vue-i18n";
import { invoke } from "@tauri-apps/api/core";
import {
  Power,
  Copy,
  Check,
  Globe,
  Layers,
  Server,
  ChevronDown,
  BarChart2,
} from "lucide-vue-next";

// ─── 类型 ────────────────────────────────────────────────────────────────────
interface Task {
  id: string;
  name: string;
  status: string;
  downloadedTiles: number;
  minZoom: number;
  maxZoom: number;
  boundsWest: number;
  boundsEast: number;
  boundsSouth: number;
  boundsNorth: number;
  tileStorePath: string | null;
}

interface ServerStatus {
  running: boolean;
  port: number;
  baseUrl: string;
  lanUrls: string[];
}

interface ServiceStats {
  tmsRequests: number;
  wmtsRequests: number;
  wmsRequests: number;
  ogcRequests: number;
  arcgisRequests: number;
  lastRequestAt: number | null;
}

interface TaskStatsDto {
  taskId: string;
  tmsRequests: number;
  wmtsRequests: number;
  wmsRequests: number;
  ogcRequests: number;
  arcgisRequests: number;
  lastRequestAt: number | null;
}

const { t } = useI18n();

// ─── 状态 ────────────────────────────────────────────────────────────────────
const tasks = ref<Task[]>([]);
const serverStatus = ref<ServerStatus>({
  running: false,
  port: 8765,
  baseUrl: "http://localhost:8765",
  lanUrls: [],
});
const port = ref(8765);
const toggling = ref(false);
const errorMsg = ref("");

// 当前选择的访问地址（默认 localhost，可切换到局域网 IP）
const selectedBase = ref("http://localhost:8765");

// 服务器状态更新时同步 selectedBase（若当前值已无效则重置）
watch(serverStatus, (s) => {
  const allBases = [s.baseUrl, ...(s.lanUrls ?? [])];
  if (!allBases.includes(selectedBase.value)) {
    selectedBase.value = s.baseUrl;
  }
}, { deep: true });

// 复制成功反馈：存储已复制的 url
const copied = ref<string | null>(null);

// 服务统计：taskId → stats
const serviceStats = ref<Record<string, TaskStatsDto>>({});
let statsInterval: ReturnType<typeof setInterval> | null = null;

async function refreshStats() {
  try {
    const list = await invoke<TaskStatsDto[]>("get_service_stats");
    const map: Record<string, TaskStatsDto> = {};
    for (const item of list) map[item.taskId] = item;
    serviceStats.value = map;
  } catch {
    // 服务未运行时忽略错误
  }
}

function getStats(taskId: string): TaskStatsDto {
  return serviceStats.value[taskId] ?? {
    taskId,
    tmsRequests: 0,
    wmtsRequests: 0,
    wmsRequests: 0,
    ogcRequests: 0,
    arcgisRequests: 0,
    lastRequestAt: null,
  };
}

function totalRequests(taskId: string): number {
  const s = getStats(taskId);
  return s.tmsRequests + s.wmtsRequests + s.wmsRequests + s.ogcRequests + s.arcgisRequests;
}

function formatLastRequest(ts: number | null): string {
  if (ts === null) return t('publish.stats.noRequests');
  const diff = Math.floor(Date.now() / 1000) - ts;
  if (diff < 60) return t('publish.stats.justNow');
  if (diff < 3600) return t('publish.stats.minutesAgo', { n: Math.floor(diff / 60) });
  return t('publish.stats.hoursAgo', { n: Math.floor(diff / 3600) });
}

watch(
  () => serverStatus.value.running,
  (running) => {
    if (running) {
      refreshStats();
      statsInterval = setInterval(refreshStats, 5000);
    } else {
      if (statsInterval !== null) {
        clearInterval(statsInterval);
        statsInterval = null;
      }
    }
  },
);

onUnmounted(() => {
  if (statsInterval !== null) clearInterval(statsInterval);
});

// 缩略图缓存：task.id → data URL（空串表示无可用缩略图，undefined 表示加载中）
const thumbnails = ref<Record<string, string>>({});

// 计算经纬度对应的 Web Mercator 瓦片坐标（EPSG:3857）
function lngLatToTileXY(
  lng: number,
  lat: number,
  z: number,
): { x: number; y: number } {
  const n = 2 ** z;
  const x = Math.floor(((lng + 180) / 360) * n);
  const latRad = (lat * Math.PI) / 180;
  const y = Math.floor(
    ((1 - Math.log(Math.tan(latRad) + 1 / Math.cos(latRad)) / Math.PI) / 2) *
      n,
  );
  return { x: Math.max(0, Math.min(n - 1, x)), y: Math.max(0, Math.min(n - 1, y)) };
}

async function loadThumbnail(task: Task) {
  if (task.id in thumbnails.value) return;
  try {
    const bytes = await invoke<number[]>("get_task_thumbnail", {
      taskId: task.id,
      size: 256,
    });
    const ua = new Uint8Array(bytes);
    const b64 = btoa(Array.from(ua, (b) => String.fromCharCode(b)).join(""));
    thumbnails.value = {
      ...thumbnails.value,
      [task.id]: `data:image/png;base64,${b64}`,
    };
  } catch {
    thumbnails.value = { ...thumbnails.value, [task.id]: "" };
  }
}


const readyTasks = computed(() =>
  tasks.value.filter(
    (t) =>
      (t.status === "completed" || t.status === "completed_with_errors") &&
      t.tileStorePath,
  ),
);

watch(readyTasks, (tasks) => tasks.forEach(loadThumbnail), { immediate: true });

// ─── 初始化 ──────────────────────────────────────────────────────────────────
onMounted(async () => {
  await refresh();
});

async function refresh() {
  try {
    const [status, taskList] = await Promise.all([
      invoke<ServerStatus>("get_server_status"),
      invoke<Task[]>("list_tasks"),
    ]);
    serverStatus.value = status;
    port.value = status.port;
    tasks.value = taskList;
  } catch (e) {
    console.error(e);
  }
}

// ─── 服务器开关 ───────────────────────────────────────────────────────────────
async function toggleServer() {
  toggling.value = true;
  errorMsg.value = "";
  try {
    if (serverStatus.value.running) {
      await invoke("stop_tile_server");
      serverStatus.value = {
        running: false,
        port: port.value,
        baseUrl: `http://localhost:${port.value}`,
        lanUrls: [],
      };
    } else {
      const result = await invoke<ServerStatus>("start_tile_server", {
        port: port.value,
      });
      serverStatus.value = result;
    }
  } catch (e: unknown) {
    errorMsg.value = String(e);
  } finally {
    toggling.value = false;
  }
}

// ─── URL 构建 ─────────────────────────────────────────────────────────────────
function tmsUrl(taskId: string) {
  return `${selectedBase.value}/tiles/${taskId}/{z}/{x}/{y}`;
}

function wmtsUrl(taskId: string) {
  return `${selectedBase.value}/wmts/${taskId}?SERVICE=WMTS&REQUEST=GetCapabilities`;
}

function wmsUrl(taskId: string) {
  return `${selectedBase.value}/wms/${taskId}?SERVICE=WMS&REQUEST=GetCapabilities`;
}

function ogcTilesUrl(taskId: string) {
  return `${selectedBase.value}/ogc-tiles/${taskId}/tiles/WebMercatorQuad`;
}

function arcgisUrl(taskId: string) {
  return `${selectedBase.value}/arcgis/rest/services/${taskId}/MapServer`;
}

function wmsGetMapUrl(taskId: string, task: Task) {
  const cx = (task.boundsWest + task.boundsEast) / 2;
  const cy = (task.boundsSouth + task.boundsNorth) / 2;
  const hw = Math.min((task.boundsEast - task.boundsWest) / 4, 1);
  const hh = Math.min((task.boundsNorth - task.boundsSouth) / 4, 1);
  const bbox = `${(cx-hw).toFixed(4)},${(cy-hh).toFixed(4)},${(cx+hw).toFixed(4)},${(cy+hh).toFixed(4)}`;
  return `${selectedBase.value}/wms/${taskId}?SERVICE=WMS&REQUEST=GetMap&VERSION=1.1.1&LAYERS=${taskId}&STYLES=&SRS=EPSG:4326&BBOX=${bbox}&WIDTH=512&HEIGHT=512&FORMAT=image/png`;
}

// ─── 复制 URL ─────────────────────────────────────────────────────────────────
async function copyUrl(url: string) {
  try {
    await navigator.clipboard.writeText(url);
  } catch {
    // 某些环境 clipboard API 不可用，用 execCommand 兜底
    const el = document.createElement("textarea");
    el.value = url;
    document.body.appendChild(el);
    el.select();
    document.execCommand("copy");
    document.body.removeChild(el);
  }
  copied.value = url;
  setTimeout(() => {
    copied.value = null;
  }, 2000);
}

// ─── 代码示例 ─────────────────────────────────────────────────────────────────
const FRAMEWORKS = ["Cesium", "Leaflet", "MapLibre"] as const;
type Framework = (typeof FRAMEWORKS)[number];

// 协议切换：'tms' | 'wmts' | 'wms' | 'ogc' | 'arcgis'
const activeProtocol = ref<Record<string, "tms" | "wmts" | "wms" | "ogc" | "arcgis">>({});
const activeFramework = ref<Record<string, Framework>>({});
const copiedCode = ref<string | null>(null);
const expandedCode = ref<Record<string, boolean>>({});

function getActiveFramework(taskId: string): Framework {
  return activeFramework.value[taskId] ?? "Cesium";
}

function setActiveFramework(taskId: string, fw: Framework) {
  activeFramework.value = { ...activeFramework.value, [taskId]: fw };
}

function getActiveProtocol(taskId: string): "tms" | "wmts" | "wms" | "ogc" | "arcgis" {
  return activeProtocol.value[taskId] ?? "tms";
}

function setActiveProtocol(taskId: string, proto: "tms" | "wmts" | "wms" | "ogc" | "arcgis") {
  activeProtocol.value = { ...activeProtocol.value, [taskId]: proto };
}

function isCodeExpanded(taskId: string): boolean {
  return expandedCode.value[taskId] ?? false;
}

function toggleCodeExpanded(taskId: string) {
  expandedCode.value = {
    ...expandedCode.value,
    [taskId]: !isCodeExpanded(taskId),
  };
}

// ── TMS 代码 ──────────────────────────────────────────────────────────────────

function cesiumCode(task: Task): string {
  const url = tmsUrl(task.id);
  const name = task.name.replace(/'/g, "\\'");
  return `// Cesium.js — UrlTemplateImageryProvider
const viewer = new Cesium.Viewer('cesiumContainer');
viewer.imageryLayers.addImageryProvider(
  new Cesium.UrlTemplateImageryProvider({
    url: '${url}',
    minimumLevel: ${task.minZoom},
    maximumLevel: ${task.maxZoom},
    credit: '${name}',
  })
);`;
}

function leafletCode(task: Task): string {
  const url = tmsUrl(task.id);
  const name = task.name.replace(/'/g, "\\'");
  return `// Leaflet.js — L.tileLayer
const map = L.map('map');
L.tileLayer('${url}', {
  minZoom: ${task.minZoom},
  maxZoom: ${task.maxZoom},
  attribution: '${name}',
}).addTo(map);`;
}

function maplibreCode(task: Task): string {
  const url = tmsUrl(task.id);
  return `// MapLibre GL JS — raster source
const map = new maplibregl.Map({
  container: 'map',
  style: {
    version: 8,
    sources: {
      local: {
        type: 'raster',
        tiles: ['${url}'],
        tileSize: 256,
        minzoom: ${task.minZoom},
        maxzoom: ${task.maxZoom},
      },
    },
    layers: [{ id: 'local', type: 'raster', source: 'local' }],
  },
});`;
}

// ── WMTS 代码 ─────────────────────────────────────────────────────────────────

function cesiumWmtsCode(task: Task): string {
  const caps = wmtsUrl(task.id);
  const name = task.name.replace(/'/g, "\\'");
  return `// Cesium.js — WebMapTileServiceImageryProvider
const viewer = new Cesium.Viewer('cesiumContainer');
viewer.imageryLayers.addImageryProvider(
  new Cesium.WebMapTileServiceImageryProvider({
    url: '${selectedBase.value}/wmts/${task.id}',
    layer: '${task.id}',
    style: 'default',
    tileMatrixSetID: 'WebMercatorQuad',
    format: 'image/png',
    minimumLevel: ${task.minZoom},
    maximumLevel: ${task.maxZoom},
    // GetCapabilities: '${caps}'
    credit: '${name}',
  })
);`;
}

function leafletWmtsCode(task: Task): string {
  const base = `${selectedBase.value}/wmts/${task.id}`;
  const name = task.name.replace(/'/g, "\\'");
  return `// Leaflet.js — leaflet-tilelayer-wmts (或手动拼接)
// 推荐使用 leaflet-tilelayer-wmts 插件:
// https://github.com/mylen/leaflet.TileLayer.WMTS
const map = L.map('map');
const wmtsUrl = '${base}?SERVICE=WMTS&VERSION=1.0.0&REQUEST=GetTile' +
  '&LAYER=${task.id}&STYLE=default&FORMAT=image/png' +
  '&TILEMATRIXSET=WebMercatorQuad&TILEMATRIX={z}&TILEROW={y}&TILECOL={x}';
L.tileLayer(wmtsUrl, {
  minZoom: ${task.minZoom},
  maxZoom: ${task.maxZoom},
  attribution: '${name}',
}).addTo(map);`;
}

function maplibreWmtsCode(task: Task): string {
  const base = `${selectedBase.value}/wmts/${task.id}`;
  return `// MapLibre GL JS — WMTS raster source
// 使用 KVP 接口拼接瓦片 URL
const wmtsTile = '${base}?SERVICE=WMTS&VERSION=1.0.0&REQUEST=GetTile' +
  '&LAYER=${task.id}&STYLE=default&FORMAT=image/png' +
  '&TILEMATRIXSET=WebMercatorQuad&TILEMATRIX={z}&TILEROW={y}&TILECOL={x}';
const map = new maplibregl.Map({
  container: 'map',
  style: {
    version: 8,
    sources: {
      wmts: {
        type: 'raster',
        tiles: [wmtsTile],
        tileSize: 256,
        minzoom: ${task.minZoom},
        maxzoom: ${task.maxZoom},
      },
    },
    layers: [{ id: 'wmts', type: 'raster', source: 'wmts' }],
  },
});`;
}

// ── WMS 代码 ──────────────────────────────────────────────────────────────────

function cesiumWmsCode(task: Task): string {
  const caps = wmsUrl(task.id);
  const name = task.name.replace(/'/g, "\\'");
  return `// Cesium.js — WebMapServiceImageryProvider
const viewer = new Cesium.Viewer('cesiumContainer');
viewer.imageryLayers.addImageryProvider(
  new Cesium.WebMapServiceImageryProvider({
    url: '${selectedBase.value}/wms/${task.id}',
    layers: '${task.id}',
    parameters: {
      SERVICE: 'WMS',
      VERSION: '1.1.1',
      FORMAT: 'image/png',
      TRANSPARENT: 'true',
    },
    // GetCapabilities: '${caps}'
    credit: '${name}',
  })
);`;
}

function leafletWmsCode(task: Task): string {
  const base = `${selectedBase.value}/wms/${task.id}`;
  const name = task.name.replace(/'/g, "\\'");
  return `// Leaflet.js — L.tileLayer.wms
const map = L.map('map');
L.tileLayer.wms('${base}', {
  layers: '${task.id}',
  format: 'image/png',
  transparent: true,
  version: '1.1.1',
  attribution: '${name}',
}).addTo(map);`;
}

function maplibreWmsCode(task: Task): string {
  const base = `${selectedBase.value}/wms/${task.id}`;
  const west  = task.boundsWest.toFixed(4);
  const south = task.boundsSouth.toFixed(4);
  const east  = task.boundsEast.toFixed(4);
  const north = task.boundsNorth.toFixed(4);
  return `// MapLibre GL JS — raster source via WMS
// WMS tile URL template (uses {bbox-epsg-4326})
const wmsUrl = '${base}?SERVICE=WMS&VERSION=1.1.1&REQUEST=GetMap' +
  '&LAYERS=${task.id}&STYLES=&SRS=EPSG:4326&FORMAT=image/png' +
  '&TRANSPARENT=true&WIDTH=256&HEIGHT=256&BBOX={bbox-epsg-4326}';
const map = new maplibregl.Map({
  container: 'map',
  style: {
    version: 8,
    sources: {
      wms: {
        type: 'raster',
        tiles: [wmsUrl],
        tileSize: 256,
        bounds: [${west}, ${south}, ${east}, ${north}],
      },
    },
    layers: [{ id: 'wms', type: 'raster', source: 'wms' }],
  },
});`;
}

// ── OGC API Tiles 代码 ────────────────────────────────────────────────────────

function cesiumOgcCode(task: Task): string {
  const tileUrl = `${selectedBase.value}/ogc-tiles/${task.id}/tiles/WebMercatorQuad/{z}/{y}/{x}`;
  const metaUrl = ogcTilesUrl(task.id);
  const name = task.name.replace(/'/g, "\\'");
  return `// Cesium.js — OGC API Tiles (UrlTemplateImageryProvider)
// Tileset 元数据: '${metaUrl}'
const viewer = new Cesium.Viewer('cesiumContainer');
viewer.imageryLayers.addImageryProvider(
  new Cesium.UrlTemplateImageryProvider({
    url: '${tileUrl}',
    minimumLevel: ${task.minZoom},
    maximumLevel: ${task.maxZoom},
    credit: '${name}',
  })
);`;
}

function leafletOgcCode(task: Task): string {
  const tileUrl = `${selectedBase.value}/ogc-tiles/${task.id}/tiles/WebMercatorQuad/{z}/{y}/{x}`;
  const name = task.name.replace(/'/g, "\\'");
  return `// Leaflet.js — OGC API Tiles
// Tileset 元数据: '${ogcTilesUrl(task.id)}'
const map = L.map('map');
L.tileLayer('${tileUrl}', {
  minZoom: ${task.minZoom},
  maxZoom: ${task.maxZoom},
  attribution: '${name}',
}).addTo(map);`;
}

function maplibreOgcCode(task: Task): string {
  const metaUrl = ogcTilesUrl(task.id);
  return `// MapLibre GL JS — OGC API Tiles
const map = new maplibregl.Map({
  container: 'map',
  style: {
    version: 8,
    sources: {
      ogc: {
        type: 'raster',
        url: '${metaUrl}',   // OGC API Tiles tileset 元数据 URL
        tileSize: 256,
        minzoom: ${task.minZoom},
        maxzoom: ${task.maxZoom},
      },
    },
    layers: [{ id: 'ogc', type: 'raster', source: 'ogc' }],
  },
});`;
}

// ── ArcGIS REST 代码 ──────────────────────────────────────────────────────────

function cesiumArcGisCode(task: Task): string {
  const base = arcgisUrl(task.id);
  return `// Cesium.js — ArcGisMapServerImageryProvider
const viewer = new Cesium.Viewer('cesiumContainer');
viewer.imageryLayers.addImageryProvider(
  new Cesium.ArcGisMapServerImageryProvider({
    url: '${base}',
  })
);`;
}

function leafletArcGisCode(task: Task): string {
  const base = arcgisUrl(task.id);
  const name = task.name.replace(/'/g, "\\'");
  return `// Leaflet.js — Esri Leaflet
// 需要: https://unpkg.com/esri-leaflet/dist/esri-leaflet.js
const map = L.map('map');
L.esri.tiledMapLayer({
  url: '${base}',
  attribution: '${name}',
}).addTo(map);`;
}

function maplibreArcGisCode(task: Task): string {
  const tileUrl = `${selectedBase.value}/arcgis/rest/services/${task.id}/MapServer/tile/{z}/{y}/{x}`;
  return `// MapLibre GL JS — ArcGIS REST (raster tiles)
const map = new maplibregl.Map({
  container: 'map',
  style: {
    version: 8,
    sources: {
      arcgis: {
        type: 'raster',
        tiles: ['${tileUrl}'],
        tileSize: 256,
        minzoom: ${task.minZoom},
        maxzoom: ${task.maxZoom},
      },
    },
    layers: [{ id: 'arcgis', type: 'raster', source: 'arcgis' }],
  },
});`;
}

function codeFor(task: Task): string {
  const fw = getActiveFramework(task.id);
  const proto = getActiveProtocol(task.id);
  if (proto === "wmts") {
    if (fw === "Cesium")   return cesiumWmtsCode(task);
    if (fw === "Leaflet")  return leafletWmtsCode(task);
    return maplibreWmtsCode(task);
  }
  if (proto === "wms") {
    if (fw === "Cesium")   return cesiumWmsCode(task);
    if (fw === "Leaflet")  return leafletWmsCode(task);
    return maplibreWmsCode(task);
  }
  if (proto === "ogc") {
    if (fw === "Cesium")   return cesiumOgcCode(task);
    if (fw === "Leaflet")  return leafletOgcCode(task);
    return maplibreOgcCode(task);
  }
  if (proto === "arcgis") {
    if (fw === "Cesium")   return cesiumArcGisCode(task);
    if (fw === "Leaflet")  return leafletArcGisCode(task);
    return maplibreArcGisCode(task);
  }
  if (fw === "Cesium") return cesiumCode(task);
  if (fw === "Leaflet") return leafletCode(task);
  return maplibreCode(task);
}

async function copyCode(code: string) {
  try {
    await navigator.clipboard.writeText(code);
  } catch {
    const el = document.createElement("textarea");
    el.value = code;
    document.body.appendChild(el);
    el.select();
    document.execCommand("copy");
    document.body.removeChild(el);
  }
  copiedCode.value = code;
  setTimeout(() => {
    copiedCode.value = null;
  }, 2000);
}

function highlightCode(code: string): string {
  const KEYWORDS = new Set([
    "const",
    "let",
    "var",
    "new",
    "function",
    "return",
    "if",
    "else",
    "true",
    "false",
    "null",
    "undefined",
  ]);
  const tokens: Array<{ type: string; value: string }> = [];
  let i = 0;
  while (i < code.length) {
    if (code[i] === "/" && code[i + 1] === "/") {
      const end = code.indexOf("\n", i);
      const val = end === -1 ? code.slice(i) : code.slice(i, end);
      tokens.push({ type: "comment", value: val });
      i += val.length;
    } else if (code[i] === '"' || code[i] === "'") {
      const quote = code[i];
      let j = i + 1;
      while (j < code.length && code[j] !== quote) {
        if (code[j] === "\\") j++;
        j++;
      }
      tokens.push({ type: "string", value: code.slice(i, j + 1) });
      i = j + 1;
    } else if (/[a-zA-Z_$]/.test(code[i])) {
      let j = i;
      while (j < code.length && /[a-zA-Z0-9_$]/.test(code[j])) j++;
      const word = code.slice(i, j);
      tokens.push({
        type: KEYWORDS.has(word) ? "keyword" : "ident",
        value: word,
      });
      i = j;
    } else if (/[0-9]/.test(code[i])) {
      let j = i;
      while (j < code.length && /[0-9]/.test(code[j])) j++;
      tokens.push({ type: "number", value: code.slice(i, j) });
      i = j;
    } else {
      tokens.push({ type: "other", value: code[i] });
      i++;
    }
  }
  return tokens
    .map(({ type, value }) => {
      const esc = value
        .replace(/&/g, "&amp;")
        .replace(/</g, "&lt;")
        .replace(/>/g, "&gt;");
      switch (type) {
        case "comment":
          return `<span style="color:#565f89">${esc}</span>`;
        case "string":
          return `<span style="color:#9ece6a">${esc}</span>`;
        case "keyword":
          return `<span style="color:#7aa2f7">${esc}</span>`;
        case "number":
          return `<span style="color:#ff9e64">${esc}</span>`;
        case "ident":
          return `<span style="color:#a9b1d6">${esc}</span>`;
        default:
          return `<span style="color:#c0caf5">${esc}</span>`;
      }
    })
    .join("");
}
</script>

<template>
  <div class="flex flex-col h-full overflow-y-auto">
    <div class="flex flex-col gap-4 w-full max-w-2xl mx-auto px-6 py-6 text-sm">
      <!-- 服务器控制卡片 -->
      <div
        class="rounded-xl border bg-white overflow-hidden"
        style="border-color: var(--color-border-subtle)"
      >
        <!-- 头部 -->
        <div
          class="flex items-center gap-2 px-4 py-3 border-b"
          style="border-color: var(--color-border-subtle)"
        >
          <Server :size="14" class="text-slate-400 shrink-0" />
          <span
            class="font-semibold text-slate-600 text-xs tracking-wide uppercase"
            >{{ t('publish.title') }}</span
          >
          <!-- 运行状态指示灯 -->
          <div class="ml-auto flex items-center gap-1.5">
            <span
              class="w-2 h-2 rounded-full"
              :class="serverStatus.running ? 'bg-emerald-500' : 'bg-slate-300'"
            />
            <span
              class="text-xs"
              :class="
                serverStatus.running ? 'text-emerald-600' : 'text-slate-400'
              "
            >
              {{
                serverStatus.running ? t('publish.running', { port: serverStatus.port }) : t('publish.stopped')
              }}
            </span>
          </div>
        </div>

        <!-- 端口 + 按钮 -->
        <div class="flex items-center gap-2 px-4 py-3">
          <span class="text-slate-500 text-xs shrink-0">{{ t('publish.port') }}</span>
          <input
            v-model.number="port"
            type="number"
            min="1024"
            max="65535"
            :disabled="serverStatus.running"
            class="w-20 px-2 py-1 rounded-md bg-slate-100 border border-slate-200 text-slate-700 text-xs focus:outline-none focus:ring-1 focus:ring-blue-500/60 disabled:opacity-40 disabled:cursor-not-allowed"
          />
          <button
            @click="toggleServer"
            :disabled="toggling"
            class="ml-auto flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
            :class="
              serverStatus.running
                ? 'bg-red-50 text-red-600 hover:bg-red-100 border border-red-200'
                : 'bg-emerald-50 text-emerald-700 hover:bg-emerald-100 border border-emerald-200'
            "
          >
            <Power :size="12" />
            {{ serverStatus.running ? t('publish.stopServer') : t('publish.startServer') }}
          </button>
        </div>

        <!-- 错误提示 -->
        <div v-if="errorMsg" class="px-4 pb-3 text-xs text-red-500">
          {{ errorMsg }}
        </div>

        <!-- 访问地址选择器（含局域网 IP） -->
        <div
          v-if="serverStatus.running"
          class="px-4 pb-3 flex items-center gap-2"
        >
          <span class="text-xs text-slate-400 shrink-0">{{ t('publish.accessAddr') }}</span>
          <select
            v-model="selectedBase"
            class="flex-1 text-xs text-slate-600 bg-slate-100 border border-slate-200 rounded-md px-2 py-1 focus:outline-none focus:ring-1 focus:ring-blue-500/60 truncate"
          >
            <option :value="serverStatus.baseUrl">{{ serverStatus.baseUrl }}</option>
            <option
              v-for="url in serverStatus.lanUrls"
              :key="url"
              :value="url"
            >{{ url }}</option>
          </select>
          <button
            @click="copyUrl(selectedBase)"
            class="shrink-0 p-1.5 rounded-md hover:bg-slate-100 text-slate-400 hover:text-slate-600 transition-colors"
          >
            <Check v-if="copied === selectedBase" :size="12" class="text-emerald-500" />
            <Copy v-else :size="12" />
          </button>
        </div>
      </div>

      <!-- 任务列表 -->
      <div
        v-if="readyTasks.length === 0"
        class="flex flex-col items-center justify-center py-10 gap-2 text-slate-400"
      >
        <Layers :size="32" class="opacity-30" />
        <p class="text-xs">{{ t('publish.noTasks') }}</p>
      </div>

      <template v-else>
        <div class="text-xs text-slate-400 px-0.5">
          {{ t('publish.readyTasks', { count: readyTasks.length }) }}
        </div>

        <div
          v-for="task in readyTasks"
          :key="task.id"
          class="rounded-xl border bg-white overflow-hidden"
          style="border-color: var(--color-border-subtle)"
        >
          <!-- 任务名称 -->
          <div
            class="flex items-center gap-2 px-4 py-2.5 border-b"
            style="border-color: var(--color-border-subtle)"
          >
            <Globe :size="12" class="text-blue-500 shrink-0" />
            <span class="text-slate-800 text-xs font-medium truncate">{{
              task.name
            }}</span>
            <span class="ml-auto text-slate-400 text-xs shrink-0"
              >Z{{ task.minZoom }}-{{ task.maxZoom }}</span
            >
          </div>

          <!-- 缩略图 + URL 行 -->
          <div class="flex">
            <!-- 左侧缩略图 -->
            <div
              v-if="thumbnails[task.id]"
              class="relative shrink-0"
              style="width: 88px"
            >
              <img
                :src="thumbnails[task.id]"
                class="w-full h-full object-cover"
                alt=""
              />
              <!-- 右侧渐变过渡 -->
              <div
                class="absolute inset-y-0 right-0 w-5"
                style="background: linear-gradient(to right, transparent, white)"
              />
            </div>

            <!-- URL 行 -->
            <div class="flex-1 flex flex-col px-3 py-3 gap-2 min-w-0">
            <!-- XYZ -->
            <div class="flex items-center gap-1.5">
              <span class="text-xs text-slate-500 w-9 shrink-0 font-medium"
                >XYZ</span
              >
              <code
                class="flex-1 text-xs text-slate-600 bg-slate-100 rounded px-2 py-1 truncate min-w-0"
              >
                {{ tmsUrl(task.id) }}
              </code>
              <button
                @click="copyUrl(tmsUrl(task.id))"
                :disabled="!serverStatus.running"
                class="shrink-0 p-1.5 rounded-md hover:bg-slate-100 text-slate-400 hover:text-slate-600 transition-colors disabled:opacity-30 disabled:cursor-not-allowed"
                :title="t('publish.copyXyz')"
              >
                <Check
                  v-if="copied === tmsUrl(task.id)"
                  :size="12"
                  class="text-emerald-500"
                />
                <Copy v-else :size="12" />
              </button>
            </div>
            <!-- WMTS -->
            <div class="flex items-center gap-1.5">
              <span class="text-xs text-slate-500 w-9 shrink-0 font-medium"
                >WMTS</span
              >
              <code
                class="flex-1 text-xs text-slate-600 bg-slate-100 rounded px-2 py-1 truncate min-w-0"
              >
                {{ wmtsUrl(task.id) }}
              </code>
              <button
                @click="copyUrl(wmtsUrl(task.id))"
                :disabled="!serverStatus.running"
                class="shrink-0 p-1.5 rounded-md hover:bg-slate-100 text-slate-400 hover:text-slate-600 transition-colors disabled:opacity-30 disabled:cursor-not-allowed"
                :title="t('publish.copyWmts')"
              >
                <Check
                  v-if="copied === wmtsUrl(task.id)"
                  :size="12"
                  class="text-emerald-500"
                />
                <Copy v-else :size="12" />
              </button>
            </div>
            <!-- WMS -->
            <div class="flex items-center gap-1.5">
              <span class="text-xs text-slate-500 w-9 shrink-0 font-medium"
                >WMS</span
              >
              <code
                class="flex-1 text-xs text-slate-600 bg-slate-100 rounded px-2 py-1 truncate min-w-0"
              >
                {{ wmsUrl(task.id) }}
              </code>
              <button
                @click="copyUrl(wmsUrl(task.id))"
                :disabled="!serverStatus.running"
                class="shrink-0 p-1.5 rounded-md hover:bg-slate-100 text-slate-400 hover:text-slate-600 transition-colors disabled:opacity-30 disabled:cursor-not-allowed"
                :title="t('publish.copyWms')"
              >
                <Check
                  v-if="copied === wmsUrl(task.id)"
                  :size="12"
                  class="text-emerald-500"
                />
                <Copy v-else :size="12" />
              </button>
            </div>
            <!-- OGC API Tiles -->
            <div class="flex items-center gap-1.5">
              <span class="text-xs text-slate-500 w-9 shrink-0 font-medium leading-tight"
                >OGC</span
              >
              <code
                class="flex-1 text-xs text-slate-600 bg-slate-100 rounded px-2 py-1 truncate min-w-0"
              >
                {{ ogcTilesUrl(task.id) }}
              </code>
              <button
                @click="copyUrl(ogcTilesUrl(task.id))"
                :disabled="!serverStatus.running"
                class="shrink-0 p-1.5 rounded-md hover:bg-slate-100 text-slate-400 hover:text-slate-600 transition-colors disabled:opacity-30 disabled:cursor-not-allowed"
                :title="t('publish.copyOgc')"
              >
                <Check
                  v-if="copied === ogcTilesUrl(task.id)"
                  :size="12"
                  class="text-emerald-500"
                />
                <Copy v-else :size="12" />
              </button>
            </div>
            <!-- ArcGIS REST -->
            <div class="flex items-center gap-1.5">
              <span class="text-xs text-slate-500 w-9 shrink-0 font-medium leading-tight"
                >ESRI</span
              >
              <code
                class="flex-1 text-xs text-slate-600 bg-slate-100 rounded px-2 py-1 truncate min-w-0"
              >
                {{ arcgisUrl(task.id) }}
              </code>
              <button
                @click="copyUrl(arcgisUrl(task.id))"
                :disabled="!serverStatus.running"
                class="shrink-0 p-1.5 rounded-md hover:bg-slate-100 text-slate-400 hover:text-slate-600 transition-colors disabled:opacity-30 disabled:cursor-not-allowed"
                :title="t('publish.copyArcgis')"
              >
                <Check
                  v-if="copied === arcgisUrl(task.id)"
                  :size="12"
                  class="text-emerald-500"
                />
                <Copy v-else :size="12" />
              </button>
            </div>
            </div>
          </div>

          <!-- 代码示例（始终显示）-->
          <div
            class="border-t"
            style="border-color: var(--color-border-subtle)"
          >
            <!-- 可点击的折叠头部 -->
            <button
              type="button"
              class="w-full flex items-center gap-2 px-4 py-2.5 hover:bg-slate-50 transition-colors text-left"
              @click="toggleCodeExpanded(task.id)"
            >
              <span class="text-xs text-slate-400 font-medium">{{ t('publish.codeExample') }}</span>
              <ChevronDown
                :size="12"
                class="text-slate-300 transition-transform duration-200 ml-auto"
                :style="
                  isCodeExpanded(task.id) ? 'transform:rotate(180deg)' : ''
                "
              />
            </button>

            <!-- 展开区域 -->
            <template v-if="isCodeExpanded(task.id)">
              <!-- 协议 + 框架选项卡 -->
              <div
                class="flex items-center gap-2 px-4 pb-2 border-t"
                style="border-color: var(--color-border-subtle)"
              >
                <div class="flex items-center gap-1.5 mt-2 flex-wrap">
                    <!-- 协议选择 -->
                  <div class="flex items-center bg-blue-50 rounded-lg p-0.5 gap-0.5">
                    <button
                      type="button"
                      v-for="proto in ['tms', 'wmts', 'wms', 'ogc', 'arcgis'] as const"
                      :key="proto"
                      @click.stop="setActiveProtocol(task.id, proto)"
                      class="px-2 py-0.5 rounded-md text-xs font-medium transition-all"
                      :class="
                        getActiveProtocol(task.id) === proto
                          ? 'bg-white text-blue-700 shadow-sm'
                          : 'text-blue-400 hover:text-blue-600'
                      "
                    >
                      {{ proto === 'arcgis' ? 'ESRI' : proto.toUpperCase() }}
                    </button>
                  </div>
                  <!-- 框架选择 -->
                  <div class="flex items-center bg-slate-100 rounded-lg p-0.5 gap-0.5">
                    <button
                      type="button"
                      v-for="fw in FRAMEWORKS"
                      :key="fw"
                      @click.stop="setActiveFramework(task.id, fw)"
                      class="px-2.5 py-0.5 rounded-md text-xs font-medium transition-all"
                      :class="
                        getActiveFramework(task.id) === fw
                          ? 'bg-white text-slate-700 shadow-sm'
                          : 'text-slate-400 hover:text-slate-600'
                      "
                    >
                      {{ fw }}
                    </button>
                  </div>
                </div>
              </div>
              <!-- 代码块 -->
              <div class="relative group px-4 pb-3">
                <pre
                  class="rounded-lg px-4 py-3 overflow-x-auto text-xs leading-relaxed"
                  style="
                    background: #1a1b26;
                    font-family: &quot;IBM Plex Mono&quot;, monospace;
                    white-space: pre;
                    color: #a9b1d6;
                  "
                ><code v-html="highlightCode(codeFor(task))"></code></pre>
                <button
                  type="button"
                  @click="copyCode(codeFor(task))"
                  class="absolute top-4 right-6 flex items-center gap-1 px-2 py-0.5 rounded text-xs opacity-0 group-hover:opacity-100 transition-opacity"
                  style="
                    background: rgba(255, 255, 255, 0.08);
                    color: #9aa5ce;
                  "
                  :title="t('publish.copyCode')"
                >
                  <Check
                    v-if="copiedCode === codeFor(task)"
                    :size="10"
                    style="color: #9ece6a"
                  />
                  <Copy v-else :size="10" />
                  <span>{{
                    copiedCode === codeFor(task) ? t('publish.copied') : t('publish.copy')
                  }}</span>
                </button>
              </div>
            </template>
          </div>

          <!-- 服务监控统计（服务运行时显示）-->
          <template v-if="serverStatus.running">
            <div
              class="border-t px-4 py-2 flex items-center gap-2"
              style="border-color: var(--color-border-subtle)"
            >
              <BarChart2 :size="11" class="text-slate-300 shrink-0" />
              <span class="text-xs text-slate-400">{{ t('publish.stats.title') }}</span>
              <span class="inline-flex items-center gap-0.5 px-1.5 py-px rounded text-[10px] font-medium bg-sky-50 text-sky-500 ml-1">
                {{ totalRequests(task.id) }}
              </span>
              <span class="ml-auto text-xs text-slate-300">
                {{ totalRequests(task.id) > 0 ? formatLastRequest(getStats(task.id).lastRequestAt) : t('publish.stats.noRequests') }}
              </span>
            </div>
          </template>
        </div>
      </template>
    </div>
  </div>
</template>
