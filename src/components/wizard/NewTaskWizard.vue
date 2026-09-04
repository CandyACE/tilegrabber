<script setup lang="ts">
import { ref, watch, onUnmounted, computed } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import {
  Upload,
  Globe,
  Link,
  Scan,
  Database,
  Layers,
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
import type { CoordType, TileSource } from "~/types/tile-source";
import { useI18n } from "vue-i18n";
import { BASEMAP_PRESETS, BASEMAP_CATEGORIES } from "~/data/basemap-presets";

const emit = defineEmits<{
  confirm: [source: TileSource];
  close: [];
}>();

const { t, locale } = useI18n();

type SourceType = "file" | "wmts" | "tms" | "web" | "mbtiles" | "preset";
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
// Monotonic counter: each loadTilePreview call increments this; stale calls bail out
let previewRequestId = 0;

// ─── 请求配置（Headers + Param Scripts）─────────────────────────────────────

const advancedOpen = ref(false);
const headerRows = ref<{ key: string; value: string }[]>([]);
const scriptRows = ref<{ name: string; script: string; error: string }[]>([]);
// 瓦片像素尺寸由用户明确配置，不根据 URL 参数自动猜测。
const tileSize = ref(256);
// 自定义图源默认按 WGS84 处理；中国偏移图源由用户明确选择 GCJ02。
const coordType = ref<CoordType>("WGS84");
const tileSizeValid = computed(
  () =>
    Number.isInteger(tileSize.value) &&
    tileSize.value >= 64 &&
    tileSize.value <= 4096,
);

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

/** 切换来源类型时恢复安全的默认瓦片尺寸，避免上一个来源的配置串入新任务。 */
function selectSourceType(value: SourceType) {
  sourceType.value = value;
  urlInput.value = "";
  errorMsg.value = "";
  tileSize.value = 256;
  coordType.value = "WGS84";
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

/** 把当前瓦片尺寸、headerRows 和 scriptRows 合并到 source 后返回新对象。 */
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
  // handleNext 已完成范围校验；这里保留防御性回退，避免异常状态写入任务配置。
  const configuredTileSize = tileSizeValid.value
    ? tileSize.value
    : source.tile_size || 256;
  console.info("[NewTaskWizard] 应用瓦片与坐标配置", {
    source: source.name,
    tileSize: configuredTileSize,
    coordType: coordType.value,
  });
  return {
    ...source,
    tile_size: configuredTileSize,
    coord_type: coordType.value,
    headers,
    extra_params,
    param_scripts,
  };
}

const sourceTypeOptions = computed(() => [
  {
    value: "file" as const,
    icon: Upload,
    label: t('wizard.sourceFile'),
    desc: t('wizard.sourceFileDesc'),
  },
  {
    value: "wmts" as const,
    icon: Globe,
    label: t('wizard.sourceWmts'),
    desc: t('wizard.sourceWmtsDesc'),
  },
  {
    value: "tms" as const,
    icon: Link,
    label: t('wizard.sourceTms'),
    desc: t('wizard.sourceTmsDesc'),
  },
  {
    value: "web" as const,
    icon: Scan,
    label: t('wizard.sourceWeb'),
    desc: t('wizard.sourceWebDesc'),
  },
  {
    value: "mbtiles" as const,
    icon: Database,
    label: t('wizard.sourceMbtiles'),
    desc: t('wizard.sourceMbtilesDesc'),
  },
  {
    value: "preset" as const,
    icon: Layers,
    label: t('wizard.sourcePreset'),
    desc: t('wizard.sourcePresetDesc'),
  },
] as const)

function handlePresetSelect(preset: typeof BASEMAP_PRESETS[number]) {
  wmtsLayers.value = [];  // clear any previous WMTS layer list
  capturedTiles.value = [];
  coordType.value = preset.source.coord_type;
  if (preset.requiresToken) {
    openTokenDialog(preset);
    return;
  }
  parsedSource.value = { ...preset.source, name: preset.name };
  step.value = 2;
  loadTilePreview(parsedSource.value);
}

// ── token 弹层（用于天地图等需要凭证的预设）─────────────────────────────────
const tokenDialogOpen = ref(false);
const tokenDialogPreset = ref<typeof BASEMAP_PRESETS[number] | null>(null);
const tokenDialogValue = ref("");
const tokenDialogLoading = ref(false);

async function openTokenDialog(preset: typeof BASEMAP_PRESETS[number]) {
  tokenDialogPreset.value = preset;
  tokenDialogOpen.value = true;
  tokenDialogValue.value = "";
  const settingKey = preset.requiresToken?.settingKey;
  if (settingKey) {
    try {
      const saved = await invoke<string | null>("get_setting", { key: settingKey });
      if (saved) tokenDialogValue.value = saved;
    } catch {
      // ignore
    }
  }
}

function closeTokenDialog() {
  tokenDialogOpen.value = false;
  tokenDialogPreset.value = null;
  tokenDialogValue.value = "";
}

async function confirmTokenDialog() {
  const preset = tokenDialogPreset.value;
  const token = tokenDialogValue.value.trim();
  if (!preset || !preset.requiresToken || !token) return;
  tokenDialogLoading.value = true;
  try {
    await invoke("set_setting", {
      key: preset.requiresToken.settingKey,
      value: token,
    });
  } catch {
    // 持久化失败不阻塞，本次仍可用
  }
  const tokenKey = preset.requiresToken.tokenKey;
  coordType.value = preset.source.coord_type;
  const merged: TileSource = {
    ...preset.source,
    name: preset.name,
    extra_params: { ...preset.source.extra_params, [tokenKey]: token },
  };
  parsedSource.value = merged;
  tokenDialogLoading.value = false;
  closeTokenDialog();
  step.value = 2;
  loadTilePreview(merged);
}

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
  const myRequestId = ++previewRequestId;
  revokePreviews();
  previewLoading.value = true;

  const b = src.bounds;
  let lat = (b.north + b.south) / 2;
  let lon = (b.east + b.west) / 2;
  const lonSpan = b.east - b.west;
  const latSpan = b.north - b.south;
  // 近全球范围（例如默认基础图层预设）：根据界面语言选择一个有陆地的中心点，避免落在大西洋空白海域
  const isNearGlobal =
    lonSpan > 300 ||
    (b.west < -150 && b.east > 150 && latSpan > 140);
  if (isNearGlobal) {
    if (locale.value.startsWith("zh")) {
      lat = 35;
      lon = 104;
    } else {
      lat = 40;
      lon = -3;
    }
  }
  // Pick a zoom level appropriate for the area, then clamp to the source's actual range
  const lonSpanZ = lonSpan > 80 ? 4 : lonSpan > 20 ? 6 : 8;
  let minZ = src.min_zoom ?? 0;
  let maxZ = src.max_zoom ?? 18;

  // For TileGrabber's own WMTS sources, fetch actual zoom range from the REST API
  // (Rust capabilities parsing may not have populated min/max zoom yet)
  const isTileGrabberWmts =
    src.url_template.includes("localhost:8765/wmts/") ||
    src.url_template.includes("127.0.0.1:8765/wmts/");
  if (isTileGrabberWmts && minZ === 0 && maxZ === 18) {
    try {
      const taskIdMatch = src.url_template.match(/wmts\/([^?/&]+)/);
      if (taskIdMatch) {
        const taskId = taskIdMatch[1];
        const apiBase = src.url_template.replace(/\/wmts\/.*/, "");
        const resp = await fetch(`${apiBase}/api/tasks/${taskId}`);
        if (resp.ok) {
          const task = await resp.json();
          if (typeof task.minZoom === "number") minZ = task.minZoom;
          if (typeof task.maxZoom === "number") maxZ = task.maxZoom;
        }
      }
    } catch {
      // ignore, fall through with defaults
    }
  }

  const z = Math.max(minZ, Math.min(maxZ, lonSpanZ));

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
      if (previewRequestId !== myRequestId) return null; // cancelled by a newer call
      const url = buildTileUrl(src, z, x, y);
      try {
        const bytes = await invoke<number[]>("fetch_tile", {
          url,
          headers: src.headers ?? {},
        });
        const blob = new Blob([new Uint8Array(bytes)], { type: "image/png" });
        return URL.createObjectURL(blob);
      } catch (e) {
        console.error(`[TilePreview] fetch failed z=${z} x=${x} y=${y} url=${url}:`, e);
        return null;
      }
    }),
  );

  if (previewRequestId !== myRequestId) return; // a newer call superseded us
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
  const usesRequestConfig =
    sourceType.value !== "file" && sourceType.value !== "mbtiles";
  if (usesRequestConfig && !tileSizeValid.value) {
    console.warn("[NewTaskWizard] 拒绝无效瓦片尺寸", {
      tileSize: tileSize.value,
    });
    errorMsg.value = t("wizard.tileSizeInvalid");
    return;
  }
  if (step.value === 1) {
    isLoading.value = true;
    try {
      if (sourceType.value === "file") await pickAndParseFile();
      else if (sourceType.value === "wmts") await parseWmts();
      else if (sourceType.value === "web") {
        if (captureStatus.value === "idle") await startCapture();
        else await finishCapture();
      } else if (sourceType.value === "mbtiles") await pickAndParseMbtiles();
      else await parseTms();
    } catch (e: unknown) {
      errorMsg.value = e instanceof Error ? e.message : String(e);
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
    title: t('wizard.selectFileTitle'),
    filters: [{ name: t('wizard.selectFileFilter'), extensions: ["lrc", "lra", "ovmap"] }],
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
    errorMsg.value = t('wizard.errNoWmtsUrl');
    return;
  }
  const results: TileSource[] = await invoke("parse_wmts_url", {
    url: urlInput.value.trim(),
  });
  if (!results.length) {
    errorMsg.value = t('wizard.errNoLayers');
    return;
  }
  wmtsLayers.value = results;
  parsedSource.value = results[0];
  selectedLayerIdx.value = 0;
  step.value = 2;
  loadTilePreview(results[0]);
}

