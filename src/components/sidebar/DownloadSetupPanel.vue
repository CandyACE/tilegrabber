<script setup lang="ts">
import { ref, computed } from "vue";
import {
  Globe,
  Link,
  FileText,
  Archive,
  BoxSelect,
  Trash2,
  Download,
  ChevronLeft,
  Scissors,
  Square,
  Hexagon,
  FolderOpen,
} from "lucide-vue-next";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import ZoomPicker from "./ZoomPicker.vue";
import UiInput from "@/components/ui/input/Input.vue";
import type { TileSource, Bounds, CrsType } from "~/types/tile-source";

// ─── Props & Emits ────────────────────────────────────────────────────────────

const props = defineProps<{
  source: TileSource;
  bounds: Bounds | null;
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
      clipToBounds: boolean;
    },
  ];
  "draw-mode-change": [mode: "rectangle" | "polygon"];
  "import-bounds": [bounds: Bounds, polygon: [number, number][] | null];
}>();

// ─── 下载配置 ──────────────────────────────────────────────────────────────────

const taskName = ref(props.source.name);
const minZoom = ref(props.source.min_zoom ?? 8);
const maxZoom = ref(props.source.max_zoom ?? 14);
const clipToBounds = ref(false);
const drawMode = ref<"rectangle" | "polygon">("rectangle");

function setDrawMode(mode: "rectangle" | "polygon") {
  drawMode.value = mode;
  emit("draw-mode-change", mode);
}

// ─── KML / KMZ / GeoJSON 导入 ─────────────────────────────────────────────────

const importError = ref("");

async function triggerImport() {
  importError.value = "";
  const path = await openDialog({
    title: "导入区域文件",
    multiple: false,
    directory: false,
    filters: [
      {
        name: "区域文件 (KML, KMZ, GeoJSON)",
        extensions: ["kml", "kmz", "geojson", "json"],
      },
    ],
  });
  if (!path || typeof path !== "string") return;
  try {
    const result = await invoke<{
      west: number;
      east: number;
      south: number;
      north: number;
      polygon: [number, number][] | null;
    }>("parse_area_file", { path });
    const bounds: Bounds = {
      west: result.west,
      east: result.east,
      south: result.south,
      north: result.north,
    };
    emit("import-bounds", bounds, result.polygon);
  } catch (e) {
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
    WEB_MERCATOR: "EPSG:3857",
    WGS84: "EPSG:4326",
    TERRAIN: "地形高程",
    UNKNOWN: "未知",
  };
  return labels[props.source.crs ?? ""] ?? "—";
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
        title="返回任务列表"
        @click="emit('close')"
      >
        <ChevronLeft class="size-4 text-slate-500" />
      </button>
      <span
        class="text-sm font-semibold"
        style="color: var(--color-text-primary)"
      >
        新建下载任务
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
          任务名称
        </div>
        <UiInput v-model="taskName" placeholder="输入任务名称" class="w-full" />
      </div>

      <!-- ① 数据源信息 -->
      <div>
        <div
          class="text-xs font-medium uppercase tracking-wider mb-2"
          style="color: var(--color-text-muted)"
        >
          数据源
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
          1 · 绘制下载范围
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
            矩形
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
            多边形
          </button>
          <button
            class="flex items-center justify-center gap-1 px-2 py-1.5 rounded-md text-xs font-medium border border-slate-200 text-slate-500 hover:border-emerald-400 hover:bg-emerald-50 hover:text-emerald-700 transition-colors"
            title="从 KML / GeoJSON 文件导入范围"
            @click="triggerImport"
          >
            <FolderOpen class="size-3.5" />
            导入
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
                ? "在地图上拖拽选框，确定要下载的地理范围。"
                : "在地图上依次点击顶点，双击完成多边形绘制。"
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
                ? "点击取消绘制"
                : drawMode === "rectangle"
                  ? "开始框选"
                  : "开始绘制"
            }}
          </button>
        </template>

        <!-- 已选区域 -->
        <template v-else>
          <div class="rounded-lg border-2 border-green-300 bg-green-50 p-3">
            <div class="flex items-center justify-between mb-2">
              <span class="text-xs font-semibold text-green-700"
                >✓ 已选择区域</span
              >
              <button
                class="flex items-center gap-1 text-xs text-gray-400 hover:text-red-500 transition-colors"
                @click="emit('clear-bounds')"
              >
                <Trash2 class="size-3" />
                清除
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

      <!-- ③ 缩放级别 -->
      <div>
        <div
          class="text-xs font-medium uppercase tracking-wider mb-2"
          style="color: var(--color-text-muted)"
        >
          2 · 选择下载级别
        </div>
        <ZoomPicker
          v-model:min-zoom="minZoom"
          v-model:max-zoom="maxZoom"
          :bounds="bounds"
          :crs="crs"
        />
      </div>

      <!-- ④ 裁剪设置 -->
      <div>
        <div
          class="text-xs font-medium uppercase tracking-wider mb-2"
          style="color: var(--color-text-muted)"
        >
          3 · 下载选项
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
                严格裁剪至选框范围
              </div>
              <div class="text-[11px] text-slate-400 mt-0.5 leading-tight">
                导出时将边缘瓦片精确裁剪至选框，避免溢出区域数据
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
    </div>

    <!-- 底部操作 -->
    <div
      class="shrink-0 p-4 border-t space-y-2"
      style="border-color: var(--color-border-subtle)"
    >
      <button
        class="w-full flex items-center justify-center gap-2 rounded-lg py-2.5 text-sm font-semibold transition-colors"
        :class="
          bounds
            ? 'bg-blue-600 text-white hover:bg-blue-700 cursor-pointer'
            : 'bg-slate-100 text-slate-400 cursor-not-allowed'
        "
        :disabled="!bounds"
        @click="
          emit('start-download', {
            name: taskName,
            minZoom,
            maxZoom,
            clipToBounds,
          })
        "
      >
        <Download class="size-4" />
        开始下载
      </button>
      <button
        class="w-full text-sm py-1 transition-colors"
        style="color: var(--color-text-muted)"
        @click="emit('close')"
      >
        取消
      </button>
    </div>
  </div>
</template>
