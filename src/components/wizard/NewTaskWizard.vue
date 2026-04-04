<script setup lang="ts">
import { ref, watch, onUnmounted } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import {
  Upload,
  Globe,
  Link,
  Scan,
  PlusCircle,
  X,
  Check,
  AlertCircle,
  ArrowRight,
  Loader2,
  Radio,
  Eye,
} from "lucide-vue-next";
import UiInput from "@/components/ui/input/Input.vue";
import UiButton from "@/components/ui/button/Button.vue";
import type { TileSource } from "~/types/tile-source";

const emit = defineEmits<{
  confirm: [source: TileSource];
  close: [];
}>();

type SourceType = "file" | "wmts" | "tms" | "web";
type Step = 1 | 2;
type CaptureStatus = "idle" | "capturing";

const step = ref<Step>(1);
const sourceType = ref<SourceType>("file");
const isLoading = ref(false);
const errorMsg = ref("");

const parsedSource = ref<TileSource | null>(null);
const wmtsLayers = ref<TileSource[]>([]);
const selectedLayerIdx = ref(0);
const urlInput = ref("");
const customName = ref("");

// 网页抓取状态
const captureStatus = ref<CaptureStatus>("idle");
const capturedTiles = ref<TileSource[]>([]);
let pollTimer: ReturnType<typeof setInterval> | null = null;

// 瓦片预览状态（step 2）
const previewBlobUrls = ref<(string | null)[]>(new Array(9).fill(null));
const previewLoading = ref(false);

// ─── 请求配置（Headers + Param Scripts）─────────────────────────────────────

const advancedOpen = ref(false);
const headerRows = ref<{ key: string; value: string }[]>([]);
const scriptRows = ref<{ name: string; script: string; error: string }[]>([]);

function addHeaderRow() {
  headerRows.value.push({ key: "", value: "" });
}
function removeHeaderRow(i: number) {
  headerRows.value.splice(i, 1);
}
function addScriptRow() {
  scriptRows.value.push({ name: "", script: "", error: "" });
}
function removeScriptRow(i: number) {
  scriptRows.value.splice(i, 1);
}

/** 对所有 param_scripts 求值 → extra_params，并更新行的 error 状态 */
function evalParamScripts(): Record<string, string> {
  const result: Record<string, string> = {};
  for (const row of scriptRows.value) {
    if (!row.name.trim() || !row.script.trim()) continue;
    try {
      // eslint-disable-next-line no-new-func
      const val = new Function('"use strict"; return (' + row.script + ")")();
      row.error = "";
      result[row.name.trim()] = String(val ?? "");
    } catch (e: unknown) {
      row.error = e instanceof Error ? e.message : String(e);
    }
  }
  return result;
}

/** 把当前 headerRows 和 scriptRows 合并到 source 后返回新对象 */
function applyRequestConfig(source: TileSource): TileSource {
  const headers: Record<string, string> = { ...(source.headers ?? {}) };
  for (const r of headerRows.value) {
    if (r.key.trim()) headers[r.key.trim()] = r.value;
  }
  const extra_params = evalParamScripts();
  const param_scripts: Record<string, string> = {};
  for (const r of scriptRows.value) {
    if (r.name.trim()) param_scripts[r.name.trim()] = r.script;
  }
  return { ...source, headers, extra_params, param_scripts };
}

const sourceTypeOptions = [
  {
    value: "file",
    icon: Upload,
    label: "本地文件",
    desc: ".lrc / .lra / .ovmap 图层资源文件",
  },
  {
    value: "wmts",
    icon: Globe,
    label: "WMTS 服务",
    desc: "输入 GetCapabilities URL",
  },
  { value: "tms", icon: Link, label: "TMS/XYZ URL", desc: "输入瓦片 URL 模板" },
  { value: "web", icon: Scan, label: "网页抓取", desc: "拦截浏览器实际请求" },
] as const;

function stopPolling() {
  if (pollTimer !== null) {
    clearInterval(pollTimer);
    pollTimer = null;
  }
}

function revokePreviews() {
  for (const blobUrl of previewBlobUrls.value) {
    if (blobUrl) URL.revokeObjectURL(blobUrl);
  }
  previewBlobUrls.value = new Array(9).fill(null);
}

