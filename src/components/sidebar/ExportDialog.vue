<script setup lang="ts">
import { ref, computed, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { save as saveDialog, open as openDir } from "@tauri-apps/plugin-dialog";
import {
  X,
  PackageCheck,
  FolderOpen,
  Database,
  Info,
  Loader2,
  AlertCircle,
  Layers,
  Map,
  Scissors,
} from "lucide-vue-next";
import { useExportJobs } from "~/composables/useExportJobs";

// ─── 类型 ──────────────────────────────────────────────────────────────────
type ExportFormat = "mbtiles" | "directory" | "tiff";

interface TaskInfo {
  id: string;
  name: string;
  downloadedTiles: number;
  minZoom: number;
  maxZoom: number;
  boundsWest: number;
  boundsEast: number;
  boundsSouth: number;
  boundsNorth: number;
}

// ─── Props / Emits ─────────────────────────────────────────────────────────
const props = defineProps<{
  open: boolean;
  task: TaskInfo;
}>();

const emit = defineEmits<{
  close: [];
}>();

// ─── 状态 ──────────────────────────────────────────────────────────────────
const format = ref<ExportFormat>("mbtiles");
const destPath = ref("");
const clipToBounds = ref(false);
const tiffZoom = ref(0);
const starting = ref(false);
const startError = ref<string | null>(null);

const { registerJob } = useExportJobs();

watch(
  () => props.open,
  (val) => {
    if (val) {
      format.value = "mbtiles";
      destPath.value = "";
      clipToBounds.value = false;
      tiffZoom.value = props.task.maxZoom;
      starting.value = false;
      startError.value = null;
    }
  },
);

// ─── 路径选择 ──────────────────────────────────────────────────────────────
async function pickMbtilesPath() {
  const p = await saveDialog({
    title: "导出为 MBTiles",
    defaultPath: `${props.task.name}.mbtiles`,
    filters: [{ name: "MBTiles 瓦片包", extensions: ["mbtiles"] }],
  });
  if (p) destPath.value = p;
}

async function pickDirectory() {
  const p = await openDir({
    title: "选择导出目录",
    directory: true,
  });
  if (p)
    destPath.value = typeof p === "string" ? p : ((p as string[])[0] ?? "");
}

async function pickTiffPath() {
  const p = await saveDialog({
    title: "导出为 GeoTIFF",
    defaultPath: `${props.task.name}.tif`,
    filters: [{ name: "GeoTIFF 图像", extensions: ["tif", "tiff"] }],
  });
  if (p) destPath.value = p;
}

async function pickPath() {
  if (format.value === "mbtiles") await pickMbtilesPath();
  else if (format.value === "directory") await pickDirectory();
  else await pickTiffPath();
}

// ─── 开始导出 ──────────────────────────────────────────────────────────────
const canExport = computed(() => destPath.value.length > 0 && !starting.value);

async function doExport() {
  if (!canExport.value) return;
  starting.value = true;
  startError.value = null;
  try {
    let jobId: string;
    if (format.value === "mbtiles") {
      jobId = await invoke<string>("export_mbtiles", {
        taskId: props.task.id,
        destPath: destPath.value,
        clipToBounds: clipToBounds.value,
      });
    } else if (format.value === "directory") {
      jobId = await invoke<string>("export_directory", {
        taskId: props.task.id,
        destDir: destPath.value,
        clipToBounds: clipToBounds.value,
      });
    } else {
      jobId = await invoke<string>("export_geotiff", {
        taskId: props.task.id,
        destPath: destPath.value,
        zoom: tiffZoom.value,
        clipToBounds: clipToBounds.value,
      });
    }
    registerJob(jobId, props.task.id, format.value, destPath.value);
    emit("close");
  } catch (e) {
    startError.value = String(e);
    starting.value = false;
  }
}

// ─── 工具 ──────────────────────────────────────────────────────────────────
function fmtCount(n: number) {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(n);
}

const formatLabel: Record<ExportFormat, string> = {
  mbtiles: "MBTiles",
  directory: "目录 (z/x/y)",
  tiff: "GeoTIFF",
};
</script>

<template>
  <Transition name="modal-backdrop">
    <div
      v-if="open"
      class="fixed inset-0 top-11 z-50 flex items-center justify-center p-4"
      style="background: rgba(15, 23, 42, 0.45); backdrop-filter: blur(2px)"
      @click.self="emit('close')"
    >
      <Transition name="modal-panel" appear>
        <div
          v-if="open"
          class="relative w-full max-w-lg rounded-2xl shadow-xl flex flex-col border bg-white overflow-hidden"
          style="
            max-height: min(640px, calc(100vh - 7rem));
            border-color: var(--color-border-subtle);
          "
        >
          <!-- ── 顶部色带 + 标题 ───────────────────────────────────────── -->
          <div
            class="relative px-6 pt-6 pb-4 border-b"
            style="border-color: var(--color-border-subtle)"
          >
            <!-- 渐变装饰 -->
            <div
              class="absolute inset-x-0 top-0 h-0.5 bg-linear-to-r from-blue-400 via-blue-500 to-indigo-500 rounded-t-2xl"
            />
            <div class="flex items-start justify-between gap-4">
              <div class="flex items-center gap-3">
                <div
                  class="w-9 h-9 rounded-xl bg-blue-50 border border-blue-100 flex items-center justify-center shrink-0"
                >
                  <PackageCheck class="size-4.5 text-blue-600" />
                </div>
                <div>
                  <h2
                    class="text-sm font-semibold text-slate-900 leading-tight"
                  >
                    导出瓦片
                  </h2>
                  <p class="text-xs text-slate-500 mt-0.5 truncate max-w-64">
                    {{ task.name }}
                  </p>
                </div>
              </div>
              <button
                class="shrink-0 flex items-center justify-center size-7 rounded-lg text-slate-400 hover:text-slate-600 hover:bg-slate-100 transition-colors mt-0.5"
                @click="emit('close')"
              >
                <X class="size-4" />
              </button>
            </div>
          </div>

          <!-- ── 正文 ──────────────────────────────────────────────────── -->
          <div class="flex-1 overflow-y-auto px-6 py-5 space-y-5">
            <!-- 任务统计条 -->
            <div
              class="flex items-center gap-2 rounded-xl bg-slate-50 border border-slate-200 px-4 py-3"
            >
              <Info class="size-3.5 shrink-0 text-blue-500" />
              <span class="text-xs text-slate-600">
                共
                <strong class="text-slate-900 font-semibold">{{
                  fmtCount(task.downloadedTiles)
                }}</strong>
                个瓦片 &nbsp;·&nbsp; 层级 z{{ task.minZoom }}–z{{
                  task.maxZoom
                }}
              </span>
              <Layers class="size-3.5 shrink-0 text-slate-400 ml-auto" />
            </div>

            <!-- 格式选择 -->
            <div>
              <p
                class="text-xs font-semibold text-slate-500 uppercase tracking-wide mb-3"
              >
                选择导出格式
              </p>
              <div class="grid grid-cols-3 gap-2.5">
                <!-- MBTiles -->
                <button
                  class="group relative flex flex-col gap-2 rounded-xl p-3 border-2 text-left transition-all duration-200"
                  :class="
                    format === 'mbtiles'
                      ? 'border-blue-500 bg-blue-50 shadow-sm shadow-blue-100'
                      : 'border-slate-200 hover:border-slate-300 bg-white'
                  "
                  @click="
                    format = 'mbtiles';
                    destPath = '';
                  "
                >
                  <div class="flex items-center gap-2">
                    <div
                      class="w-7 h-7 rounded-lg flex items-center justify-center transition-colors duration-200"
                      :class="
                        format === 'mbtiles'
                          ? 'bg-blue-100'
                          : 'bg-slate-100 group-hover:bg-slate-200'
                      "
                    >
                      <Database
                        class="size-3.5 transition-colors duration-200"
                        :class="
                          format === 'mbtiles'
                            ? 'text-blue-600'
                            : 'text-slate-500'
                        "
                      />
                    </div>
                    <Transition name="scale-in">
                      <div
                        v-if="format === 'mbtiles'"
                        class="ml-auto size-4 rounded-full bg-blue-500 flex items-center justify-center shrink-0"
                      >
                        <span class="block size-1.5 rounded-full bg-white" />
                      </div>
                    </Transition>
                  </div>
                  <span
                    class="text-xs font-semibold transition-colors duration-200 leading-tight"
                    :class="
                      format === 'mbtiles' ? 'text-blue-700' : 'text-slate-800'
                    "
                  >
                    MBTiles
                  </span>
                  <p class="text-[10px] leading-relaxed text-slate-400">
                    单文件 SQLite，兼容 QGIS、Cesium 等。
                  </p>
                </button>

                <!-- 目录 -->
                <button
                  class="group relative flex flex-col gap-2 rounded-xl p-3 border-2 text-left transition-all duration-200"
                  :class="
                    format === 'directory'
                      ? 'border-amber-400 bg-amber-50 shadow-sm shadow-amber-100'
                      : 'border-slate-200 hover:border-slate-300 bg-white'
                  "
                  @click="
                    format = 'directory';
                    destPath = '';
                  "
                >
                  <div class="flex items-center gap-2">
                    <div
                      class="w-7 h-7 rounded-lg flex items-center justify-center transition-colors duration-200"
                      :class="
                        format === 'directory'
                          ? 'bg-amber-100'
                          : 'bg-slate-100 group-hover:bg-slate-200'
                      "
                    >
                      <FolderOpen
                        class="size-3.5 transition-colors duration-200"
                        :class="
                          format === 'directory'
                            ? 'text-amber-600'
                            : 'text-slate-500'
                        "
                      />
                    </div>
                    <Transition name="scale-in">
                      <div
                        v-if="format === 'directory'"
                        class="ml-auto size-4 rounded-full bg-amber-400 flex items-center justify-center shrink-0"
                      >
                        <span class="block size-1.5 rounded-full bg-white" />
                      </div>
                    </Transition>
                  </div>
                  <span
                    class="text-xs font-semibold transition-colors duration-200 leading-tight"
                    :class="
                      format === 'directory'
                        ? 'text-amber-700'
                        : 'text-slate-800'
                    "
                  >
                    目录 z/x/y
                  </span>
                  <p class="text-[10px] leading-relaxed text-slate-400">
                    文件树，可作静态 XYZ 服务。
                  </p>
                </button>

                <!-- GeoTIFF -->
                <button
                  class="group relative flex flex-col gap-2 rounded-xl p-3 border-2 text-left transition-all duration-200"
                  :class="
                    format === 'tiff'
                      ? 'border-emerald-500 bg-emerald-50 shadow-sm shadow-emerald-100'
                      : 'border-slate-200 hover:border-slate-300 bg-white'
                  "
                  @click="
                    format = 'tiff';
                    destPath = '';
                  "
                >
                  <div class="flex items-center gap-2">
                    <div
                      class="w-7 h-7 rounded-lg flex items-center justify-center transition-colors duration-200"
                      :class="
                        format === 'tiff'
                          ? 'bg-emerald-100'
                          : 'bg-slate-100 group-hover:bg-slate-200'
                      "
                    >
                      <Map
                        class="size-3.5 transition-colors duration-200"
                        :class="
                          format === 'tiff'
                            ? 'text-emerald-600'
                            : 'text-slate-500'
                        "
                      />
                    </div>
                    <Transition name="scale-in">
                      <div
                        v-if="format === 'tiff'"
                        class="ml-auto size-4 rounded-full bg-emerald-500 flex items-center justify-center shrink-0"
                      >
                        <span class="block size-1.5 rounded-full bg-white" />
                      </div>
                    </Transition>
                  </div>
                  <span
                    class="text-xs font-semibold transition-colors duration-200 leading-tight"
                    :class="
                      format === 'tiff' ? 'text-emerald-700' : 'text-slate-800'
                    "
                  >
                    GeoTIFF
                  </span>
                  <p class="text-[10px] leading-relaxed text-slate-400">
                    地理参考栅格图，支持 QGIS/ArcGIS。
                  </p>
                </button>
              </div>
            </div>

            <!-- 路径选择 -->
            <div>
              <p
                class="text-xs font-semibold text-slate-500 uppercase tracking-wide mb-2"
              >
                {{ format === "directory" ? "导出目录" : "保存位置" }}
              </p>
              <div class="flex items-center gap-2">
                <div
                  class="flex-1 flex items-center gap-2 rounded-xl px-3 h-9 text-xs border border-slate-200 bg-slate-50 min-w-0 overflow-hidden"
                >
                  <FolderOpen class="size-3.5 shrink-0 text-slate-400" />
                  <span
                    class="truncate"
                    :class="destPath ? 'text-slate-800' : 'text-slate-400'"
                  >
                    {{
                      destPath ||
                      (format === "directory"
                        ? "未选择目录…"
                        : "未选择保存路径…")
                    }}
                  </span>
                </div>
                <button
                  class="h-9 px-4 text-xs font-medium rounded-xl border border-slate-200 bg-white text-slate-700 hover:border-slate-300 hover:bg-slate-50 transition-colors shrink-0"
                  :class="starting ? 'opacity-40 pointer-events-none' : ''"
                  @click="pickPath"
                >
                  浏览…
                </button>
              </div>
            </div>

            <!-- GeoTIFF 拼合层级选择（仅 tiff 格式） -->
            <Transition name="tiff-row">
              <div
                v-if="format === 'tiff'"
                class="flex items-center justify-between gap-3 rounded-xl border p-3"
                style="
                  border-color: var(--color-border-subtle);
                  background: var(--color-surface-raised);
                "
              >
                <div class="flex items-center gap-2 min-w-0">
                  <Layers class="size-3.5 shrink-0 text-slate-400" />
                  <div>
                    <div class="text-xs font-medium text-slate-700">
                      拼合层级
                    </div>
                    <div
                      class="text-[11px] text-slate-400 mt-0.5 leading-tight"
                    >
                      z{{ task.minZoom }}–z{{ task.maxZoom }}
                      可选，层级越高越清晰但文件越大
                    </div>
                  </div>
                </div>
                <!-- 步进器 -->
                <div class="flex items-center gap-1.5 shrink-0">
                  <button
                    type="button"
                    class="size-6 flex items-center justify-center rounded-lg border text-slate-600 hover:bg-slate-100 transition-colors disabled:opacity-30 disabled:pointer-events-none text-sm font-bold leading-none"
                    style="border-color: var(--color-border-subtle)"
                    :disabled="tiffZoom <= task.minZoom || starting"
                    @click="tiffZoom = Math.max(task.minZoom, tiffZoom - 1)"
                  >
                    ‹
                  </button>
                  <span
                    class="min-w-10 text-center text-xs font-semibold text-slate-800 tabular-nums"
                  >
                    z{{ tiffZoom }}
                  </span>
                  <button
                    type="button"
                    class="size-6 flex items-center justify-center rounded-lg border text-slate-600 hover:bg-slate-100 transition-colors disabled:opacity-30 disabled:pointer-events-none text-sm font-bold leading-none"
                    style="border-color: var(--color-border-subtle)"
                    :disabled="tiffZoom >= task.maxZoom || starting"
                    @click="tiffZoom = Math.min(task.maxZoom, tiffZoom + 1)"
                  >
                    ›
                  </button>
                </div>
              </div>
            </Transition>

            <!-- 裁剪选项 -->
            <div
              class="flex items-center justify-between gap-3 rounded-xl border p-3"
              style="
                border-color: var(--color-border-subtle);
                background: var(--color-surface-raised);
              "
            >
              <div class="flex items-center gap-2 min-w-0">
                <Scissors class="size-3.5 shrink-0 text-slate-400" />
                <div>
                  <div class="text-xs font-medium text-slate-700">
                    严格裁剪至任务选框范围
                  </div>
                  <div class="text-[11px] text-slate-400 mt-0.5 leading-tight">
                    {{
                      format === "tiff"
                        ? "将输出像素精确裁剪至任务选框经纬度边界"
                        : "仅导出完全位于任务选框内的瓦片，排除边缘相交瓦片"
                    }}
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

            <!-- 启动错误提示 -->
            <div
              v-if="startError"
              class="flex items-start gap-2.5 rounded-xl px-4 py-3 text-xs bg-red-50 border border-red-200 text-red-600"
            >
              <AlertCircle class="size-4 shrink-0 mt-0.5 text-red-500" />
              <span>{{ startError }}</span>
            </div>
          </div>

          <!-- ── 底部操作栏 ──────────────────────────────────────────── -->
          <div
            class="flex items-center justify-between gap-3 px-6 py-4 border-t bg-slate-50"
            style="border-color: var(--color-border-subtle)"
          >
            <!-- 格式徽章 -->
            <div
              class="flex items-center gap-1.5 text-xs text-slate-500 font-medium"
            >
              <Database
                v-if="format === 'mbtiles'"
                class="size-3.5 text-blue-500"
              />
              <FolderOpen
                v-else-if="format === 'directory'"
                class="size-3.5 text-amber-500"
              />
              <Map v-else class="size-3.5 text-emerald-500" />
              <span>{{ formatLabel[format] }}</span>
            </div>
            <div class="flex items-center gap-2">
              <button
                class="h-8 px-4 text-xs font-medium rounded-xl border border-slate-200 text-slate-600 hover:bg-slate-100 transition-colors"
                @click="emit('close')"
              >
                取消
              </button>
              <button
                class="h-8 px-5 text-xs font-semibold rounded-xl flex items-center gap-1.5 transition-all duration-200"
                :class="
                  canExport
                    ? 'bg-blue-600 hover:bg-blue-500 active:bg-blue-700 text-white shadow-sm'
                    : 'bg-slate-200 text-slate-400 cursor-not-allowed'
                "
                :disabled="!canExport"
                @click="doExport"
              >
                <Loader2 v-if="starting" class="size-3.5 animate-spin" />
                <PackageCheck v-else class="size-3.5" />
                {{ starting ? "导出中…" : "开始导出" }}
              </button>
            </div>
          </div>
        </div>
      </Transition>
    </div>
  </Transition>
</template>

<style scoped>
.scale-in-enter-active,
.scale-in-leave-active {
  transition:
    opacity 0.15s ease,
    transform 0.15s ease;
}
.scale-in-enter-from,
.scale-in-leave-to {
  opacity: 0;
  transform: scale(0.5);
}

.fade-slide-up-enter-active,
.fade-slide-up-leave-active {
  transition:
    opacity 0.25s ease,
    transform 0.25s ease;
}
.fade-slide-up-enter-from,
.fade-slide-up-leave-to {
  opacity: 0;
  transform: translateY(6px);
}

.tiff-row-enter-active,
.tiff-row-leave-active {
  transition:
    opacity 0.2s ease,
    max-height 0.2s ease;
  max-height: 80px;
  overflow: hidden;
}
.tiff-row-enter-from,
.tiff-row-leave-to {
  opacity: 0;
  max-height: 0;
}
</style>
