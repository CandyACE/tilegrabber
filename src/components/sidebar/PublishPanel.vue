<script setup lang="ts">
import { ref, computed, onMounted, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import {
  Power,
  Copy,
  Check,
  Globe,
  Layers,
  Server,
  ChevronDown,
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
}

// ─── 状态 ────────────────────────────────────────────────────────────────────
const tasks = ref<Task[]>([]);
const serverStatus = ref<ServerStatus>({
  running: false,
  port: 8765,
  baseUrl: "http://localhost:8765",
});
const port = ref(8765);
const toggling = ref(false);
const errorMsg = ref("");

// 复制成功反馈：存储已复制的 url
const copied = ref<string | null>(null);

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
  return `${serverStatus.value.baseUrl}/tiles/${taskId}/{z}/{x}/{y}`;
}

function wmtsUrl(taskId: string) {
  return `${serverStatus.value.baseUrl}/wmts/${taskId}?SERVICE=WMTS&REQUEST=GetCapabilities`;
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

const activeFramework = ref<Record<string, Framework>>({});
const copiedCode = ref<string | null>(null);
const expandedCode = ref<Record<string, boolean>>({});

function getActiveFramework(taskId: string): Framework {
  return activeFramework.value[taskId] ?? "Cesium";
}

function setActiveFramework(taskId: string, fw: Framework) {
  activeFramework.value = { ...activeFramework.value, [taskId]: fw };
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

function codeFor(task: Task): string {
  const fw = getActiveFramework(task.id);
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
            >本地发布服务</span
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
                serverStatus.running ? `运行中 :${serverStatus.port}` : "已停止"
              }}
            </span>
          </div>
        </div>

        <!-- 端口 + 按钮 -->
        <div class="flex items-center gap-2 px-4 py-3">
          <span class="text-slate-500 text-xs shrink-0">端口</span>
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
            {{ serverStatus.running ? "停止服务" : "启动服务" }}
          </button>
        </div>

        <!-- 错误提示 -->
        <div v-if="errorMsg" class="px-4 pb-3 text-xs text-red-500">
          {{ errorMsg }}
        </div>
      </div>

      <!-- 任务列表 -->
      <div
        v-if="readyTasks.length === 0"
        class="flex flex-col items-center justify-center py-10 gap-2 text-slate-400"
      >
        <Layers :size="32" class="opacity-30" />
        <p class="text-xs">无可发布的任务（需下载完成后才可发布）</p>
      </div>

      <template v-else>
        <div class="text-xs text-slate-400 px-0.5">
          可发布的任务（{{ readyTasks.length }}）
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
                title="复制 XYZ URL"
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
                title="复制 WMTS 能力文档 URL"
              >
                <Check
                  v-if="copied === wmtsUrl(task.id)"
                  :size="12"
                  class="text-emerald-500"
                />
                <Copy v-else :size="12" />
              </button>
            </div>
            </div>
          </div>

          <!-- 代码示例（服务运行时显示）-->
          <template v-if="serverStatus.running">
            <div
              class="border-t"
              style="border-color: var(--color-border-subtle)"
            >
              <!-- 可点击的折叠头部 -->
              <button
                class="w-full flex items-center gap-2 px-4 py-2.5 hover:bg-slate-50 transition-colors text-left"
                @click="toggleCodeExpanded(task.id)"
              >
                <span class="text-xs text-slate-400 font-medium">示例代码</span>
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
                <!-- 框架选项卡 -->
                <div
                  class="flex items-center gap-2 px-4 pb-2 border-t"
                  style="border-color: var(--color-border-subtle)"
                >
                  <div
                    class="flex items-center bg-slate-100 rounded-lg p-0.5 gap-0.5 mt-2"
                  >
                    <button
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
                    @click="copyCode(codeFor(task))"
                    class="absolute top-4 right-6 flex items-center gap-1 px-2 py-0.5 rounded text-xs opacity-0 group-hover:opacity-100 transition-opacity"
                    style="
                      background: rgba(255, 255, 255, 0.08);
                      color: #9aa5ce;
                    "
                    title="复制代码"
                  >
                    <Check
                      v-if="copiedCode === codeFor(task)"
                      :size="10"
                      style="color: #9ece6a"
                    />
                    <Copy v-else :size="10" />
                    <span>{{
                      copiedCode === codeFor(task) ? "已复制" : "复制"
                    }}</span>
                  </button>
                </div>
              </template>
            </div>
          </template>
        </div>
      </template>
    </div>
  </div>
</template>