onUnmounted(async () => {
  stopPolling();
  revokePreviews();
  await invoke("close_capture_window").catch(() => {});
});

// ─── 瓦片预览辅助 ─────────────────────────────────────────────────────────────

/** Web Mercator 经纬度 → 瓦片 X/Y */
function latLonToTileXY(lat: number, lon: number, z: number): [number, number] {
  const n = Math.pow(2, z);
  const x = Math.floor(((lon + 180) / 360) * n);
  const latRad = (lat * Math.PI) / 180;
  const y = Math.floor(
    ((1 - Math.log(Math.tan(latRad) + 1 / Math.cos(latRad)) / Math.PI) / 2) * n,
  );
  return [Math.max(0, Math.min(n - 1, x)), Math.max(0, Math.min(n - 1, y))];
}

/** 将 {z}/{x}/{y}/{s} 变量填入模板 */
function buildTileUrl(
  src: TileSource,
  z: number,
  x: number,
  y: number,
): string {
  let url = src.url_template;
  const sub = src.subdomains.length > 0 ? src.subdomains[0] : "";
  url = url.replace(/\{s\}/g, sub);
  // north_to_south=false → TMS y（y=0 在南），需把 XYZ y 取反
  const finalY = src.north_to_south ? y : Math.pow(2, z) - 1 - y;
  url = url
    .replace(/\{z\}/g, String(z))
    .replace(/\{x\}/g, String(x))
    .replace(/\{y\}/g, String(Math.max(0, finalY)));
  return url;
}

/** 根据数据源边界，加载 3×3 预览瓦片 */
async function loadTilePreview(src: TileSource) {
  revokePreviews();
  previewLoading.value = true;

  const b = src.bounds;
  const lat = (b.north + b.south) / 2;
  const lon = (b.east + b.west) / 2;
  const lonSpan = b.east - b.west;
  const z = lonSpan > 80 ? 4 : lonSpan > 20 ? 6 : 8;

  const n = Math.pow(2, z);
  const [cx, cy] = latLonToTileXY(lat, lon, z);

  // 3×3 邻域瓦片（行主序）
  const coords: [number, number, number][] = [];
  for (let dy = -1; dy <= 1; dy++) {
    for (let dx = -1; dx <= 1; dx++) {
      const x = Math.max(0, Math.min(n - 1, cx + dx));
      const y = Math.max(0, Math.min(n - 1, cy + dy));
      coords.push([z, x, y]);
    }
  }

  const results = await Promise.all(
    coords.map(async ([z, x, y]) => {
      const url = buildTileUrl(src, z, x, y);
      try {
        const bytes = await invoke<number[]>("fetch_tile", {
          url,
          headers: src.headers ?? {},
        });
        const blob = new Blob([new Uint8Array(bytes)], { type: "image/png" });
        return URL.createObjectURL(blob);
      } catch {
        return null;
      }
    }),
  );

  previewBlobUrls.value = results;
  previewLoading.value = false;
}

// 选中图层变化时刷新预览
watch(parsedSource, (src) => {
  if (step.value === 2 && src) loadTilePreview(src);
});

// ─── 命令处理 ─────────────────────────────────────────────────────────────────

async function handleNext() {
  errorMsg.value = "";
  if (step.value === 1) {
    isLoading.value = true;
    try {
      if (sourceType.value === "file") await pickAndParseFile();
      else if (sourceType.value === "wmts") await parseWmts();
      else if (sourceType.value === "web") {
        if (captureStatus.value === "idle") await startCapture();
        else await finishCapture();
      } else await parseTms();
    } finally {
      isLoading.value = false;
    }
  } else if (step.value === 2) {
    if (parsedSource.value)
      emit("confirm", applyRequestConfig(parsedSource.value));
  }
}

async function pickAndParseFile() {
  const selected = await open({
    multiple: false,
    title: "选择图层资源文件",
    filters: [{ name: "图层资源", extensions: ["lrc", "lra", "ovmap"] }],
  });
  if (!selected) return;
  const filePath = typeof selected === "string" ? selected : selected[0];
  const result: TileSource = await invoke("parse_source_file", {
    path: filePath,
  });
  // 用文件名（去扩展名）作为下载任务名称的默认值
  const fileName =
    filePath
      .replace(/\\/g, "/")
      .split("/")
      .pop()
      ?.replace(/\.[^.]+$/, "") ?? result.name;
  emit("confirm", { ...result, name: fileName });
}

