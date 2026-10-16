<script setup lang="ts">
import { ref, computed, watch, onMounted, onUnmounted } from "vue";
import { useI18n } from "vue-i18n";
import {
  Globe,
  Link,
  FileText,
  Archive,
  BoxSelect,
  Trash2,
  Download,
  ChevronLeft,
  ChevronDown,
  MapPin,
  Scissors,
  Square,
  Hexagon,
  FolderOpen,
  Share2,
  Loader2,
} from "lucide-vue-next";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { mergeDownloadAreas, presetDownloadArea, type DownloadArea, type PresetRegion, type AreaPolygons } from "~/lib/download-area";
import ZoomPicker from "./ZoomPicker.vue";
import UiInput from "@/components/ui/input/Input.vue";
import type { TileSource, Bounds, CrsType } from "~/types/tile-source";
import { useRemoteStore } from "~/composables/useRemoteStore";

const { t } = useI18n();
const { connectedServer } = useRemoteStore();

// ─── Props & Emits ────────────────────────────────────────────────────────────

const props = defineProps<{
  source: TileSource;
  bounds: Bounds | null;
  polygons?: AreaPolygons | null;
  drawActive: boolean;
}>();

const emit = defineEmits<{
  close: [];
  "toggle-draw": [];
  "clear-bounds": [];
  "start-download": [
    config: {
      name: string;
      minZoom: number;
      maxZoom: number;
      zoomLevels: number[] | null;
      clipToBounds: boolean;
      remoteTarget: { url: string; token: string; name: string } | null;
    },
  ];
  "draw-mode-change": [mode: "rectangle" | "polygon"];
  "import-bounds": [bounds: Bounds, polygons: [number, number][][] | null];
}>();

// ─── 下载配置 ──────────────────────────────────────────────────────────────────

const taskName = ref(props.source.name);
const minZoom = ref(props.source.min_zoom ?? 8);
const maxZoom = ref(props.source.max_zoom ?? 14);
const clipToBounds = ref(false);
const drawMode = ref<"rectangle" | "polygon">("rectangle");
// 下载目标：null = 本地，否则为远端服务器
const useRemoteTarget = ref(false);
const submitLoading = ref(false);

// 连续范围为默认模式；勾选模式允许跳过中间层级。
const customZooms = ref(false);
const selectedZooms = ref<number[]>([]);
const availableZooms = computed(() => Array.from({ length: maxZoom.value - minZoom.value + 1 }, (_, i) => minZoom.value + i));
watch([minZoom, maxZoom, customZooms], () => {
  selectedZooms.value = availableZooms.value.slice();
});
const zoomLevels = computed(() => customZooms.value ? [...selectedZooms.value].sort((a, b) => a - b) : null);
const validZooms = computed(() => !customZooms.value || selectedZooms.value.length > 0);

function submitDownload() {
  if (!props.bounds || !validZooms.value) return;
  console.info("[DownloadSetupPanel] 提交区域组合下载", { zoomLevels: zoomLevels.value });
  emit("start-download", {
    name: taskName.value,
    minZoom: minZoom.value,
    maxZoom: maxZoom.value,
    zoomLevels: zoomLevels.value,
    clipToBounds: clipToBounds.value,
    remoteTarget: useRemoteTarget.value && connectedServer.value
      ? { url: connectedServer.value.url, token: connectedServer.value.token, name: connectedServer.value.name }
      : null,
  });
}

function setDrawMode(mode: "rectangle" | "polygon") {
  drawMode.value = mode;
  emit("draw-mode-change", mode);
}

// ─── KML / KMZ / GeoJSON 导入 ─────────────────────────────────────────────────

const importError = ref("");