async function parseTms() {
  if (!urlInput.value.trim()) {
    errorMsg.value = t('wizard.errNoTmsUrl');
    return;
  }
  const result: TileSource = await invoke("parse_tms_url", {
    url: urlInput.value.trim(),
    name: customName.value || null,
  });
  emit("confirm", applyRequestConfig(result));
}

async function pickAndParseMbtiles() {
  const selected = await open({
    multiple: false,
    title: t('wizard.mbtilesPickerTitle'),
    filters: [{ name: "MBTiles", extensions: ["mbtiles"] }],
  });
  if (!selected) return;
  const filePath = typeof selected === "string" ? selected : selected[0];
  const result: TileSource = await invoke("parse_mbtiles_source", {
    path: filePath,
  });
  emit("confirm", result);
}

/** 开始抓取：打开 WebView 窗口，开始轮询捕获结果 */
async function startCapture() {
  if (!urlInput.value.trim()) {
    errorMsg.value = t('wizard.errNoWebUrl');
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
    errorMsg.value = t('wizard.noCaptured');
    return;
  }
  wmtsLayers.value = tiles;
  parsedSource.value = tiles[0];
  selectedLayerIdx.value = 0;
  step.value = 2;
  loadTilePreview(tiles[0]);
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
          class="bg-white rounded-2xl shadow-xl flex flex-col w-full border transition-[max-width] duration-300 overflow-hidden"
          :class="step === 2 ? 'max-w-190' : 'max-w-135'"
          style="max-height: 86vh; border-color: var(--color-border-subtle)"
        >
          <!-- 头部 -->
          <div
            class="relative px-6 pt-6 pb-4 border-b"
            style="border-color: var(--color-border-subtle)"
          >
            <div class="flex items-center justify-between">
              <div class="flex items-center gap-3">
                <div
                  class="w-9 h-9 rounded-xl bg-blue-50 border border-blue-100 flex items-center justify-center shrink-0"
                >
                  <PlusCircle class="w-4.5 h-4.5 text-blue-600" />
                </div>
                <div>
                  <h2 class="text-sm font-semibold text-slate-900">
                    {{ t('wizard.selectSource') }}
                  </h2>
                  <p class="text-xs text-slate-500 mt-0.5">
                    {{ step === 1 ? t('wizard.step1Desc') : t('wizard.step2Desc') }}
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
                    selectSourceType(opt.value);
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
                        {{ t('wizard.filePickerTitle') }}
                      </p>
                      <p class="text-xs text-slate-500 mt-1">
                        {{ t('wizard.filePickerDesc') }}
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
                      {{ t('wizard.wmtsHint') }}
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
                        >{{ t('wizard.tmsLabel') }}</label
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
                        >{{ t('wizard.customNameLabel') }}</label
                      >
                      <UiInput
                        v-model="customName"
                        :placeholder="t('wizard.customNamePlaceholder')"
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
                      >{{ t('wizard.webUrlLabel') }}</label
                    >
                    <UiInput
                      v-model="urlInput"
                      placeholder="https://example.com/map"
                      class="w-full font-mono text-xs"
                      @keydown.enter="handleNext"
                    />
                    <p class="text-[11px] text-slate-400">
                      {{ t('wizard.webHint') }}
                    </p>
                  </div>
                  <!-- 抓取中：实时结果 -->
                  <div v-else class="space-y-2 pb-1">
                    <div
                      class="flex items-center gap-2 text-xs text-emerald-600 font-medium"
                    >
                      <Radio class="w-3.5 h-3.5 animate-pulse" />
                      <span>{{ t('wizard.capturing') }}</span>
                      <span class="ml-auto text-slate-400"
                        >{{ t('wizard.capturedCount', { count: capturedTiles.length }) }}</span
                      >
                    </div>
                    <div
                      v-if="capturedTiles.length === 0"
                      class="rounded-lg border border-dashed border-slate-200 p-3 text-[11px] text-slate-400 text-center"
                    >
                      {{ t('wizard.captureWaiting') }}
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

              <!-- MBTiles -->
              <div
                class="grid transition-[grid-template-rows] duration-300 ease-out"
                :style="{
                  gridTemplateRows: sourceType === 'mbtiles' ? '1fr' : '0fr',
                }"
              >
                <div class="overflow-hidden">
                  <div
                    class="border-2 border-dashed border-slate-200 rounded-xl p-8 flex flex-col items-center gap-3 text-center hover:border-blue-400 hover:bg-blue-50/40 transition-all duration-200 cursor-pointer"
                    @click="sourceType === 'mbtiles' && handleNext()"
                  >
                    <div
                      class="w-12 h-12 rounded-full bg-slate-100 flex items-center justify-center"
                    >
                      <Database class="w-5 h-5 text-slate-500" />
                    </div>
                    <div>
                      <p class="text-sm font-medium text-slate-700">
                        {{ t('wizard.mbtilesPickerTitle') }}
                      </p>
                      <p class="text-xs text-slate-500 mt-1">
                        {{ t('wizard.mbtilesPickerDesc') }}
                      </p>
                    </div>
                  </div>
                </div>
              </div>

              <!-- 预置底图 -->
              <div
                class="grid transition-[grid-template-rows] duration-300 ease-out"
                :style="{
                  gridTemplateRows: sourceType === 'preset' ? '1fr' : '0fr',
                }"
              >
                <div class="overflow-hidden">
                  <div class="space-y-3 pb-1">
                    <div
                      v-for="cat in BASEMAP_CATEGORIES"
                      :key="cat.id"
                    >
                      <p class="text-xs font-semibold text-slate-500 mb-2">
                        {{ cat.label }}
                      </p>
                      <div class="grid grid-cols-2 gap-1.5">
                        <button
                          v-for="preset in BASEMAP_PRESETS.filter(p => p.category === cat.id)"
                          :key="preset.id"
                          type="button"
                          class="flex items-center gap-2 px-3 py-2 rounded-lg border transition-colors text-left text-xs bg-white hover:bg-blue-50 hover:border-blue-400 hover:text-blue-700 border-slate-200 text-slate-700"
                          @click="handlePresetSelect(preset)"
                        >
                          <span class="flex-1 font-medium">{{ preset.name }}</span>
                          <span
                            v-if="preset.source.coord_type === 'GCJ02'"
                            class="text-[9px] px-1 py-0.5 rounded bg-amber-100 text-amber-600 font-bold shrink-0"
                          >GCJ02</span>
                        </button>
                      </div>
                    </div>
                  </div>
                </div>
              </div>

              <!-- ── 请求配置（仅 wmts / tms / web 显示）────────────────────── -->
              <div
                class="grid transition-[grid-template-rows] duration-300 ease-out mt-3"
                :style="{
                  gridTemplateRows: (sourceType !== 'file' && sourceType !== 'mbtiles' && sourceType !== 'preset') ? '1fr' : '0fr',
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
                    <span>{{ t('wizard.requestConfig') }}</span>
                    <span
                      v-if="
                        tileSize !== 256 ||
                        coordType !== 'WGS84' ||
                        headerRows.length ||
                        scriptRows.length
                      "
                      class="ml-1 px-1.5 py-0.5 rounded-full text-[10px] bg-blue-100 text-blue-600 font-semibold"
                    >
                      {{
                        (tileSize !== 256 ? 1 : 0) +
                        (coordType !== 'WGS84' ? 1 : 0) +
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
                        <!-- ── 瓦片大小 ── -->
                        <div>
                          <div class="flex items-center justify-between gap-4">
                            <div class="min-w-0">
                              <span class="text-xs font-medium text-slate-600">
                                {{ t("wizard.tileSize") }}
                              </span>
                              <p
                                id="tile-size-hint"
                                class="text-[10px] text-slate-400 mt-0.5"
                              >
                                {{ t("wizard.tileSizeHint") }}
                              </p>
                            </div>
                            <div class="flex items-center gap-1.5 shrink-0">
                              <UiInput
                                v-model.number="tileSize"
                                type="number"
                                min="64"
                                max="4096"
                                step="1"
                                aria-describedby="tile-size-hint"
                                :aria-invalid="!tileSizeValid"
                                class="w-24 h-8 text-xs font-mono"
                                :class="
                                  tileSizeValid
                                    ? ''
                                    : 'border-red-300 focus-visible:ring-red-200'
                                "
                              />
                              <span class="text-[11px] text-slate-400">px</span>
                            </div>
                          </div>
                          <p
                            v-if="!tileSizeValid"
                            class="text-[10px] text-red-500 mt-1"
                          >
                            {{ t("wizard.tileSizeInvalid") }}
                          </p>
                        </div>

                        <!-- ── 坐标偏移类型 ── -->
                        <div>
                          <div class="flex items-center justify-between gap-4">
                            <div class="min-w-0">
                              <span class="text-xs font-medium text-slate-600">
                                {{ t("wizard.coordType") }}
                              </span>
                              <p class="text-[10px] text-slate-400 mt-0.5">
                                {{ t("wizard.coordTypeHint") }}
                              </p>
                            </div>
                            <select
                              v-model="coordType"
                              class="h-8 w-36 shrink-0 rounded-md border border-slate-200 bg-white px-2 text-xs text-slate-700 outline-none focus:border-blue-400 focus:ring-2 focus:ring-blue-100"
                            >
                              <option value="WGS84">
                                {{ t("wizard.coordTypeWgs84") }}
                              </option>
                              <option value="GCJ02">
                                {{ t("wizard.coordTypeGcj02") }}
                              </option>
                            </select>
                          </div>
                        </div>

                        <!-- ──  请求头 ── -->
                        <div>
                          <div class="flex items-center justify-between mb-1.5">
                            <span class="text-xs font-medium text-slate-600"
                              >{{ t('wizard.httpHeaders') }}</span
                            >
                            <button
                              type="button"
                              class="text-[11px] text-blue-600 hover:text-blue-700 font-medium flex items-center gap-0.5"
                              @click="addHeaderRow"
                            >
                              <span class="text-base leading-none">+</span>
                              {{ t('wizard.add') }}
                            </button>
                          </div>
                          <div
                            v-if="headerRows.length === 0"
                            class="text-[11px] text-slate-400 border border-dashed border-slate-200 rounded-lg p-2.5 text-center"
                          >
                            {{ t('wizard.noHeaders') }}
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
                                :placeholder="t('wizard.headerKey')"
                                class="flex-1 min-w-0 text-[11px] font-mono bg-transparent outline-none placeholder:text-slate-300 text-slate-700"
                              />
                              <span class="text-slate-300 shrink-0">:</span>
                              <input
                                v-model="row.value"
                                :placeholder="t('wizard.headerValue')"
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
                                >{{ t('wizard.dynamicParams') }}</span
                              >
                              <p class="text-[10px] text-slate-400 mt-0.5">
                                {{ t('wizard.dynamicParamsHint') }}
                                <code
                                  class="font-mono bg-slate-100 px-0.5 rounded"
                                  >{{ '{' + t('wizard.paramNameExample') + '}' }}</code
                                >
                                {{ t('wizard.dynamicParamsHintPost') }}
                              </p>
                            </div>
                            <button
                              type="button"
                              class="text-[11px] text-blue-600 hover:text-blue-700 font-medium flex items-center gap-0.5 shrink-0 self-start"
                              @click="addScriptRow"
                            >
                              <span class="text-base leading-none">+</span>
                              {{ t('wizard.add') }}
                            </button>
                          </div>
                          <div
                            v-if="scriptRows.length === 0"
                            class="text-[11px] text-slate-400 border border-dashed border-slate-200 rounded-lg p-2.5 text-center"
                          >
                            {{ t('wizard.dynamicParamsEmpty') }}
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
                                  >{{ t('wizard.paramName') }}</span
                                >
                                <input
                                  v-model="row.name"
                                  :placeholder="t('wizard.paramNamePlaceholder')"
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
                                  >{{ t('wizard.jsExpr') }}</span
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
                <!-- 左：图层列表 or 预置信息 -->
                <template v-if="wmtsLayers.length > 0">
                  <div class="flex-1 min-w-0">
                    <p class="text-sm text-slate-600 mb-3">
                      {{ sourceType === "web" ? t('wizard.webFoundPrefix') : t('wizard.wmtsContainsPrefix') }}
                      <strong class="text-slate-900">{{
                        wmtsLayers.length
                      }}</strong>
                      {{ sourceType === "web" ? t('wizard.webFoundSuffix') : t('wizard.wmtsContainsSuffix') }}
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
                </template>
                <!-- 预置底图信息面板（无图层列表时显示） -->
                <template v-else>
                  <div class="flex-1 min-w-0 flex flex-col justify-center">
                    <div class="rounded-xl border border-slate-200 p-4 bg-slate-50 space-y-3">
                      <div>
                        <p class="text-sm font-semibold text-slate-900 mb-1">{{ parsedSource?.name }}</p>
                        <p class="text-[11px] font-mono text-slate-400 break-all leading-relaxed">{{ parsedSource?.url_template }}</p>
                      </div>
                      <div class="flex flex-wrap gap-1.5">
                        <span class="text-[11px] px-2 py-0.5 rounded-full bg-slate-100 text-slate-600">{{ parsedSource?.crs }}</span>
                        <span
                          v-if="parsedSource?.coord_type === 'GCJ02'"
                          class="text-[11px] px-2 py-0.5 rounded-full bg-amber-100 text-amber-700 font-semibold"
                        >GCJ02 纠偏</span>
                      </div>
                      <p class="text-[11px] text-slate-400">{{ t('wizard.presetPreviewHint') }}</p>
                    </div>
                  </div>
                </template>

                <!-- 右：瓦片预览 -->
                <div class="shrink-0 w-52 flex flex-col">
                  <div
                    class="flex items-center gap-1.5 text-xs font-medium text-slate-600 mb-2"
                  >
                    <Eye class="w-3.5 h-3.5" />
                    <span>{{ t('wizard.layerPreview') }}</span>
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
                wmtsLayers = [];
                parsedSource = null;
                revokePreviews();
              "
              >{{ t('wizard.back') }}</UiButton
            >
            <div v-else />
            <div class="flex items-center gap-2">
              <UiButton variant="ghost" size="sm" @click="emit('close')"
                >{{ t('wizard.cancel') }}</UiButton
              >
              <UiButton
                v-if="(sourceType !== 'file' && sourceType !== 'preset') || step === 2"
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
                    ? t('wizard.confirm')
                    : sourceType === "web"
                      ? captureStatus === "capturing"
                        ? t('wizard.stopCapture')
                        : t('wizard.startCapture')
                      : t('wizard.parse')
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

  <!-- ── Token 输入弹层（天地图等需 token 的预设）──────────────────────────── -->
  <Transition
    enter-active-class="transition duration-150 ease-out"
    enter-from-class="opacity-0"
    enter-to-class="opacity-100"
    leave-active-class="transition duration-150 ease-in"
    leave-from-class="opacity-100"
    leave-to-class="opacity-0"
  >
    <div
      v-if="tokenDialogOpen && tokenDialogPreset"
      class="fixed inset-0 z-[60] flex items-center justify-center bg-black/40 backdrop-blur-sm"
      @click.self="closeTokenDialog"
    >
      <div class="w-[420px] bg-white rounded-xl shadow-2xl p-5 space-y-4">
        <div class="flex items-start gap-3">
          <div class="w-9 h-9 rounded-lg bg-blue-50 flex items-center justify-center shrink-0">
            <Layers class="w-5 h-5 text-blue-600" />
          </div>
          <div class="flex-1 min-w-0">
            <p class="text-sm font-semibold text-slate-800">
              {{ tokenDialogPreset.name }}
            </p>
            <p class="text-xs text-slate-500 mt-0.5">
              {{ t('wizard.presetTokenHint', { label: tokenDialogPreset.requiresToken?.label }) }}
            </p>
          </div>
          <button
            type="button"
            class="text-slate-400 hover:text-slate-600 transition-colors"
            @click="closeTokenDialog"
          >
            <X class="w-4 h-4" />
          </button>
        </div>

        <div class="space-y-2">
          <label class="text-xs font-medium text-slate-700">
            {{ tokenDialogPreset.requiresToken?.label }}
          </label>
          <UiInput
            v-model="tokenDialogValue"
            type="text"
            :placeholder="t('wizard.presetTokenPlaceholder')"
            class="text-sm font-mono"
            autofocus
            @keydown.enter.prevent="confirmTokenDialog"
          />
          <a
            v-if="tokenDialogPreset.requiresToken?.helpUrl"
            :href="tokenDialogPreset.requiresToken.helpUrl"
            target="_blank"
            rel="noopener"
            class="text-[11px] text-blue-600 hover:underline inline-flex items-center gap-1"
          >
            {{ t('wizard.presetTokenApply') }}
            <ArrowRight class="w-3 h-3" />
          </a>
        </div>

        <div class="flex justify-end gap-2 pt-1">
          <UiButton variant="ghost" size="sm" @click="closeTokenDialog">
            {{ t('wizard.cancel') }}
          </UiButton>
          <UiButton
            variant="default"
            size="sm"
            :disabled="!tokenDialogValue.trim() || tokenDialogLoading"
            @click="confirmTokenDialog"
          >
            <Loader2 v-if="tokenDialogLoading" class="w-3.5 h-3.5 mr-1 animate-spin" />
            {{ t('wizard.confirm') }}
          </UiButton>
        </div>
      </div>
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