async function parseWmts() {
  if (!urlInput.value.trim()) {
    errorMsg.value = "请输入 WMTS 服务 URL";
    return;
  }
  const results: TileSource[] = await invoke("parse_wmts_url", {
    url: urlInput.value.trim(),
  });
  if (!results.length) {
    errorMsg.value = "WMTS 服务未返回任何图层";
    return;
  }
  if (results.length === 1) {
    emit("confirm", applyRequestConfig(results[0]));
  } else {
    wmtsLayers.value = results;
    parsedSource.value = results[0];
    selectedLayerIdx.value = 0;
    step.value = 2;
    loadTilePreview(results[0]);
  }
}

async function parseTms() {
  if (!urlInput.value.trim()) {
    errorMsg.value = "请输入瓦片 URL 模板";
    return;
  }
  const result: TileSource = await invoke("parse_tms_url", {
    url: urlInput.value.trim(),
    name: customName.value || null,
  });
  emit("confirm", applyRequestConfig(result));
}

/** 开始抓取：打开 WebView 窗口，开始轮询捕获结果 */
async function startCapture() {
  if (!urlInput.value.trim()) {
    errorMsg.value = "请输入要抓取的网页 URL";
    return;
  }
  await invoke("clear_captured_tiles");
  capturedTiles.value = [];

  await invoke("open_capture_window", { url: urlInput.value.trim() });

  captureStatus.value = "capturing";

  // 每 800ms 轮询一次捕获结果
  pollTimer = setInterval(async () => {
    const tiles: TileSource[] = await invoke("get_captured_tiles");
    capturedTiles.value = tiles;
  }, 800);
}

/** 完成抓取：停止轮询，关闭窗口，进入选择流程 */
async function finishCapture() {
  stopPolling();
  await invoke("close_capture_window");
  captureStatus.value = "idle";

  const tiles: TileSource[] = await invoke("get_captured_tiles");
  if (!tiles.length) {
    errorMsg.value = "未捕获到瓦片 URL，请打开抓取窗口后浏览地图再试";
    return;
  }
  if (tiles.length === 1) {
    emit("confirm", applyRequestConfig(tiles[0]));
  } else {
    wmtsLayers.value = tiles;
    parsedSource.value = tiles[0];
    selectedLayerIdx.value = 0;
    step.value = 2;
    loadTilePreview(tiles[0]);
  }
}

function onLayerSelect(idx: number) {
  selectedLayerIdx.value = idx;
  parsedSource.value = wmtsLayers.value[idx] ?? null;
}
</script>

