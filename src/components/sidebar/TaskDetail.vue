<script setup lang="ts">
import { ref, computed, watch, onMounted, onUnmounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  MapPin,
  ArrowLeft,
  RefreshCw,
  Trash2,
  Loader,
  PackageOpen,
  PackageCheck,
  Pause,
  Play,
  FolderOpen,
  Expand,
  Download,
  X,
} from "lucide-vue-next";
import { useI18n } from "vue-i18n";
import ExportDialog from "./ExportDialog.vue";
import MiniMapPreview from "./MiniMapPreview.vue";
import { useTaskDetail } from "~/composables/useTaskDetail";
import { useExportJobs } from "~/composables/useExportJobs";
import { useExtendMode } from "~/composables/useExtendMode";
import { useFailedTilesView } from "~/composables/useFailedTilesView";

// ─── 类型 ──────────────────────────────────────────────────────────────────
interface BackendTask {
  id: string;
  name: string;
  sourceConfig: string;
  status: string;
  boundsWest: number;
  boundsEast: number;
  boundsSouth: number;
  boundsNorth: number;
  minZoom: number;
  maxZoom: number;
  totalTiles: number;
  downloadedTiles: number;
  failedTiles: number;
  tileStorePath: string | null;
  polygonWgs84: string | null;
  createdAt: string;
  updatedAt: string;
}

interface LogEntry {
  id: number;
  taskId: string | null;
  level: string;
  message: string;
  timestamp: string;
}

const props = defineProps<{
  taskId: string;
}>();

const emit = defineEmits<{
  close: [];
  deleted: [id: string];
  flyTo: [bounds: { west: number; east: number; south: number; north: number }];
}>();

// ─── 数据 ──────────────────────────────────────────────────────────────────
const task = ref<BackendTask | null>(null);
const logs = ref<LogEntry[]>([]);
const loadingLogs = ref(false);
const deleting = ref(false);
const pausing = ref(false);
const showDeleteConfirm = ref(false);

// 实时速度（来自 tilegrab-progress 事件）
const liveSpeedTiles = ref(0); // 瓦片/秒
const liveSpeedMb = ref(0); // MB/秒

// 裁剪进度（来自 tilegrab-progress 事件，status=processing）
const liveClipDone = ref(0);
const liveClipTotal = ref(0);

const { t } = useI18n();
const { selectedTaskStatus } = useTaskDetail();
const { getActiveJobForTask, getHistoryForTask, revealInExplorer } =
  useExportJobs();

const activeExportJob = computed(() => getActiveJobForTask(props.taskId));
const exportHistory = computed(() => getHistoryForTask(props.taskId));

const exportFormatLabel = computed(
  (): Record<string, string> => ({
    mbtiles: t("taskDetail.exportFormat.mbtiles"),
    directory: t("taskDetail.exportFormat.directory"),
    tiff: t("taskDetail.exportFormat.tiff"),
  }),
);

function fmtExportCount(n: number) {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(n);
}

function fmtTime(d: Date) {
  return d.toLocaleString("zh-CN", { hour12: false });
}

async function loadTask() {
  try {
    task.value = await invoke<BackendTask>("get_task", {
      taskId: props.taskId,
    });
    selectedTaskStatus.value = task.value?.status ?? null;
    await loadFailedSummary();
  } catch (e) {
    console.error("[TaskDetail] get_task failed:", e);
  }
}

async function loadLogs() {
  loadingLogs.value = true;
  try {
    logs.value = await invoke<LogEntry[]>("get_task_logs", {
      taskId: props.taskId,
      limit: 100,
    });
  } catch (e) {
    console.error("[TaskDetail] get_task_logs failed:", e);
  } finally {
    loadingLogs.value = false;
  }
}

watch(
  () => props.taskId,
  async () => {
    await loadTask();
    await loadLogs();
  },
  { immediate: true },
);

// 下载中轮询进度
let pollTimer: ReturnType<typeof setInterval> | null = null;
let unlistenProgress: UnlistenFn | null = null;

watch(
  () => task.value?.status,
  (status) => {
    pausing.value = false;
    if (status === "downloading" || status === "processing") {
      if (!pollTimer) {
        pollTimer = setInterval(() => {
          loadTask();
        }, 1500);
      }
    } else {
      if (pollTimer) {
        clearInterval(pollTimer);
        pollTimer = null;
      }
      // 状态不再是下载/裁剪中，清除速度显示
      liveSpeedTiles.value = 0;
      liveSpeedMb.value = 0;
      liveClipDone.value = 0;
      liveClipTotal.value = 0;
    }
  },
);

onMounted(async () => {
  unlistenProgress = await listen<{
    task_id: string;
    speed: number;
    bytes_per_sec: number;
    eta_secs: number | null;
    status: string;
  }>("tilegrab-progress", (event) => {
    const p = event.payload;
    if (p.task_id !== props.taskId) return;
    if (p.status === "downloading") {
      liveSpeedTiles.value = p.speed;
      liveSpeedMb.value = p.bytes_per_sec / (1024 * 1024);
    } else if (p.status === "processing") {
      liveSpeedTiles.value = 0;
      liveSpeedMb.value = 0;
      liveClipDone.value = (event.payload as any).downloaded ?? 0;
      liveClipTotal.value = (event.payload as any).total ?? 0;
      loadTask();
    } else {
      liveSpeedTiles.value = 0;
      liveSpeedMb.value = 0;
      liveClipDone.value = 0;
      liveClipTotal.value = 0;
      loadTask();
    }
  });
});