const importing = ref(false);
const appendArea = ref(true);
const presets = ref<PresetRegion[]>([]);
// 国家、省份和城市逐级选择；城市只引用省级数据中的外环，避免重复打包边界。
interface PresetCity { id: string; name: string; polygonStart: number; polygonCount: number }
const cityIndex = ref<Record<string, PresetCity[]>>({});
const selectedProvince = ref("");
const selectedCity = ref("");
const presetsLoading = ref(false);
const presetsLoaded = ref(false);
const presetsError = ref("");
const provinces = computed(() => presets.value.filter(region => region.id !== "中华人民共和国"));
const cities = computed(() => cityIndex.value[selectedProvince.value] ?? []);
const selectedRegion = computed<PresetRegion | undefined>(() => {
  const province = presets.value.find(region => region.id === (selectedProvince.value || "中华人民共和国"));
  if (!province || !selectedCity.value) return province;
  const city = cities.value.find(region => region.id === selectedCity.value);
  if (!city) return undefined;
  return { id: city.name, polygons: province.polygons.slice(city.polygonStart, city.polygonStart + city.polygonCount) };
});
watch(selectedProvince, (province) => {
  // 更换省份必须清除旧城市，防止跨省误用先前的市级边界。
  selectedCity.value = "";
  console.info("[DownloadSetupPanel] 切换行政区省份", { province });
});
onMounted(loadPresets);

// 所有文件解析成功后一次性替换，任一失败都不会破坏当前已选区域。
function applyAreas(areas: DownloadArea[]) {
  if (appendArea.value && props.bounds) areas.unshift({ ...props.bounds, polygons: props.polygons ?? null });
  const result = mergeDownloadAreas(areas);
  emit("import-bounds", result, result.polygons);
}

async function triggerImport() {
  if (importing.value) return;
  importError.value = "";
  importing.value = true;
  try {
    const paths = await openDialog({
      title: t("downloadSetup.importAreaTitle"), multiple: true, directory: false,
      filters: [{ name: t("downloadSetup.importAreaFilter"), extensions: ["kml", "kmz", "geojson", "json"] }],
    });
    if (!paths) return;
    const files = Array.isArray(paths) ? paths : [paths];
    if (!files.length) return;
    const areas = await Promise.all(files.map(path => invoke<DownloadArea>("parse_area_file", { path })));
    applyAreas(areas);
    console.info("[DownloadSetupPanel] 多文件导入完成", { files: files.length });
  } catch (e) {
    console.error("[DownloadSetupPanel] 区域导入失败", e);
    importError.value = String(e);
  } finally {
    importing.value = false;
  }
}

/** 打开下载面板后加载离线数据，边界和城市索引全部成功后才允许应用。 */
async function loadPresets() {
  if (presetsLoading.value || presetsLoaded.value) return;
  presetsLoading.value = true;
  try {
    const [regions, index] = await Promise.all([
      import("~/assets/regions/china.json"), import("~/assets/regions/cities.json"),
    ]);
    presets.value = regions.default as PresetRegion[];
    cityIndex.value = index.default as Record<string, PresetCity[]>;
    presetsLoaded.value = true;
    presetsError.value = "";
    console.info("[DownloadSetupPanel] 离线行政区加载完成", { provinces: provinces.value.length });
  } catch (e) {
    console.error("[DownloadSetupPanel] 预置行政区加载失败", e);
    presetsError.value = String(e);
  } finally {
    presetsLoading.value = false;
  }
}

function applyPresets() {
  try {
    if (!presetsLoaded.value || !selectedRegion.value) return;
    applyAreas([presetDownloadArea(selectedRegion.value)]);
    console.info("[DownloadSetupPanel] 应用行政区下拉选择", { province: selectedProvince.value, city: selectedCity.value });
    importError.value = "";
  } catch (e) {
    console.error("[DownloadSetupPanel] 行政区选择失败", e);
    importError.value = String(e);
  }
}

// ─── 辅助计算 ──────────────────────────────────────────────────────────────────

const crs = computed<CrsType>(() => props.source.crs ?? "WEB_MERCATOR");

const kindIcon = computed(() => {
  const icons: Record<string, unknown> = {
    lrc: FileText,
    lra: Archive,
    ovmap: Archive,
    wmts: Globe,
    tms: Link,
  };
  return icons[props.source.kind ?? ""] ?? Globe;
});