<template>
  <Transition name="modal-backdrop">
    <div
      class="fixed inset-0 top-11 z-50 flex items-center justify-center p-4"
      style="background: rgba(15, 23, 42, 0.45); backdrop-filter: blur(2px)"
      @click.self="emit('close')"
    >
      <Transition name="modal-panel" appear>
        <div
          class="bg-white rounded-2xl shadow-xl flex flex-col w-full border transition-[max-width] duration-300"
          :class="step === 2 ? 'max-w-190' : 'max-w-135'"
          style="max-height: 86vh; border-color: var(--color-border-subtle)"
        >
          <!-- 顶部色带 + 头部 -->
          <div
            class="relative px-6 pt-6 pb-4 border-b"
            style="border-color: var(--color-border-subtle)"
          >
            <div
              class="absolute inset-x-0 top-0 h-0.5 bg-linear-to-r from-blue-400 via-blue-500 to-indigo-500 rounded-t-2xl"
            />
            <div class="flex items-center justify-between">
              <div class="flex items-center gap-3">
                <div
                  class="w-9 h-9 rounded-xl bg-blue-50 border border-blue-100 flex items-center justify-center shrink-0"
                >
                  <PlusCircle class="w-4.5 h-4.5 text-blue-600" />
                </div>
                <div>
                  <h2 class="text-sm font-semibold text-slate-900">
                    选择数据源
                  </h2>
                  <p class="text-xs text-slate-500 mt-0.5">
                    {{ step === 1 ? "选择来源类型并解析" : "选择要下载的图层" }}
                  </p>
                </div>
              </div>
              <button
                class="w-7 h-7 rounded-lg flex items-center justify-center text-slate-400 hover:text-slate-600 hover:bg-slate-100 transition-colors"
                @click="emit('close')"
              >
                <X class="w-4 h-4" />
              </button>
            </div>
          </div>

          <!-- 内容区 -->
          <div class="flex-1 overflow-y-auto px-6 py-5">
            <!-- 步骤 1 -->
            <template v-if="step === 1">
              <!-- 类型选择器 -->
              <div class="grid grid-cols-3 gap-2.5 mb-5">
                <button
                  v-for="opt in sourceTypeOptions"
                  :key="opt.value"
                  class="flex flex-col items-center gap-2 p-3.5 rounded-xl border-2 transition-all duration-200 text-center"
                  :class="
                    sourceType === opt.value
                      ? 'border-blue-500 bg-blue-50 shadow-sm shadow-blue-100'
                      : 'border-slate-200 hover:border-slate-300 bg-white'
                  "
                  @click="
                    sourceType = opt.value;
                    urlInput = '';
                    errorMsg = '';
                  "
                >
                  <div
                    class="w-10 h-10 rounded-lg flex items-center justify-center transition-colors duration-200"
                    :class="
                      sourceType === opt.value ? 'bg-blue-100' : 'bg-slate-100'
                    "
                  >
                    <component
                      :is="opt.icon"
                      class="w-5 h-5 transition-colors duration-200"
                      :class="
                        sourceType === opt.value
                          ? 'text-blue-600'
                          : 'text-slate-500'
                      "
                    />
                  </div>
                  <div>
                    <div
                      class="text-xs font-semibold transition-colors duration-200"
                      :class="
                        sourceType === opt.value
                          ? 'text-blue-700'
                          : 'text-slate-800'
                      "
                    >
                      {{ opt.label }}
                    </div>
                    <div
                      class="text-[11px] text-slate-500 mt-0.5 leading-tight"
                    >
                      {{ opt.desc }}
                    </div>
                  </div>
                </button>
              </div>

              <!-- 类型对应内容 — 每块单独 grid 折叠，实现高度动画 -->

              <!-- 文件 -->
              <div
                class="grid transition-[grid-template-rows] duration-300 ease-out"
                :style="{
                  gridTemplateRows: sourceType === 'file' ? '1fr' : '0fr',
                }"
              >
                <div class="overflow-hidden">
                  <div
                    class="border-2 border-dashed border-slate-200 rounded-xl p-8 flex flex-col items-center gap-3 text-center hover:border-blue-400 hover:bg-blue-50/40 transition-all duration-200 cursor-pointer"
                    @click="sourceType === 'file' && handleNext()"
                  >
                    <div
                      class="w-12 h-12 rounded-full bg-slate-100 flex items-center justify-center"
                    >
                      <Upload class="w-5 h-5 text-slate-500" />
                    </div>
                    <div>
                      <p class="text-sm font-medium text-slate-700">
                        点击选择 .lrc、.lra 或 .ovmap 文件
                      </p>
                      <p class="text-xs text-slate-500 mt-1">
                        来自 OrbitGIS、EasyEarth、LocaSpaceViewer
                        等软件导出的图层资源
                      </p>
                    </div>
                  </div>
                </div>
              </div>

              <!-- WMTS -->
              <div
                class="grid transition-[grid-template-rows] duration-300 ease-out"
                :style="{
                  gridTemplateRows: sourceType === 'wmts' ? '1fr' : '0fr',
                }"
              >
                <div class="overflow-hidden">
                  <div class="space-y-2 pb-1">
                    <label
                      class="block text-xs font-medium text-slate-600 mb-1.5"
                      >GetCapabilities URL</label
                    >
                    <UiInput
                      v-model="urlInput"
                      placeholder="https://example.com/wmts?SERVICE=WMTS&REQUEST=GetCapabilities"
                      class="w-full font-mono text-xs"
                      @keydown.enter="handleNext"
                    />
                    <p class="text-[11px] text-slate-400">
                      输入 WMTS 服务的能力文档地址，将自动获取图层列表
                    </p>
                  </div>
                </div>
              </div>

              <!-- XYZ/TMS -->
              <div
                class="grid transition-[grid-template-rows] duration-300 ease-out"
                :style="{
                  gridTemplateRows: sourceType === 'tms' ? '1fr' : '0fr',
                }"
              >
                <div class="overflow-hidden">
                  <div class="space-y-2.5 pb-1">
                    <div>
                      <label
                        class="block text-xs font-medium text-slate-600 mb-1.5"
                        >瓦片 URL 模板</label
                      >
                      <UiInput
                        v-model="urlInput"
                        placeholder="https://tile.openstreetmap.org/{z}/{x}/{y}.png"
                        class="w-full font-mono text-xs"
                        @keydown.enter="handleNext"
                      />
                    </div>
                    <div>
                      <label
                        class="block text-xs font-medium text-slate-600 mb-1.5"
                        >图层名称（可选）</label
                      >
                      <UiInput
                        v-model="customName"
                        placeholder="自定义图层名称"
                        class="w-full"
                      />
                    </div>
                  </div>
                </div>
              </div>

              <!-- 网页抓取 -->
              <div
                class="grid transition-[grid-template-rows] duration-300 ease-out"
                :style="{
                  gridTemplateRows: sourceType === 'web' ? '1fr' : '0fr',
                }"
              >
                <div class="overflow-hidden">
                  <!-- 待开始：URL 输入 -->
                  <div v-if="captureStatus === 'idle'" class="space-y-2 pb-1">
                    <label
                      class="block text-xs font-medium text-slate-600 mb-1.5"
                      >网页 URL</label
                    >
                    <UiInput
                      v-model="urlInput"
                      placeholder="https://example.com/map"
                      class="w-full font-mono text-xs"
                      @keydown.enter="handleNext"
                    />
                    <p class="text-[11px] text-slate-400">
                      点击「开始抓取」后将打开浏览器窗口，浏览地图页面以触发瓦片请求，完成后点击「完成抓取」
                    </p>
                  </div>
                  <!-- 抓取中：实时结果 -->
                  <div v-else class="space-y-2 pb-1">
                    <div
                      class="flex items-center gap-2 text-xs text-emerald-600 font-medium"
                    >
                      <Radio class="w-3.5 h-3.5 animate-pulse" />
                      <span>正在捕获瓦片请求…</span>
                      <span class="ml-auto text-slate-400"
                        >已发现 {{ capturedTiles.length }} 个</span
                      >
                    </div>
                    <div
                      v-if="capturedTiles.length === 0"
                      class="rounded-lg border border-dashed border-slate-200 p-3 text-[11px] text-slate-400 text-center"
                    >
                      请在弹出的浏览器窗口中浏览地图，等待瓦片出现在此列表
                    </div>
                    <ul
                      v-else
                      class="max-h-36 overflow-y-auto space-y-1.5 rounded-lg border border-slate-100 bg-slate-50 p-2"
                    >
                      <li
                        v-for="(tile, i) in capturedTiles"
                        :key="i"
                        class="flex flex-col gap-0.5"
                      >
                        <div class="flex items-center gap-1.5">
                          <Check class="w-3 h-3 shrink-0 text-emerald-500" />
                          <span
                            class="text-[11px] font-medium text-slate-700 truncate"
                            >{{ tile.name }}</span
                          >
                        </div>
                        <span
                          class="text-[10px] text-slate-400 font-mono truncate pl-4.5"
                          >{{ tile.url_template }}</span
                        >
                      </li>
                    </ul>
                  </div>
                </div>
              </div>

              <!-- ── 请求配置（仅 wmts / tms / web 显示）────────────────────── -->
              <div
                class="grid transition-[grid-template-rows] duration-300 ease-out mt-3"
                :style="{
                  gridTemplateRows: sourceType !== 'file' ? '1fr' : '0fr',
                }"
              >
                <div class="overflow-hidden">
                  <!-- 折叠标题 -->
                  <button
                    type="button"
                    class="flex items-center gap-1.5 text-xs font-medium text-slate-500 hover:text-slate-700 transition-colors w-full mb-2"
                    @click="advancedOpen = !advancedOpen"
                  >
                    <svg
                      class="w-3 h-3 transition-transform duration-200"
                      :class="advancedOpen ? 'rotate-90' : ''"
                      fill="none"
                      viewBox="0 0 24 24"
                      stroke="currentColor"
                      stroke-width="2.5"
                    >
                      <path
                        stroke-linecap="round"
                        stroke-linejoin="round"
                        d="M9 5l7 7-7 7"
                      />
                    </svg>
                    <span>请求配置</span>
                    <span
                      v-if="headerRows.length || scriptRows.length"
                      class="ml-1 px-1.5 py-0.5 rounded-full text-[10px] bg-blue-100 text-blue-600 font-semibold"
                    >
                      {{
                        headerRows.filter((r) => r.key).length +
                        scriptRows.filter((r) => r.name).length
                      }}
                    </span>
                  </button>

                  <!-- 折叠内容 -->
                  <div
                    class="grid transition-[grid-template-rows] duration-300 ease-out"
                    :style="{
                      gridTemplateRows: advancedOpen ? '1fr' : '0fr',
                    }"
                  >
                    <div class="overflow-hidden">
                      <div class="space-y-4 pb-2">
                        <!-- ──  请求头 ── -->
                        <div>
                          <div class="flex items-center justify-between mb-1.5">
                            <span class="text-xs font-medium text-slate-600"
                              >请求头（HTTP Headers）</span
                            >
                            <button
                              type="button"
                              class="text-[11px] text-blue-600 hover:text-blue-700 font-medium flex items-center gap-0.5"
                              @click="addHeaderRow"
                            >
                              <span class="text-base leading-none">+</span>
                              添加
                            </button>
                          </div>
                          <div
                            v-if="headerRows.length === 0"
                            class="text-[11px] text-slate-400 border border-dashed border-slate-200 rounded-lg p-2.5 text-center"
                          >
                            暂无请求头，点击「添加」可设置
                            Referer、Authorization 等
                          </div>
                          <div
                            v-else
                            class="rounded-lg border border-slate-200 overflow-hidden"
                          >
                            <div
                              v-for="(row, i) in headerRows"
                              :key="i"
                              class="flex items-center gap-1.5 px-2 py-1.5 border-b border-slate-100 last:border-0"
                            >
                              <input
                                v-model="row.key"
                                placeholder="键（如 Referer）"
                                class="flex-1 min-w-0 text-[11px] font-mono bg-transparent outline-none placeholder:text-slate-300 text-slate-700"
                              />
                              <span class="text-slate-300 shrink-0">:</span>
                              <input
                                v-model="row.value"
                                placeholder="值"
                                class="flex-2 min-w-0 text-[11px] font-mono bg-transparent outline-none placeholder:text-slate-300 text-slate-700"
                              />
                              <button
                                type="button"
                                class="shrink-0 text-slate-300 hover:text-red-400 transition-colors"
                                @click="removeHeaderRow(i)"
                              >
                                <svg
                                  class="w-3.5 h-3.5"
                                  fill="none"
                                  viewBox="0 0 24 24"
                                  stroke="currentColor"
                                  stroke-width="2"
                                >
                                  <path
                                    stroke-linecap="round"
                                    stroke-linejoin="round"
                                    d="M6 18L18 6M6 6l12 12"
                                  />
                                </svg>
                              </button>
                            </div>
                          </div>
                        </div>

                        <!-- ── 参数脚本 ── -->
                        <div>
                          <div class="flex items-center justify-between mb-1.5">
                            <div>
                              <span class="text-xs font-medium text-slate-600"
                                >动态参数脚本</span
                              >
                              <p class="text-[10px] text-slate-400 mt-0.5">
                                在 URL 模板中用
                                <code
                                  class="font-mono bg-slate-100 px-0.5 rounded"
                                  >{参数名}</code
                                >
                                引用，脚本为 JS 表达式，下载前自动求值
                              </p>
                            </div>
                            <button
                              type="button"
                              class="text-[11px] text-blue-600 hover:text-blue-700 font-medium flex items-center gap-0.5 shrink-0 self-start"
                              @click="addScriptRow"
                            >
                              <span class="text-base leading-none">+</span>
                              添加
                            </button>
                          </div>
                          <div
                            v-if="scriptRows.length === 0"
                            class="text-[11px] text-slate-400 border border-dashed border-slate-200 rounded-lg p-2.5 text-center"
                          >
                            例：名称
                            <code class="font-mono bg-slate-100 px-0.5 rounded"
                              >ts</code
                            >，脚本
                            <code class="font-mono bg-slate-100 px-0.5 rounded"
                              >Date.now()</code
                            >，URL 中用
                            <code class="font-mono bg-slate-100 px-0.5 rounded"
                              >{ts}</code
                            >
                          </div>
                          <div v-else class="space-y-1.5">
                            <div
                              v-for="(row, i) in scriptRows"
                              :key="i"
                              class="rounded-lg border overflow-hidden"
                              :class="
                                row.error
                                  ? 'border-red-200 bg-red-50/40'
                                  : 'border-slate-200'
                              "
                            >
                              <div
                                class="flex items-center gap-1.5 px-2 py-1.5 border-b border-slate-100"
                              >
                                <span
                                  class="text-[10px] text-slate-400 shrink-0"
                                  >名称</span
                                >
                                <input
                                  v-model="row.name"
                                  placeholder="参数名（如 token）"
                                  class="flex-1 min-w-0 text-[11px] font-mono bg-transparent outline-none placeholder:text-slate-300 text-slate-700"
                                />
                                <button
                                  type="button"
                                  class="shrink-0 text-slate-300 hover:text-red-400 transition-colors"
                                  @click="removeScriptRow(i)"
                                >
                                  <svg
                                    class="w-3.5 h-3.5"
                                    fill="none"
                                    viewBox="0 0 24 24"
                                    stroke="currentColor"
                                    stroke-width="2"
                                  >
                                    <path
                                      stroke-linecap="round"
                                      stroke-linejoin="round"
                                      d="M6 18L18 6M6 6l12 12"
                                    />
                                  </svg>
                                </button>
                              </div>
                              <div class="px-2 py-1.5">
                                <span
                                  class="text-[10px] text-slate-400 block mb-1"
                                  >JS 表达式</span
                                >
                                <textarea
                                  v-model="row.script"
                                  placeholder="Date.now()  /  Math.round(Date.now()/1000)  /  'static_value'"
                                  rows="2"
                                  spellcheck="false"
                                  class="w-full text-[11px] font-mono bg-transparent outline-none resize-none placeholder:text-slate-300 text-slate-700"
                                />
                                <p
                                  v-if="row.error"
                                  class="text-[10px] text-red-500 mt-0.5"
                                >
                                  ⚠ {{ row.error }}
                                </p>
                              </div>
                            </div>
                          </div>
                        </div>
                      </div>
                    </div>
                  </div>
                </div>
              </div>

              <Transition name="fade-slide-up">
                <div
                  v-if="errorMsg"
                  class="mt-4 flex items-start gap-2 p-3 bg-red-50 border border-red-200 rounded-xl text-xs text-red-600"
                >
                  <AlertCircle
                    class="w-3.5 h-3.5 mt-0.5 shrink-0 text-red-500"
                  />
                  {{ errorMsg }}
                </div>
              </Transition>
            </template>

            <!-- 步骤 2：多图层选择（WMTS / 网页抓取）+ 瓦片预览 -->
            <template v-else-if="step === 2">
              <div class="flex gap-5">
                <!-- 左：图层列表 -->
                <div class="flex-1 min-w-0">
                  <p class="text-sm text-slate-600 mb-3">
                    {{
                      sourceType === "web" ? "页面中发现" : "该 WMTS 服务包含"
                    }}
                    <strong class="text-slate-900">{{
                      wmtsLayers.length
                    }}</strong>
                    个{{ sourceType === "web" ? "瓦片服务" : "图层" }}，请选择：
                  </p>
                  <div
                    class="space-y-1 max-h-72 overflow-y-auto rounded-xl border border-slate-200"
                  >
                    <button
                      v-for="(layer, idx) in wmtsLayers"
                      :key="idx"
                      class="w-full flex items-start gap-3 px-3 py-2.5 text-left transition-colors"
                      :class="
                        selectedLayerIdx === idx
                          ? 'bg-blue-50 text-blue-700'
                          : 'hover:bg-slate-50 text-slate-800'
                      "
                      @click="onLayerSelect(idx)"
                    >
                      <div
                        class="w-2 h-2 rounded-full shrink-0 mt-1.5 transition-colors"
                        :class="
                          selectedLayerIdx === idx
                            ? 'bg-blue-500'
                            : 'bg-slate-300'
                        "
                      />
                      <div class="flex-1 min-w-0">
                        <div class="text-xs font-medium truncate">
                          {{ layer.name }}
                        </div>
                        <div
                          class="text-[10px] font-mono text-slate-400 truncate mt-0.5"
                        >
                          {{ layer.url_template }}
                        </div>
                      </div>
                      <span
                        class="text-[11px] text-slate-400 shrink-0 mt-0.5"
                        >{{ layer.crs }}</span
                      >
                    </button>
                  </div>
                </div>

                <!-- 右：瓦片预览 -->
                <div class="shrink-0 w-52 flex flex-col">
                  <div
                    class="flex items-center gap-1.5 text-xs font-medium text-slate-600 mb-2"
                  >
                    <Eye class="w-3.5 h-3.5" />
                    <span>图层预览</span>
                  </div>
                  <div
                    class="rounded-xl overflow-hidden border border-slate-200 bg-slate-100 relative"
                    style="width: 208px; height: 208px"
                  >
                    <!-- 加载中 -->
                    <div
                      v-if="previewLoading"
                      class="absolute inset-0 flex items-center justify-center bg-slate-100"
                    >
                      <Loader2 class="w-6 h-6 animate-spin text-slate-400" />
                    </div>
                    <!-- 3×3 瓦片网格 -->
                    <div v-else class="grid grid-cols-3 w-full h-full">
                      <div
                        v-for="(blobUrl, i) in previewBlobUrls"
                        :key="i"
                        class="bg-slate-200 overflow-hidden"
                        style="width: 69px; height: 69px"
                      >
                        <img
                          v-if="blobUrl"
                          :src="blobUrl"
                          class="w-full h-full object-cover"
                          draggable="false"
                        />
                        <div v-else class="w-full h-full bg-slate-200/80" />
                      </div>
                    </div>
                  </div>
                  <p
                    class="text-[10px] text-slate-400 mt-1.5 truncate font-mono"
                    :title="parsedSource?.url_template"
                  >
                    {{ parsedSource?.url_template }}
                  </p>
                </div>
              </div>
            </template>
          </div>

          <!-- 底部 -->
          <div
            class="flex items-center justify-between px-6 py-4 border-t bg-slate-50/60"
            style="border-color: var(--color-border-subtle)"
          >
            <UiButton
              v-if="step === 2"
              variant="ghost"
              size="sm"
              @click="
                step = 1;
                errorMsg = '';
              "
              >返回</UiButton
            >
            <div v-else />
            <div class="flex items-center gap-2">
              <UiButton variant="ghost" size="sm" @click="emit('close')"
                >取消</UiButton
              >
              <UiButton
                v-if="sourceType !== 'file' || step === 2"
                size="sm"
                :disabled="
                  isLoading ||
                  (step === 2 && !parsedSource) ||
                  (sourceType === 'web' &&
                    captureStatus === 'capturing' &&
                    capturedTiles.length === 0)
                "
                @click="handleNext"
              >
                <Loader2 v-if="isLoading" class="size-3.5 animate-spin" />
                <Radio
                  v-else-if="
                    sourceType === 'web' && captureStatus === 'capturing'
                  "
                  class="size-3.5 animate-pulse"
                />
                <Check v-else-if="step === 2" class="size-3.5" />
                {{
                  step === 2
                    ? "确认选择"
                    : sourceType === "web"
                      ? captureStatus === "capturing"
                        ? "完成抓取"
                        : "开始抓取"
                      : "解析"
                }}
                <ArrowRight
                  v-if="
                    step !== 2 &&
                    !isLoading &&
                    !(sourceType === 'web' && captureStatus === 'capturing')
                  "
                  class="size-3.5"
                />
              </UiButton>
            </div>
          </div>
        </div>
      </Transition>
    </div>
  </Transition>
</template>

<style scoped>
.fade-slide-up-enter-active,
.fade-slide-up-leave-active {
  transition:
    opacity 0.2s ease,
    transform 0.2s ease;
}
.fade-slide-up-enter-from,
.fade-slide-up-leave-to {
  opacity: 0;
  transform: translateY(6px);
}
</style>