onUnmounted(() => {
  if (pollTimer) {
    clearInterval(pollTimer);
    pollTimer = null;
  }
  unlistenProgress?.();
  // 切换/卸载任务时清理失败瓦片图层
  hideFailedView();
});

// ─── 操作 ──────────────────────────────────────────────────────────────────
async function retryFailed() {
  await invoke("retry_failed", { taskId: props.taskId }).catch(console.error);
  await loadTask();
  // 重试后清掉失败可视化层
  hideFailedView();
  failedSummary.value = [];
}

// ─── E 套件：失败瓦片可视化 ────────────────────────────────────────────────
interface FailedZoomStat {
  zoom: number;
  count: number;
}
const failedSummary = ref<FailedZoomStat[]>([]);
const failedSelected = ref<Set<number>>(new Set());
const failedBatchBusy = ref(false);
const {
  view: failedView,
  show: showFailedView,
  hide: hideFailedView,
} = useFailedTilesView();

const failedSelectedTotal = computed(() => {
  let total = 0;
  for (const row of failedSummary.value) {
    if (failedSelected.value.has(row.zoom)) total += row.count;
  }
  return total;
});

async function loadFailedSummary() {
  if (!task.value || task.value.failedTiles <= 0) {
    failedSummary.value = [];
    failedSelected.value = new Set();
    return;
  }
  try {
    failedSummary.value = await invoke<FailedZoomStat[]>(
      "failed_tiles_summary",
      { taskId: props.taskId },
    );
    // 修剪已不存在的层级
    const valid = new Set(failedSummary.value.map((r) => r.zoom));
    const next = new Set<number>();
    for (const z of failedSelected.value) if (valid.has(z)) next.add(z);
    failedSelected.value = next;
  } catch (e) {
    console.error("[TaskDetail] failed_tiles_summary failed:", e);
    failedSummary.value = [];
    failedSelected.value = new Set();
  }
}

function toggleFailedZoom(zoom: number) {
  if (
    failedView.value?.taskId === props.taskId &&
    failedView.value.zoom === zoom
  ) {
    hideFailedView();
  } else {
    showFailedView(props.taskId, zoom);
  }
}

function toggleFailedZoomSelected(zoom: number) {
  const s = new Set(failedSelected.value);
  if (s.has(zoom)) s.delete(zoom);
  else s.add(zoom);
  failedSelected.value = s;
}

function toggleFailedSelectAll() {
  if (failedSelected.value.size === failedSummary.value.length) {
    failedSelected.value = new Set();
  } else {
    failedSelected.value = new Set(failedSummary.value.map((r) => r.zoom));
  }
}

async function retryFailedAtZoom(zoom: number) {
  await invoke("retry_failed_tiles", {
    taskId: props.taskId,
    zoom,
  }).catch(console.error);
  hideFailedView();
  await loadTask();
  await loadFailedSummary();
}

async function retryFailedSelected() {
  if (failedSelected.value.size === 0 || failedBatchBusy.value) return;
  failedBatchBusy.value = true;
  try {
    // 全选时一次性 retry_failed_tiles(zoom=undefined) 重置所有
    if (failedSelected.value.size === failedSummary.value.length) {
      await invoke("retry_failed_tiles", { taskId: props.taskId }).catch(
        console.error,
      );
    } else {
      // 部分选中：每层单独重置（数量通常 <10）
      for (const z of failedSelected.value) {
        await invoke("retry_failed_tiles", {
          taskId: props.taskId,
          zoom: z,
        }).catch(console.error);
      }
    }
    hideFailedView();
    await loadTask();
    await loadFailedSummary();
  } finally {
    failedBatchBusy.value = false;
  }
}

// ─── F 套件：增量下载 ──────────────────────────────────────────────────────
type IncrementalDialog = null | "import";
const incrementalDialog = ref<IncrementalDialog>(null);
const incrementalBusy = ref(false);
const incrementalMessage = ref<string>("");

// 任务是否已完成（用于显示/隐藏增量卡片）
const isCompleted = computed(
  () =>
    !!task.value &&
    task.value.totalTiles > 0 &&
    task.value.downloadedTiles >= task.value.totalTiles &&
    task.value.status !== "downloading" &&
    task.value.status !== "processing",
);

// F2 — 扩展任务（地图绘制）
const { start: startExtendMode } = useExtendMode();
function startExtend() {
  if (!task.value) return;
  startExtendMode({
    taskId: task.value.id,
    taskName: task.value.name,
    originalBounds: {
      west: task.value.boundsWest,
      east: task.value.boundsEast,
      south: task.value.boundsSouth,
      north: task.value.boundsNorth,
    },
    originalPolygon: task.value.polygonWgs84
      ? (JSON.parse(task.value.polygonWgs84) as [number, number][])
      : null,
    originalMinZoom: task.value.minZoom,
    originalMaxZoom: task.value.maxZoom,
  });
  // 关闭详情面板，让用户专注地图操作
  emit("close");
}