const crsLabel = computed(() => {
  const labels: Record<string, string> = {
    WEB_MERCATOR: t('downloadSetup.crsLabels.WEB_MERCATOR'),
    WGS84: t('downloadSetup.crsLabels.WGS84'),
    TERRAIN: t('downloadSetup.crsLabels.TERRAIN'),
    UNKNOWN: t('downloadSetup.crsLabels.UNKNOWN'),
  };
  return labels[props.source.crs ?? ""] ?? "—";
});

// ─── 下载规模估算 ──────────────────────────────────────────────────────────────

interface EstimateInfo {
  tile_count: number;
  estimated_bytes: number;
}

const estimate = ref<EstimateInfo | null>(null);
let estimateTimer: ReturnType<typeof setTimeout> | null = null;
// 参数变化即使旧请求失效，避免快速切换区域或层级时显示过期估算。
let estimateRevision = 0;

function formatBytes(bytes: number): string {
  if (bytes >= 1024 * 1024 * 1024)
    return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
  if (bytes >= 1024 * 1024)
    return `${(bytes / (1024 * 1024)).toFixed(0)} MB`;
  return `${(bytes / 1024).toFixed(0)} KB`;
}

async function refreshEstimate(revision: number) {
  if (!props.bounds || !validZooms.value) {
    estimate.value = null;
    return;
  }
  try {
    const result = await invoke<EstimateInfo>("estimate_download", {
      west: props.bounds.west,
      east: props.bounds.east,
      south: props.bounds.south,
      north: props.bounds.north,
      minZoom: minZoom.value,
      maxZoom: maxZoom.value,
      zoomLevels: zoomLevels.value,
      crsStr: props.source.crs ?? "WEB_MERCATOR",
    });
    if (revision === estimateRevision) estimate.value = result;
  } catch (error) {
    if (revision === estimateRevision) estimate.value = null;
    console.warn("[DownloadSetupPanel] 下载规模估算失败", error);
  }
}

function scheduleEstimate() {
  if (estimateTimer) clearTimeout(estimateTimer);
  const revision = ++estimateRevision;
  estimate.value = null;
  estimateTimer = setTimeout(() => refreshEstimate(revision), 400);
}

watch(() => [props.bounds, minZoom.value, maxZoom.value, zoomLevels.value], scheduleEstimate, {
  immediate: true,
});
onUnmounted(() => {
  estimateRevision++;
  if (estimateTimer) clearTimeout(estimateTimer);
});
</script>