// F4 — 从其他任务导入
const importCandidates = ref<BackendTask[]>([]);
const importSourceId = ref<string | null>(null);
const importReusableCount = ref<number | null>(null);
const importCountLoading = ref(false);
const selectedCandidate = computed<BackendTask | null>(
  () =>
    importCandidates.value.find((c) => c.id === importSourceId.value) ?? null,
);
async function openImportDialog() {
  if (!task.value) return;
  incrementalMessage.value = "";
  importSourceId.value = null;
  importReusableCount.value = null;
  // 列出所有任务，前端筛选同源 + 已完成
  try {
    const all = await invoke<BackendTask[]>("list_tasks");
    const me = task.value;
    const myCfg = JSON.parse(me.sourceConfig);
    importCandidates.value = all.filter((t) => {
      if (t.id === me.id) return false;
      if (!t.tileStorePath) return false;
      // 严格只挑已完成的任务作为借数据源
      if (t.totalTiles <= 0 || t.downloadedTiles < t.totalTiles) return false;
      try {
        const c = JSON.parse(t.sourceConfig);
        return (
          c.urlTemplate === myCfg.urlTemplate &&
          c.format === myCfg.format &&
          c.crs === myCfg.crs &&
          c.coordType === myCfg.coordType
        );
      } catch {
        return false;
      }
    });
  } catch (e) {
    console.error(e);
    importCandidates.value = [];
  }
  incrementalDialog.value = "import";
}
async function previewImport() {
  if (!importSourceId.value) return;
  importCountLoading.value = true;
  importReusableCount.value = null;
  try {
    importReusableCount.value = await invoke<number>("count_reusable_tiles", {
      targetTaskId: props.taskId,
      sourceTaskId: importSourceId.value,
    });
  } catch (e: any) {
    incrementalMessage.value = String(e);
  } finally {
    importCountLoading.value = false;
  }
}
async function runImport() {
  if (!importSourceId.value) return;
  incrementalBusy.value = true;
  incrementalMessage.value = "";
  try {
    const [imported, pendingBefore] = await invoke<[number, number]>(
      "import_tiles_from_source",
      {
        targetTaskId: props.taskId,
        sourceTaskId: importSourceId.value,
      },
    );
    incrementalMessage.value = t("taskDetail.incremental.importedHint", {
      imported,
      pendingBefore,
    });
    await loadTask();
  } catch (e: any) {
    incrementalMessage.value = String(e);
  } finally {
    incrementalBusy.value = false;
  }
}

async function pauseTask() {
  pausing.value = true;
  await invoke("pause_download", { taskId: props.taskId }).catch(console.error);
  await loadTask();
  pausing.value = false; // 无论状态是否立即变化，总是清除加载态
}

async function resumeTask() {
  await invoke("resume_download", { taskId: props.taskId }).catch(
    console.error,
  );
  await loadTask();
}
async function deleteTask() {
  deleting.value = true;
  showDeleteConfirm.value = false;
  try {
    await invoke("delete_task", { taskId: props.taskId });
    emit("deleted", props.taskId);
    emit("close");
  } catch (e) {
    console.error("[TaskDetail] delete_task failed:", e);
  } finally {
    deleting.value = false;
  }
}

// ─── 导出 ──────────────────────────────────────────────────────────────────
const showExport = ref(false);

function openExport() {
  showExport.value = true;
}

function flyToTask() {
  if (!task.value) return;
  emit("flyTo", {
    west: task.value.boundsWest,
    east: task.value.boundsEast,
    south: task.value.boundsSouth,
    north: task.value.boundsNorth,
  });
}

// ─── 样式工具 ──────────────────────────────────────────────────────────────
const statusMap = computed(
  (): Record<string, { label: string; cls: string }> => ({
    pending: {
      label: t("taskDetail.status.pending"),
      cls: "bg-slate-100 text-slate-500",
    },
    downloading: {
      label: t("taskDetail.status.downloading"),
      cls: "bg-blue-50 text-blue-700",
    },
    processing: {
      label: t("taskDetail.status.processing"),
      cls: "bg-purple-50 text-purple-700",
    },
    paused: {
      label: t("taskDetail.status.paused"),
      cls: "bg-amber-50 text-amber-700",
    },
    completed: {
      label: t("taskDetail.status.completed"),
      cls: "bg-green-50 text-green-700",
    },
    completed_with_errors: {
      label: t("taskDetail.status.completed_with_errors"),
      cls: "bg-orange-50 text-orange-700",
    },
    failed: {
      label: t("taskDetail.status.failed"),
      cls: "bg-red-50 text-red-700",
    },
    cancelled: {
      label: t("taskDetail.status.cancelled"),
      cls: "bg-slate-100 text-slate-500",
    },
  }),
);

const logLevelMap: Record<string, string> = {
  info: "text-slate-500",
  warn: "text-amber-600",
  error: "text-red-600",
};

function statusInfo(s: string) {
  return statusMap.value[s] ?? { label: s, cls: "bg-slate-100 text-slate-500" };
}

function percent(t: BackendTask) {
  if (!t.totalTiles) return 0;
  return Math.min(100, Math.floor((t.downloadedTiles / t.totalTiles) * 100));
}

function formatDate(iso: string) {
  return new Date(iso).toLocaleString("zh-CN", { hour12: false });
}

function formatCount(n: number) {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(n);
}

function logTime(iso: string) {
  return new Date(iso).toLocaleTimeString("zh-CN", { hour12: false });
}
</script>

<template>
  <!-- 任务详情面板：覆盖任务列表，由父级 absolute inset-0 定位 -->
  <div
    class="h-full flex flex-col overflow-hidden border-r"
    style="
      background: var(--color-app-bg);
      border-color: var(--color-border-subtle);
    "
  >
    <!-- 头部 -->
    <div
      class="flex items-center justify-between px-3 py-2.5 border-b shrink-0"
      style="border-color: var(--color-border-subtle)"
    >
      <!-- 返回按钮 -->
      <button
        class="flex items-center gap-1.5 text-xs px-2 py-1 rounded-md transition-colors"
        style="color: var(--color-text-secondary)"
        @click="emit('close')"
      >
        <ArrowLeft class="size-3.5" />
        <span>{{ t("taskDetail.back") }}</span>
      </button>
      <!-- 地图定位 -->
      <button
        v-if="task"
        :title="t('taskDetail.locateOnMap')"
        class="flex items-center justify-center size-7 rounded-md transition-colors"
        style="color: var(--color-text-secondary)"
        @click="flyToTask"
      >
        <MapPin class="size-4" />
      </button>
    </div>

    <div v-if="task" class="flex-1 overflow-y-auto px-4 py-3 space-y-4">
      <!-- 任务名称 + 状态 -->
      <div>
        <p
          class="text-sm font-semibold mb-1"
          style="color: var(--color-text-primary)"
        >
          {{ task.name }}
        </p>
        <span
          class="inline-flex items-center px-2 py-0.5 rounded-full text-[11px] font-medium"
          :class="statusInfo(task.status).cls"
        >
          {{ statusInfo(task.status).label }}
        </span>
      </div>

      <!-- 进度统计 -->
      <div
        class="rounded-lg p-3 space-y-2"
        style="background: var(--color-surface)"
      >
        <div class="h-1.5 w-full rounded-full overflow-hidden bg-slate-100">
          <div
            class="h-full rounded-full bg-blue-500 transition-all"
            :style="{ width: `${percent(task)}%` }"
          />
        </div>
        <div class="grid grid-cols-3 gap-2 text-center">
          <div>
            <p class="text-[11px]" style="color: var(--color-text-muted)">
              {{ t("taskDetail.downloaded") }}
            </p>
            <p
              class="text-sm font-semibold"
              style="color: var(--color-text-primary)"
            >
              {{ formatCount(task.downloadedTiles) }}
            </p>
          </div>
          <div>
            <p class="text-[11px]" style="color: var(--color-text-muted)">
              {{ t("taskDetail.failed") }}
            </p>
            <p
              class="text-sm font-semibold"
              :class="task.failedTiles > 0 ? 'text-red-500' : ''"
            >
              {{ task.failedTiles }}
            </p>
          </div>
          <div>
            <p class="text-[11px]" style="color: var(--color-text-muted)">
              {{ t("taskDetail.total") }}
            </p>
            <p
              class="text-sm font-semibold"
              style="color: var(--color-text-primary)"
            >
              {{ formatCount(task.totalTiles) }}
            </p>
          </div>
        </div>

        <!-- 实时速度（仅下载中且有数据时） -->
        <div
          v-if="task.status === 'downloading' && liveSpeedTiles > 0"
          class="flex items-center justify-between pt-1.5 border-t text-xs"
          style="
            border-color: var(--color-border-subtle);
            color: var(--color-text-secondary);
          "
        >
          <span class="text-blue-600 font-medium">
            {{
              liveSpeedTiles < 1
                ? liveSpeedTiles.toFixed(1)
                : Math.round(liveSpeedTiles)
            }}
            {{ t("taskDetail.tilesPerSec") }}
          </span>
          <span>
            {{
              liveSpeedMb >= 0.1
                ? liveSpeedMb.toFixed(2) + " MB/s"
                : (liveSpeedMb * 1024).toFixed(0) + " KB/s"
            }}
          </span>
        </div>
      </div>

      <!-- 导出进度（有活跃导出任务时） -->
      <div
        v-if="activeExportJob"
        class="rounded-lg p-3 space-y-2 border"
        style="background: var(--color-surface); border-color: #c7d2fe"
      >
        <div class="flex items-center justify-between text-xs">
          <span class="flex items-center gap-1.5 font-medium text-indigo-600">
            <PackageCheck class="size-3.5" />
            {{
              t("taskDetail.exporting", {
                format:
                  exportFormatLabel[activeExportJob.format] ??
                  activeExportJob.format,
              })
            }}
          </span>
          <span
            v-if="activeExportJob.total > 0"
            class="font-mono tabular-nums"
            style="color: var(--color-text-muted)"
          >
            {{ fmtExportCount(activeExportJob.done) }} /
            {{ fmtExportCount(activeExportJob.total) }}
          </span>
        </div>
        <div class="h-1.5 w-full rounded-full overflow-hidden bg-slate-100">
          <div
            class="h-full rounded-full transition-all duration-300"
            :class="
              activeExportJob.total > 0
                ? 'bg-indigo-500'
                : 'bg-indigo-400 animate-pulse'
            "
            :style="{
              width:
                activeExportJob.total > 0
                  ? `${Math.min(100, Math.round((activeExportJob.done / activeExportJob.total) * 100))}%`
                  : '30%',
            }"
          />
        </div>
        <p class="text-[11px]" style="color: var(--color-text-muted)">
          {{ activeExportJob.destPath }}
        </p>
      </div>

      <!-- 导出记录 -->
      <div v-if="exportHistory.length > 0">
        <p
          class="text-xs font-semibold mb-2"
          style="color: var(--color-text-primary)"
        >
          {{ t("taskDetail.exportHistory") }}
        </p>
        <div class="space-y-2">
          <div
            v-for="rec in exportHistory"
            :key="rec.jobId"
            class="rounded-lg p-2.5 border flex flex-col gap-1.5"
            :style="
              rec.status === 'error'
                ? 'background: #fff5f5; border-color: #fed7d7'
                : 'background: var(--color-surface); border-color: var(--color-border-subtle)'
            "
          >
            <div class="flex items-center justify-between gap-2">
              <span
                class="text-[11px] font-semibold px-1.5 py-0.5 rounded"
                :style="
                  rec.status === 'error'
                    ? 'background:#fed7d7;color:#c53030'
                    : 'background:#e0e7ff;color:#3730a3'
                "
              >
                {{ exportFormatLabel[rec.format] ?? rec.format }}
              </span>
              <span
                v-if="rec.status === 'done'"
                class="text-[11px]"
                style="color: var(--color-text-muted)"
              >
                {{ fmtExportCount(rec.done) }} 块
              </span>
              <span
                v-else-if="rec.status === 'error'"
                class="text-[11px] text-red-500"
                >{{ t("taskDetail.failed") }}</span
              >
            </div>
            <p
              class="text-[11px] truncate font-mono"
              style="color: var(--color-text-secondary)"
            >
              {{ rec.destPath }}
            </p>
            <div class="flex items-center justify-between">
              <span class="text-[10px]" style="color: var(--color-text-muted)">
                {{ rec.finishedAt ? fmtTime(rec.finishedAt) : "" }}
              </span>
              <button
                v-if="rec.status === 'done'"
                class="flex items-center gap-1 text-[11px] font-medium px-2 py-0.5 rounded border transition-colors hover:bg-indigo-50"
                style="color: #4338ca; border-color: #c7d2fe"
                @click="revealInExplorer(rec.destPath)"
              >
                <FolderOpen class="size-3" />
                {{ t("taskDetail.openLocation") }}
              </button>
              <p
                v-else-if="rec.error"
                class="text-[11px] text-red-500 truncate max-w-35"
              >
                {{ rec.error }}
              </p>
            </div>
          </div>
        </div>
      </div>

      <!-- 元信息 -->
      <dl class="space-y-1.5 text-xs">
        <div class="flex justify-between">
          <dt style="color: var(--color-text-muted)">
            {{ t("taskDetail.zoomRange") }}
          </dt>
          <dd class="font-mono" style="color: var(--color-text-primary)">
            z{{ task.minZoom }} – z{{ task.maxZoom }}
          </dd>
        </div>
        <div class="flex justify-between">
          <dt style="color: var(--color-text-muted)">
            {{ t("taskDetail.bbox") }}
          </dt>
          <dd
            class="font-mono text-[10px]"
            style="color: var(--color-text-primary)"
          >
            {{ task.boundsWest.toFixed(4) }},{{ task.boundsSouth.toFixed(4) }}
            →
            {{ task.boundsEast.toFixed(4) }},{{ task.boundsNorth.toFixed(4) }}
          </dd>
        </div>
        <div class="flex justify-between">
          <dt style="color: var(--color-text-muted)">
            {{ t("taskDetail.createdAt") }}
          </dt>
          <dd style="color: var(--color-text-primary)">
            {{ formatDate(task.createdAt) }}
          </dd>
        </div>
        <div class="flex justify-between">
          <dt style="color: var(--color-text-muted)">
            {{ t("taskDetail.updatedAt") }}
          </dt>
          <dd style="color: var(--color-text-primary)">
            {{ formatDate(task.updatedAt) }}
          </dd>
        </div>
      </dl>

      <!-- 操作按钮 -->
      <div class="flex flex-col gap-2">
        <!-- 裁剪中：只显示进度，不可操作 -->
        <template v-if="task.status === 'processing'">
          <div
            class="w-full rounded-lg p-3 space-y-2"
            style="background: var(--color-surface)"
          >
            <div class="flex items-center justify-between text-xs">
              <span
                class="flex items-center gap-1.5 text-purple-600 font-medium"
              >
                <Loader class="size-3.5 animate-spin" />
                {{ t("taskDetail.clipping") }}
              </span>
              <span
                v-if="liveClipTotal > 0"
                class="font-mono tabular-nums"
                style="color: var(--color-text-muted)"
              >
                {{ formatCount(liveClipDone) }} /
                {{ formatCount(liveClipTotal) }}
              </span>
            </div>
            <div class="h-1.5 w-full rounded-full overflow-hidden bg-slate-100">
              <div
                class="h-full rounded-full bg-purple-500 transition-all duration-300"
                :style="{
                  width:
                    liveClipTotal > 0
                      ? `${Math.min(100, Math.round((liveClipDone / liveClipTotal) * 100))}%`
                      : '0%',
                }"
              />
            </div>
          </div>
        </template>

        <!-- 下载中：只显示暂停 -->
        <template v-else-if="task.status === 'downloading'">
          <button
            class="w-full flex items-center justify-center gap-1.5 rounded-lg py-1.5 text-xs font-medium border transition-colors"
            :class="
              pausing
                ? 'opacity-60 cursor-not-allowed border-slate-200 bg-slate-50 text-slate-400'
                : 'border-amber-300 bg-amber-50 text-amber-700 hover:bg-amber-100'
            "
            :disabled="pausing"
            @click="pauseTask"
          >
            <Loader v-if="pausing" class="size-3.5 animate-spin" />
            <Pause v-else class="size-3.5" />
            {{
              pausing ? t("taskDetail.pausing") : t("taskDetail.pauseDownload")
            }}
          </button>
        </template>

        <!-- 已暂停：继续 + 导出 + 删除 -->
        <template v-else-if="task.status === 'paused'">
          <button
            class="w-full flex items-center justify-center gap-1.5 rounded-lg py-1.5 text-xs font-medium border border-blue-300 bg-blue-50 text-blue-700 hover:bg-blue-100 transition-colors"
            @click="resumeTask"
          >
            <Play class="size-3.5" />
            {{ t("taskDetail.resumeDownload") }}
          </button>
          <button
            v-if="task.downloadedTiles > 0"
            class="w-full flex items-center justify-center gap-1.5 rounded-lg py-1.5 text-xs font-medium border border-blue-200 bg-white text-blue-600 hover:bg-blue-50 transition-colors"
            @click="openExport"
          >
            <PackageOpen class="size-3.5" />
            {{ t("taskDetail.exportTiles") }}
          </button>
          <!-- 删除确认 -->
          <div
            v-if="showDeleteConfirm"
            class="w-full rounded-lg border border-red-200 bg-red-50 px-3 py-2.5 flex flex-col gap-2"
          >
            <p class="text-xs text-red-600 font-medium">
              {{ t("taskDetail.confirmDelete") }}
            </p>
            <div class="flex gap-2">
              <button
                class="flex-1 py-1 rounded-md text-xs font-medium bg-red-500 text-white hover:bg-red-600 disabled:opacity-50 transition-colors"
                :disabled="deleting"
                @click="deleteTask"
              >
                {{
                  deleting
                    ? t("taskDetail.deleting")
                    : t("taskDetail.confirmDeleteBtn")
                }}
              </button>
              <button
                class="flex-1 py-1 rounded-md text-xs font-medium border border-slate-200 text-slate-600 hover:bg-slate-100 transition-colors"
                @click="showDeleteConfirm = false"
              >
                {{ t("taskDetail.cancelDelete") }}
              </button>
            </div>
          </div>
          <button
            v-else
            class="w-full flex items-center justify-center gap-1.5 rounded-lg py-1.5 text-xs font-medium border border-red-200 bg-white text-red-500 hover:bg-red-50 transition-colors"
            @click="showDeleteConfirm = true"
          >
            <Trash2 class="size-3.5" />
            {{ t("taskDetail.deleteTask") }}
          </button>
        </template>

        <!-- 其他状态（完成/失败/等待）：重试 + 导出 + 删除 -->
        <template v-else>
          <button
            v-if="task.failedTiles > 0"
            class="w-full flex items-center justify-center gap-1.5 rounded-lg py-1.5 text-xs font-medium border border-amber-300 bg-amber-50 text-amber-700 hover:bg-amber-100 transition-colors"
            @click="retryFailed"
          >
            <RefreshCw class="size-3.5" />
            {{ t("taskDetail.retryFailed", { count: task.failedTiles }) }}
          </button>
          <button
            v-if="task.downloadedTiles > 0"
            class="w-full flex items-center justify-center gap-1.5 rounded-lg py-1.5 text-xs font-medium border border-blue-200 bg-blue-50 text-blue-700 hover:bg-blue-100 transition-colors"
            @click="openExport"
          >
            <PackageOpen class="size-3.5" />
            {{ t("taskDetail.exportTiles") }}
          </button>
          <!-- 删除确认 -->
          <div
            v-if="showDeleteConfirm"
            class="w-full rounded-lg border border-red-200 bg-red-50 px-3 py-2.5 flex flex-col gap-2"
          >
            <p class="text-xs text-red-600 font-medium">
              {{ t("taskDetail.confirmDelete") }}
            </p>
            <div class="flex gap-2">
              <button
                class="flex-1 py-1 rounded-md text-xs font-medium bg-red-500 text-white hover:bg-red-600 disabled:opacity-50 transition-colors"
                :disabled="deleting"
                @click="deleteTask"
              >
                {{
                  deleting
                    ? t("taskDetail.deleting")
                    : t("taskDetail.confirmDeleteBtn")
                }}
              </button>
              <button
                class="flex-1 py-1 rounded-md text-xs font-medium border border-slate-200 text-slate-600 hover:bg-slate-100 transition-colors"
                @click="showDeleteConfirm = false"
              >
                {{ t("taskDetail.cancelDelete") }}
              </button>
            </div>
          </div>
          <button
            v-else
            class="w-full flex items-center justify-center gap-1.5 rounded-lg py-1.5 text-xs font-medium border border-red-200 bg-white text-red-500 hover:bg-red-50 transition-colors"
            @click="showDeleteConfirm = true"
          >
            <Trash2 class="size-3.5" />
            {{ t("taskDetail.deleteTask") }}
          </button>
        </template>
      </div>

      <!-- E 套件：失败瓦片可视化（仅已完成任务可见，避免下载中误干扰） -->
      <div
        v-if="
          (task.status === 'completed' || task.status === 'failed') &&
          task.failedTiles > 0 &&
          failedSummary.length > 0
        "
        class="rounded-lg border px-3 py-2.5 flex flex-col gap-2"
        style="
          border-color: var(--color-border);
          background: var(--color-surface);
        "
      >
        <div class="flex items-center justify-between">
          <p
            class="text-xs font-semibold"
            style="color: var(--color-text-primary)"
          >
            {{ t("taskDetail.failedView.title") }}
          </p>
          <span
            v-if="failedView && failedView.taskId === task.id"
            class="text-[10px] px-1.5 py-0.5 rounded bg-red-50 text-red-600 border border-red-200"
          >
            z{{ failedView.zoom }} {{ t("taskDetail.failedView.previewing") }}
          </span>
        </div>
        <p class="text-[11px]" style="color: var(--color-text-muted)">
          {{ t("taskDetail.failedView.hint") }}
        </p>

        <!-- 全选 + 批量重试栏 -->
        <div class="flex items-center justify-between gap-2 px-1">
          <label
            class="flex items-center gap-1.5 text-[11px] cursor-pointer"
            style="color: var(--color-text-primary)"
          >
            <input
              type="checkbox"
              class="size-3 cursor-pointer"
              :checked="
                failedSelected.size === failedSummary.length &&
                failedSummary.length > 0
              "
              :indeterminate.prop="
                failedSelected.size > 0 &&
                failedSelected.size < failedSummary.length
              "
              @change="toggleFailedSelectAll"
            />
            <span>{{ t("taskDetail.failedView.selectAll") }}</span>
          </label>
          <button
            class="text-[11px] px-2 py-0.5 rounded bg-amber-500 text-white hover:bg-amber-600 disabled:opacity-50 transition-colors"
            :disabled="failedSelected.size === 0 || failedBatchBusy"
            @click="retryFailedSelected"
          >
            {{
              failedBatchBusy
                ? t("taskDetail.failedView.retrying")
                : t("taskDetail.failedView.retrySelected", {
                    count: failedSelectedTotal,
                  })
            }}
          </button>
        </div>

        <ul class="flex flex-col gap-1">
          <li
            v-for="row in failedSummary"
            :key="row.zoom"
            class="flex items-center justify-between gap-2 rounded px-2 py-1 border"
            :class="
              failedView &&
              failedView.taskId === task.id &&
              failedView.zoom === row.zoom
                ? 'border-red-300 bg-red-50'
                : 'border-slate-200 bg-white'
            "
          >
            <input
              type="checkbox"
              class="size-3 cursor-pointer"
              :checked="failedSelected.has(row.zoom)"
              @change="toggleFailedZoomSelected(row.zoom)"
            />
            <button
              class="flex items-center gap-2 flex-1 text-left"
              @click="toggleFailedZoom(row.zoom)"
            >
              <span
                class="font-mono text-xs"
                style="color: var(--color-text-primary)"
              >
                z{{ row.zoom }}
              </span>
              <span
                class="px-1.5 py-0.5 rounded-full bg-red-500 text-white text-[10px] font-semibold"
              >
                {{ row.count }}
              </span>
            </button>
            <button
              class="text-[11px] px-2 py-0.5 rounded border border-amber-300 bg-amber-50 text-amber-700 hover:bg-amber-100 transition-colors"
              @click="retryFailedAtZoom(row.zoom)"
            >
              {{ t("taskDetail.failedView.retryThisLevel") }}
            </button>
          </li>
        </ul>
      </div>

      <!-- F 套件：增量更新入口（仅当任务已完成时显示） -->
      <div
        v-if="isCompleted"
        class="rounded-lg border px-3 py-2.5 flex flex-col gap-2"
        style="
          border-color: var(--color-border);
          background: var(--color-surface);
        "
      >
        <p
          class="text-xs font-semibold"
          style="color: var(--color-text-primary)"
        >
          {{ t("taskDetail.incremental.title") }}
        </p>
        <div class="grid grid-cols-2 gap-1.5">
          <button
            class="flex flex-col items-center gap-1 rounded-md border border-slate-200 bg-white px-2 py-1.5 text-[11px] text-slate-600 hover:bg-slate-50 transition-colors"
            @click="startExtend"
          >
            <Expand class="size-3.5" />
            <span>{{ t("taskDetail.incremental.extend") }}</span>
          </button>
          <button
            class="flex flex-col items-center gap-1 rounded-md border border-slate-200 bg-white px-2 py-1.5 text-[11px] text-slate-600 hover:bg-slate-50 transition-colors"
            @click="openImportDialog"
          >
            <Download class="size-3.5" />
            <span>{{ t("taskDetail.incremental.import") }}</span>
          </button>
        </div>
      </div>

      <!-- F 套件：导入弹层 -->
      <div
        v-if="incrementalDialog === 'import'"
        class="fixed inset-0 z-50 flex items-center justify-center bg-black/40"
        @click.self="incrementalDialog = null"
      >
        <div
          class="w-[560px] max-w-[92vw] rounded-xl border bg-white p-4 shadow-xl flex flex-col gap-3"
          style="border-color: var(--color-border)"
        >
          <div class="flex items-center justify-between">
            <p
              class="text-sm font-semibold"
              style="color: var(--color-text-primary)"
            >
              {{ t("taskDetail.incremental.importTitle") }}
            </p>
            <button
              class="p-1 rounded hover:bg-slate-100"
              @click="incrementalDialog = null"
            >
              <X class="size-4" />
            </button>
          </div>

          <p
            class="text-xs leading-relaxed"
            style="color: var(--color-text-muted)"
          >
            {{ t("taskDetail.incremental.importDesc") }}
          </p>
          <p
            v-if="importCandidates.length === 0"
            class="text-xs italic"
            style="color: var(--color-text-muted)"
          >
            {{ t("taskDetail.incremental.importNoneFound") }}
          </p>
          <div v-else class="flex gap-3 max-h-72">
            <!-- 左：候选任务列表 -->
            <div
              class="flex-1 min-w-0 overflow-y-auto rounded border border-slate-200 divide-y divide-slate-100"
            >
              <label
                v-for="cand in importCandidates"
                :key="cand.id"
                class="flex items-center gap-2 px-2 py-1.5 text-xs cursor-pointer hover:bg-slate-50"
              >
                <input
                  type="radio"
                  :value="cand.id"
                  v-model="importSourceId"
                  @change="previewImport"
                />
                <span
                  class="flex-1 truncate"
                  style="color: var(--color-text-primary)"
                  >{{ cand.name }}</span
                >
                <span
                  class="font-mono tabular-nums text-[10px]"
                  style="color: var(--color-text-muted)"
                >
                  z{{ cand.minZoom }}-{{ cand.maxZoom }} ·
                  {{ formatCount(cand.downloadedTiles) }}
                </span>
              </label>
            </div>
            <!-- 右：所选任务的缩略预览 -->
            <div
              class="w-[200px] shrink-0 rounded border border-slate-200 overflow-hidden bg-slate-50 flex items-center justify-center"
              style="height: auto"
            >
              <MiniMapPreview
                v-if="selectedCandidate"
                :key="selectedCandidate.id"
                :task-id="selectedCandidate.id"
                :bounds="{
                  west: selectedCandidate.boundsWest,
                  east: selectedCandidate.boundsEast,
                  south: selectedCandidate.boundsSouth,
                  north: selectedCandidate.boundsNorth,
                }"
                :format="
                  (() => {
                    try {
                      return (
                        JSON.parse(selectedCandidate.sourceConfig).format ??
                        'png'
                      );
                    } catch {
                      return 'png';
                    }
                  })()
                "
                :show-basemap="false"
                class="w-full h-full"
              />
              <span
                v-else
                class="text-[11px] px-2 text-center"
                style="color: var(--color-text-muted)"
              >
                {{ t("taskDetail.incremental.previewHint") }}
              </span>
            </div>
          </div>
          <p
            v-if="importReusableCount !== null"
            class="text-xs px-2 py-1 rounded bg-blue-50 text-blue-700"
          >
            {{
              t("taskDetail.incremental.importReusable", {
                count: importReusableCount,
              })
            }}
          </p>
          <p
            v-else-if="importCountLoading"
            class="text-xs"
            style="color: var(--color-text-muted)"
          >
            {{ t("taskDetail.incremental.importCounting") }}
          </p>
          <p
            v-if="incrementalMessage"
            class="text-xs px-2 py-1 rounded bg-slate-50"
            style="color: var(--color-text-primary)"
          >
            {{ incrementalMessage }}
          </p>
          <div class="flex justify-end gap-2 mt-1">
            <button
              class="px-3 py-1.5 text-xs rounded-md border border-slate-200 text-slate-600 hover:bg-slate-50"
              @click="incrementalDialog = null"
            >
              {{ t("taskDetail.incremental.close") }}
            </button>
            <button
              class="px-3 py-1.5 text-xs rounded-md bg-blue-500 text-white hover:bg-blue-600 disabled:opacity-50"
              :disabled="incrementalBusy || !importSourceId"
              @click="runImport"
            >
              {{
                incrementalBusy
                  ? t("taskDetail.incremental.running")
                  : t("taskDetail.incremental.importConfirm")
              }}
            </button>
          </div>
        </div>
      </div>

      <!-- 运行日志 -->
      <div>
        <div class="flex items-center justify-between mb-2">
          <p
            class="text-xs font-semibold"
            style="color: var(--color-text-primary)"
          >
            运行日志
          </p>
          <button
            class="text-[10px] hover:underline"
            style="color: var(--color-text-muted)"
            @click="loadLogs"
          >
            刷新
          </button>
        </div>

        <div
          v-if="loadingLogs"
          class="text-xs text-center py-4"
          style="color: var(--color-text-muted)"
        >
          加载中…
        </div>

        <div
          v-else-if="logs.length === 0"
          class="text-xs text-center py-4"
          style="color: var(--color-text-muted)"
        >
          暂无日志
        </div>

        <div v-else class="space-y-1 max-h-60 overflow-y-auto">
          <div
            v-for="log in logs"
            :key="log.id"
            class="flex gap-2 text-[11px] font-mono"
          >
            <span class="shrink-0" style="color: var(--color-text-muted)">
              {{ logTime(log.timestamp) }}
            </span>
            <span
              class="shrink-0 uppercase text-[9px] font-bold mt-0.5"
              :class="logLevelMap[log.level] ?? 'text-slate-400'"
            >
              {{ log.level }}
            </span>
            <span class="break-all" style="color: var(--color-text-secondary)">
              {{ log.message }}
            </span>
          </div>
        </div>
      </div>
    </div>

    <!-- 加载占位 -->
    <div
      v-else
      class="flex-1 flex items-center justify-center"
      style="color: var(--color-text-muted)"
    >
      <Loader class="size-8 animate-spin" />
    </div>

    <!-- 导出对话框（在主 div 内，保持单根节点，避免 Fragment 导致 class 继承失效） -->
    <ExportDialog
      v-if="task"
      :open="showExport"
      :task="{
        id: task.id,
        name: task.name,
        downloadedTiles: task.downloadedTiles,
        minZoom: task.minZoom,
        maxZoom: task.maxZoom,
        boundsWest: task.boundsWest,
        boundsEast: task.boundsEast,
        boundsSouth: task.boundsSouth,
        boundsNorth: task.boundsNorth,
        crs: (() => {
          try {
            return JSON.parse(task.sourceConfig)?.crs ?? 'WEB_MERCATOR';
          } catch {
            return 'WEB_MERCATOR';
          }
        })(),
        format: (() => {
          try {
            return JSON.parse(task.sourceConfig)?.format ?? 'png';
          } catch {
            return 'png';
          }
        })(),
      }"
      @close="showExport = false"
    />
  </div>
</template>