<template>
  <div
    class="flex flex-col h-full border-r"
    style="
      width: 280px;
      min-width: 280px;
      background: var(--color-surface);
      border-color: var(--color-border-subtle);
    "
  >
    <!-- 头部 -->
    <div
      class="flex items-center gap-2 px-3 py-3 border-b shrink-0"
      style="border-color: var(--color-border-subtle)"
    >
      <button
        class="p-1 rounded hover:bg-slate-100 transition-colors"
        :title="t('downloadSetup.returnToList')"
        @click="emit('close')"
      >
        <ChevronLeft class="size-4 text-slate-500" />
      </button>
      <span
        class="text-sm font-semibold"
        style="color: var(--color-text-primary)"
      >
        {{ t('downloadSetup.title') }}
      </span>
    </div>

    <!-- 可滚动内容 -->

    <div class="flex-1 overflow-y-auto p-4 space-y-5">
      <!-- ⓪ 任务名称 -->
      <div>
        <div
          class="text-xs font-medium uppercase tracking-wider mb-1.5"
          style="color: var(--color-text-muted)"
        >
          {{ t('downloadSetup.taskName') }}
        </div>
        <UiInput v-model="taskName" :placeholder="t('downloadSetup.taskNamePlaceholder')" class="w-full" />
      </div>

      <!-- ① 数据源信息 -->
      <div>
        <div
          class="text-xs font-medium uppercase tracking-wider mb-2"
          style="color: var(--color-text-muted)"
        >
          {{ t('downloadSetup.dataSource') }}
        </div>
        <div
          class="rounded-lg border p-3"
          style="
            border-color: var(--color-border-subtle);
            background: var(--color-surface-raised);
          "
        >
          <div class="flex items-start gap-3">
            <div
              class="w-8 h-8 rounded-lg bg-blue-50 flex items-center justify-center shrink-0"
            >
              <component :is="kindIcon" class="size-4 text-blue-600" />
            </div>
            <div class="flex-1 min-w-0">
              <div
                class="text-sm font-semibold text-gray-900 truncate"
                :title="source.name"
              >
                {{ source.name }}
              </div>
              <div class="flex items-center gap-2 mt-1 flex-wrap">
                <span
                  class="text-xs px-1.5 py-0.5 rounded font-mono uppercase"
                  style="
                    background: var(--color-bg-subtle);
                    color: var(--color-text-secondary);
                  "
                >
                  {{ source.format }}
                </span>
                <span class="text-xs" style="color: var(--color-text-muted)">{{
                  crsLabel
                }}</span>
                <span
                  class="text-xs font-medium uppercase"
                  style="color: var(--color-text-muted)"
                  >{{ source.kind }}</span
                >
              </div>
            </div>
          </div>
        </div>
      </div>

      <!-- ② 绘制下载范围 -->
      <div>
        <div
          class="text-xs font-medium uppercase tracking-wider mb-2"
          style="color: var(--color-text-muted)"
        >
          {{ t('downloadSetup.drawArea') }}
        </div>

        <!-- 绘制模式选择 -->
        <div class="flex gap-1.5 mb-2">
          <button
            class="flex-1 flex items-center justify-center gap-1.5 px-2 py-1.5 rounded-md text-xs font-medium border transition-colors"
            :class="
              drawMode === 'rectangle'
                ? 'border-blue-500 bg-blue-50 text-blue-700'
                : 'border-slate-200 text-slate-500 hover:border-slate-300'
            "
            @click="setDrawMode('rectangle')"
          >
            <Square class="size-3.5" />
            {{ t('downloadSetup.drawRectangle') }}
          </button>
          <button
            class="flex-1 flex items-center justify-center gap-1.5 px-2 py-1.5 rounded-md text-xs font-medium border transition-colors"
            :class="
              drawMode === 'polygon'
                ? 'border-blue-500 bg-blue-50 text-blue-700'
                : 'border-slate-200 text-slate-500 hover:border-slate-300'
            "
            @click="setDrawMode('polygon')"
          >
            <Hexagon class="size-3.5" />
            {{ t('downloadSetup.drawPolygon') }}
          </button>
          <button
            class="flex items-center justify-center gap-1 px-2 py-1.5 rounded-md text-xs font-medium border border-slate-200 text-slate-500 hover:border-emerald-400 hover:bg-emerald-50 hover:text-emerald-700 transition-colors"
            :title="t('downloadSetup.importAreaTitle')"
            :disabled="importing"
            @click="triggerImport"
          >
            <FolderOpen class="size-3.5" />
            {{ t('downloadSetup.importFile') }}
          </button>
        </div>

        <!-- 多次导入可追加；行政区与文件使用同一套合并逻辑。 -->
        <label class="flex items-center gap-2 text-xs mb-2">
          <input v-model="appendArea" type="checkbox" />{{ t('downloadSetup.appendArea') }}
        </label>
        <!-- 行政区卡片沿用周围控件的圆角与主题色；三个字段共用标签列和输入列。 -->
        <div class="region-card mb-3 rounded-lg border p-3">
          <div class="flex items-center gap-2 mb-3">
            <MapPin class="size-3.5 shrink-0 text-slate-400" aria-hidden="true" />
            <span class="text-xs font-medium text-slate-700">{{ t('downloadSetup.presets') }}</span>
          </div>
          <div class="space-y-2">
            <label class="region-field">
              <span class="region-label">{{ t('downloadSetup.country') }}</span>
              <span class="region-control">
                <select class="region-select" :aria-label="t('downloadSetup.country')" :disabled="!presetsLoaded">
                  <option value="中华人民共和国">{{ t('regions.中华人民共和国') }}</option>
                </select>
                <ChevronDown class="region-chevron" aria-hidden="true" />
              </span>
            </label>
            <label class="region-field">
              <span class="region-label">{{ t('downloadSetup.province') }}</span>
              <span class="region-control">
                <select v-model="selectedProvince" class="region-select" :aria-label="t('downloadSetup.province')" :disabled="!presetsLoaded">
                  <option value="">{{ t('downloadSetup.wholeCountry') }}</option>
                  <option v-for="region in provinces" :key="region.id" :value="region.id">{{ t(`regions.${region.id}`) }}</option>
                </select>
                <ChevronDown class="region-chevron" aria-hidden="true" />
              </span>
            </label>
            <label class="region-field">
              <span class="region-label">{{ t('downloadSetup.city') }}</span>
              <span class="region-control">
                <select v-model="selectedCity" class="region-select" :aria-label="t('downloadSetup.city')" :disabled="!presetsLoaded || !cities.length">
                  <option value="">{{ t('downloadSetup.wholeProvince') }}</option>
                  <option v-for="city in cities" :key="city.id" :value="city.id">{{ city.name }}</option>
                </select>
                <ChevronDown class="region-chevron" aria-hidden="true" />
              </span>
            </label>
          </div>
          <p v-if="presetsError" class="mt-2 text-xs text-red-500 break-words" role="alert">{{ presetsError }}</p>
          <p v-if="presetsLoading" class="region-hint mt-2" role="status">{{ t('downloadSetup.loadingRegions') }}</p>
          <button v-else-if="!presetsLoaded" class="mt-2 text-xs text-blue-600 hover:text-blue-700" @click="loadPresets">{{ t('downloadSetup.retryRegions') }}</button>
          <p v-else-if="selectedProvince && !cities.length" class="region-hint mt-2">{{ t('downloadSetup.noCityBoundaries') }}</p>
          <button class="region-apply mt-3 flex h-8 w-full items-center justify-center gap-1.5 rounded-md border text-xs font-medium transition-colors disabled:opacity-40 disabled:cursor-not-allowed" :disabled="!presetsLoaded || !selectedRegion" @click="applyPresets">
            <BoxSelect class="size-3.5" aria-hidden="true" />
            {{ t('downloadSetup.applyPresets') }}
          </button>
        </div>

        <!-- 导入错误提示 -->
        <p v-if="importError" class="text-xs text-red-500 mb-2">
          {{ importError }}
        </p>

        <!-- 未选区域 -->
        <template v-if="!bounds">
          <p class="text-xs mb-2" style="color: var(--color-text-muted)">
            {{
              drawMode === "rectangle"
                ? t('downloadSetup.rectPrompt')
                : t('downloadSetup.polyPrompt')
            }}
          </p>
          <button
            class="w-full flex items-center justify-center gap-2 px-3 py-2.5 rounded-lg border-2 transition-all text-sm font-medium"
            :class="
              drawActive
                ? 'border-blue-500 bg-blue-50 text-blue-700'
                : 'border-gray-200 hover:border-blue-400 hover:bg-blue-50/50 text-gray-600'
            "
            @click="emit('toggle-draw')"
          >
            <BoxSelect class="size-4" />
            {{
              drawActive
                ? t('downloadSetup.cancelDraw')
                : drawMode === "rectangle"
                  ? t('downloadSetup.startRect')
                  : t('downloadSetup.startPoly')
            }}
          </button>
        </template>

        <!-- 已选区域 -->
        <template v-else>
          <div class="rounded-lg border-2 border-green-300 bg-green-50 p-3">
            <div class="flex items-center justify-between mb-2">
              <span class="text-xs font-semibold text-green-700"
                >{{ t('downloadSetup.areaSelected') }}</span
              >
              <button
                class="flex items-center gap-1 text-xs text-gray-400 hover:text-red-500 transition-colors"
                @click="emit('clear-bounds')"
              >
                <Trash2 class="size-3" />
                {{ t('downloadSetup.clearArea') }}
              </button>
            </div>
            <div class="grid grid-cols-2 gap-1 text-xs font-mono text-gray-600">
              <span>W: {{ bounds.west.toFixed(4) }}</span>
              <span>E: {{ bounds.east.toFixed(4) }}</span>
              <span>S: {{ bounds.south.toFixed(4) }}</span>
              <span>N: {{ bounds.north.toFixed(4) }}</span>
            </div>
          </div>
        </template>
      </div>

      <!-- ③ 裁剪设置 -->
      <div>
        <div
          class="text-xs font-medium uppercase tracking-wider mb-2"
          style="color: var(--color-text-muted)"
        >
          {{ t('downloadSetup.downloadOptions') }}
        </div>
        <div
          class="flex items-center justify-between gap-3 rounded-lg border p-3"
          style="
            border-color: var(--color-border-subtle);
            background: var(--color-surface-raised);
          "
        >
          <div class="flex items-center gap-2 min-w-0">
            <Scissors class="size-3.5 shrink-0 text-slate-400" />
            <div>
              <div class="text-xs font-medium text-slate-700">
                {{ t('downloadSetup.clipOption') }}
              </div>
              <div class="text-[11px] text-slate-400 mt-0.5 leading-tight">
                {{ t('downloadSetup.clipDesc') }}
              </div>
            </div>
          </div>
          <button
            type="button"
            role="switch"
            :aria-checked="clipToBounds"
            class="relative inline-flex h-5 w-9 shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-200 focus-visible:outline-none"
            :class="clipToBounds ? 'bg-blue-600' : 'bg-slate-200'"
            @click="clipToBounds = !clipToBounds"
          >
            <span
              class="pointer-events-none inline-block size-4 rounded-full bg-white shadow-sm ring-0 transition-transform duration-200"
              :class="clipToBounds ? 'translate-x-4' : 'translate-x-0'"
            />
          </button>
        </div>
      </div>

      <!-- ④ 缩放级别 -->
      <div>
        <div
          class="text-xs font-medium uppercase tracking-wider mb-2"
          style="color: var(--color-text-muted)"
        >
          {{ t('downloadSetup.zoomLevel') }}
        </div>
        <ZoomPicker
          v-model:min-zoom="minZoom"
          v-model:max-zoom="maxZoom"
          :bounds="bounds"
          :crs="crs"
          :show-count="false"
        />
        <label class="flex items-center gap-2 text-xs mt-2"><input v-model="customZooms" type="checkbox" />{{ t("downloadSetup.customZooms") }}</label>
        <div v-if="customZooms" class="grid grid-cols-4 gap-2 mt-2">
          <label v-for="z in availableZooms" :key="z" class="flex items-center gap-1 text-xs"><input v-model="selectedZooms" type="checkbox" :value="z" />Z{{ z }}</label>
        </div>
        <p v-if="!validZooms" class="text-xs text-red-500 mt-1">{{ t("downloadSetup.selectZoom") }}</p>
      </div>
    </div>

    <!-- 底部操作 -->
    <div
      class="shrink-0 p-4 border-t space-y-2"
      style="border-color: var(--color-border-subtle)"
    >
      <!-- 下载目标选择器（有远端连接时才显示） -->
      <div v-if="connectedServer" class="rounded-lg border overflow-hidden" style="border-color: var(--color-border-subtle)">
        <button
          class="w-full flex items-center gap-2 px-3 py-2 text-xs transition-colors"
          :class="!useRemoteTarget ? 'bg-blue-50 text-blue-700 font-medium' : 'text-slate-500 hover:bg-slate-50'"
          @click="useRemoteTarget = false"
        >
          <span class="size-3 rounded-full border-2 shrink-0" :class="!useRemoteTarget ? 'border-blue-500 bg-blue-500' : 'border-slate-300'" />
          <Download :size="11" />
          {{ t('downloadSetup.targetLocal') }}
        </button>
        <div class="h-px" style="background: var(--color-border-subtle)" />
        <button
          class="w-full flex items-center gap-2 px-3 py-2 text-xs transition-colors"
          :class="useRemoteTarget ? 'bg-blue-50 text-blue-700 font-medium' : 'text-slate-500 hover:bg-slate-50'"
          @click="useRemoteTarget = true"
        >
          <span class="size-3 rounded-full border-2 shrink-0" :class="useRemoteTarget ? 'border-blue-500 bg-blue-500' : 'border-slate-300'" />
          <Share2 :size="11" />
          {{ t('downloadSetup.targetRemote', { name: connectedServer.name }) }}
        </button>
      </div>

      <!-- 下载规模估算 -->
      <div
        v-if="bounds && estimate"
        class="flex items-center justify-between text-xs px-1"
        style="color: var(--color-text-muted)"
      >
        <span>{{ t('downloadSetup.estimateTiles', { count: estimate.tile_count.toLocaleString() }) }}</span>
        <span class="font-mono">~{{ formatBytes(estimate.estimated_bytes) }}</span>
      </div>
      <button
        class="w-full flex items-center justify-center gap-2 rounded-lg py-2.5 text-sm font-semibold transition-colors"
        :class="
          bounds
            ? (useRemoteTarget ? 'bg-violet-600 text-white hover:bg-violet-700 cursor-pointer' : 'bg-blue-600 text-white hover:bg-blue-700 cursor-pointer')
            : 'bg-slate-100 text-slate-400 cursor-not-allowed'
        "
        :disabled="!bounds || !validZooms || submitLoading || importing"
        @click="submitDownload"
      >
        <Loader2 v-if="submitLoading" class="size-4 animate-spin" />
        <Share2 v-else-if="useRemoteTarget" class="size-4" />
        <Download v-else class="size-4" />
        {{ useRemoteTarget ? t('downloadSetup.submitRemote') : t('downloadSetup.startDownload') }}
      </button>
      <button
        class="w-full text-sm py-1 transition-colors"
        style="color: var(--color-text-muted)"
        @click="emit('close')"
      >
        {{ t('downloadSetup.cancel') }}
      </button>
    </div>
  </div>
</template>

<style scoped>
/* 使用面板现有主题色，避免默认黑边与原生箭头造成视觉割裂。 */
.region-card {
  border-color: var(--color-border-subtle);
  background: var(--color-surface);
}
.region-field {
  display: grid;
  grid-template-columns: minmax(0, 4.5rem) minmax(0, 1fr);
  align-items: center;
  gap: 0.5rem;
}
.region-label {
  color: var(--color-text-secondary);
  font-size: 0.75rem;
  line-height: 1.25rem;
}
.region-control {
  position: relative;
  display: block;
  min-width: 0;
}
.region-select {
  appearance: none;
  width: 100%;
  min-width: 0;
  height: 2rem;
  padding: 0 1.75rem 0 0.625rem;
  border: 1px solid var(--color-border-subtle);
  border-radius: 0.375rem;
  background: var(--color-surface);
  color: var(--color-text-primary);
  font: inherit;
  font-size: 0.75rem;
  text-overflow: ellipsis;
  cursor: pointer;
  transition: border-color 150ms, box-shadow 150ms;
}
.region-select:hover:not(:disabled) { border-color: var(--color-border); }
.region-select:disabled {
  background: var(--color-app-bg);
  color: var(--color-text-muted);
  cursor: not-allowed;
}
.region-select:focus-visible {
  outline: none;
  border-color: var(--color-accent);
  box-shadow: 0 0 0 2px var(--color-accent-bg);
}
.region-chevron {
  position: absolute;
  top: 50%;
  right: 0.625rem;
  width: 0.875rem;
  height: 0.875rem;
  transform: translateY(-50%);
  color: var(--color-text-muted);
  pointer-events: none;
}
.region-hint {
  color: var(--color-text-muted);
  font-size: 0.6875rem;
  line-height: 1.5;
}
.region-apply {
  border-color: #bfdbfe;
  color: #1d4ed8;
  background: #eff6ff;
}
.region-apply:hover:not(:disabled) { background: #dbeafe; border-color: #93c5fd; }
.region-apply:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; }
</style>
