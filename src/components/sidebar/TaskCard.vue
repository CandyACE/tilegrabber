<script setup lang="ts">
import { computed, ref, watch } from "vue";
import {
  Download,
  PauseCircle,
  CheckCircle2,
  AlertCircle,
  Clock,
  Timer,
  Pause,
  Play,
  Loader,
  RefreshCw,
  Trash2,
  PackageOpen,
  PackageCheck,
  HardDrive,
} from "lucide-vue-next";
import type { ExportRecord } from "~/composables/useExportJobs";

export type TaskStatus =
  | "downloading"
  | "paused"
  | "completed"
  | "completed_with_errors"
  | "failed"
  | "pending"
  | "processing";

export interface Task {
  id: string;
  name: string;
  status: TaskStatus;
  sourceType: string;
  minZoom: number;
  maxZoom: number;
  totalTiles: number;
  downloadedTiles: number;
  failedTiles: number;
  speed?: number; // KB/s
  eta?: number; // seconds remaining
  createdAt: string;
  tileStorePath?: string | null;
  // 用于地图定位
  boundsWest?: number;
  boundsEast?: number;
  boundsSouth?: number;
  boundsNorth?: number;
}

const props = defineProps<{
  task: Task;
  live?: { speed: number; speedMb: number; eta: number | null };
  exportJob?: ExportRecord;
}>();

const emit = defineEmits<{
  pause: [id: string];
  resume: [id: string];
  cancel: [id: string];
  retry: [id: string];
  open: [id: string];
  delete: [id: string];
  export: [id: string];
}>();

const statusConfig: Record<
  TaskStatus,
  { label: string; bgColor: string; textColor: string; icon: object }
> = {
  downloading: {
    label: "下载中",
    bgColor: "#EBF2FA",
    textColor: "#0969DA",
    icon: Download,
  },
  paused: {
    label: "已暂停",
    bgColor: "#FFF8C5",
    textColor: "#9A6700",
    icon: PauseCircle,
  },
  completed: {
    label: "已完成",
    bgColor: "#DAFBE1",
    textColor: "#1A7F37",
    icon: CheckCircle2,
  },
  completed_with_errors: {
    label: "完成（含失败）",
    bgColor: "#FFF8C5",
    textColor: "#9A6700",
    icon: AlertCircle,
  },
  failed: {
    label: "失败",
    bgColor: "#FFEBE9",
    textColor: "#CF222E",
    icon: AlertCircle,
  },
  pending: {
    label: "等待中",
    bgColor: "#F6F8FA",
    textColor: "#57606A",
    icon: Clock,
  },
  processing: {
    label: "裁剪中",
    bgColor: "#E8DAFF",
    textColor: "#6639BA",
    icon: Download,
  },
};

const progressPercent = computed(() => {
  if (!props.task.totalTiles) return 0;
  return Math.floor((props.task.downloadedTiles / props.task.totalTiles) * 100);
});

const progressColor = computed(() => {
  switch (props.task.status) {
    case "downloading":
      return "var(--color-progress-blue)";
    case "processing":
      return "#8250df";
    case "completed":
      return "var(--color-progress-green)";
    case "completed_with_errors":
      return "var(--color-progress-amber)";
    case "failed":
      return "var(--color-progress-red)";
    case "paused":
      return "var(--color-progress-amber)";
    default:
      return "var(--color-border)";
  }
});

const formatEta = (secs: number) => {
  if (secs < 60) return `${secs.toFixed(1)}s`;
  if (secs < 3600) return `${Math.floor(secs / 60)}m${Math.floor(secs % 60)}s`;
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  return `${h}h${m}m`;
};

const sc = computed(
  () =>
    statusConfig[props.task.status] ?? {
      label: props.task.status,
      bgColor: "#F6F8FA",
      textColor: "#57606A",
      icon: Clock,
    },
);

// 暂停过渡中的加载状态：点击暂停后等待状态变更
const pausing = ref(false);
watch(
  () => props.task.status,
  () => {
    pausing.value = false;
  },
);
</script>

<template>
  <div
    class="px-3 py-2.5 rounded-lg cursor-pointer transition-all hover:shadow-sm border"
    style="
      background: var(--color-surface);
      border-color: var(--color-border-subtle);
    "
    @click="emit('open', task.id)"
  >
    <!-- 首行：名称 + 标识 + 状态徽章 -->
    <div class="flex items-start justify-between gap-2 mb-1.5">
      <span
        class="text-sm font-medium leading-snug truncate"
        style="color: var(--color-text-primary)"
      >
        {{ task.name }}
      </span>
      <div class="shrink-0 flex items-center gap-1">
        <!-- 外部文件标识：tileStorePath 指向 .tgr 文件 -->
        <span
          v-if="task.tileStorePath?.endsWith('.tgr')"
          class="flex items-center gap-0.5 px-1.5 py-0.5 rounded-full text-xs font-medium whitespace-nowrap"
          style="
            background: color-mix(
              in srgb,
              var(--color-accent) 15%,
              transparent
            );
            color: var(--color-accent);
          "
          title="此任务数据来自外部文件，请勿移动或删除该 .tgr 文件"
        >
          <HardDrive class="size-3" />
          外部
        </span>
        <span
          class="flex items-center gap-1 px-1.5 py-0.5 rounded-full text-xs font-medium whitespace-nowrap"
          :style="{ background: sc.bgColor, color: sc.textColor }"
        >
          <component :is="sc.icon" class="size-3" />
          {{ sc.label }}
        </span>
      </div>
    </div>

    <!-- 来源信息行 -->
    <p class="text-xs mb-2 font-mono" style="color: var(--color-text-muted)">
      {{ task.sourceType }} · z{{ task.minZoom }}–z{{ task.maxZoom }} ·
      {{ task.totalTiles.toLocaleString() }} 块
    </p>

    <!-- 进度条 -->
    <div
      class="relative h-1.5 rounded-full overflow-hidden mb-1.5"
      style="background: var(--color-elevated)"
    >
      <div
        class="absolute inset-y-0 left-0 rounded-full transition-all duration-300"
        :style="{ width: `${progressPercent}%`, background: progressColor }"
      />
    </div>

    <!-- 进度数据行 -->
    <div
      class="flex items-center justify-between text-xs font-mono"
      style="color: var(--color-text-secondary)"
    >
      <span
        >{{ progressPercent }}% ({{ task.downloadedTiles.toLocaleString() }}/{{
          task.totalTiles.toLocaleString()
        }})</span
      >
      <!-- 瓦片速度（下载中且有实时数据） -->
      <span
        v-if="task.status === 'downloading' && live && live.speed > 0"
        class="text-blue-600"
      >
        {{ live.speed < 1 ? live.speed.toFixed(1) : Math.round(live.speed) }}
        片/s
      </span>
    </div>

    <!-- 字节速度 + ETA（下载中） -->
    <div
      v-if="task.status === 'downloading' && live && live.speed > 0"
      class="flex items-center justify-between mt-1 text-xs"
      style="color: var(--color-text-muted)"
    >
      <span class="flex items-center gap-1">
        <Timer class="size-3" />
        <span>{{
          live.speedMb >= 0.1
            ? live.speedMb.toFixed(2) + " MB/s"
            : (live.speedMb * 1024).toFixed(0) + " KB/s"
        }}</span>
      </span>
      <span v-if="live.eta">剩余 {{ formatEta(live.eta) }}</span>
    </div>

    <!-- 导出进度条（有活跃导出任务时显示） -->
    <div v-if="exportJob" class="mt-2" @click.stop>
      <div class="flex items-center justify-between text-[11px] mb-1">
        <span
          class="flex items-center gap-1"
          style="color: var(--color-text-secondary)"
        >
          <PackageCheck class="size-3 text-indigo-500" />
          导出中
        </span>
        <span
          v-if="exportJob.total > 0"
          class="font-mono tabular-nums"
          style="color: var(--color-text-muted)"
        >
          {{ Math.round((exportJob.done / exportJob.total) * 100) }}%
        </span>
      </div>
      <div
        class="relative h-1 rounded-full overflow-hidden"
        style="background: var(--color-elevated)"
      >
        <div
          class="absolute inset-y-0 left-0 rounded-full transition-all duration-300"
          :class="
            exportJob.total > 0
              ? 'bg-indigo-500'
              : 'bg-indigo-400 animate-pulse'
          "
          :style="{
            width:
              exportJob.total > 0
                ? `${Math.min(100, Math.round((exportJob.done / exportJob.total) * 100))}%`
                : '40%',
          }"
        />
      </div>
    </div>

    <!-- 操作按钮（激活中任务） -->
    <div
      v-if="task.status === 'downloading' || task.status === 'paused'"
      class="flex items-center gap-1 mt-2 justify-end"
      @click.stop
    >
      <!-- 下载中：仅显示暂停按钮（带加载态） -->
      <button
        v-if="task.status === 'downloading'"
        class="flex items-center gap-1 px-2 py-1 rounded text-xs font-medium border transition-colors"
        :class="pausing ? 'opacity-60 cursor-not-allowed' : 'hover:bg-slate-50'"
        style="
          color: var(--color-text-secondary);
          border-color: var(--color-border-subtle);
        "
        :disabled="pausing"
        @click="
          pausing = true;
          emit('pause', task.id);
        "
      >
        <Loader v-if="pausing" class="size-3 animate-spin" />
        <Pause v-else class="size-3" />
        {{ pausing ? "正在暂停…" : "暂停" }}
      </button>
      <!-- 已暂停：继续 + 导出 + 停止 -->
      <button
        v-if="task.status === 'paused'"
        class="flex items-center gap-1 px-2 py-1 rounded text-xs font-medium border transition-colors hover:bg-blue-50"
        style="color: var(--color-accent); border-color: var(--color-accent)"
        @click="emit('resume', task.id)"
      >
        <Play class="size-3" />继续
      </button>
      <button
        v-if="task.status === 'paused'"
        class="flex items-center gap-1 px-2 py-1 rounded text-xs font-medium border transition-colors hover:bg-slate-50"
        style="
          color: var(--color-text-secondary);
          border-color: var(--color-border-subtle);
        "
        @click="emit('export', task.id)"
      >
        <PackageOpen class="size-3" />导出
      </button>
    </div>

    <!-- 失败任务重试 -->
    <div
      v-if="task.status === 'failed'"
      class="flex items-center gap-1 mt-2 justify-end"
      @click.stop
    >
      <button
        class="flex items-center gap-1 px-2 py-1 rounded text-xs font-medium border transition-colors hover:bg-orange-50"
        style="color: #9a6700; border-color: #fff8c5"
        @click="emit('retry', task.id)"
      >
        <RefreshCw class="size-3" />重试
      </button>
      <button
        class="flex items-center gap-1 px-2 py-1 rounded text-xs font-medium border transition-colors hover:bg-slate-50"
        style="
          color: var(--color-text-secondary);
          border-color: var(--color-border-subtle);
        "
        @click="emit('export', task.id)"
      >
        <PackageOpen class="size-3" />导出
      </button>
      <button
        class="flex items-center gap-1 px-2 py-1 rounded text-xs font-medium border transition-colors hover:bg-red-50"
        style="color: #cf222e; border-color: #ffebe9"
        @click="emit('delete', task.id)"
      >
        <Trash2 class="size-3" />删除
      </button>
    </div>

    <!-- 完成/取消任务导出+删除 -->
    <div
      v-if="
        task.status === 'completed' ||
        task.status === 'completed_with_errors' ||
        task.status === 'cancelled'
      "
      class="flex items-center gap-1 mt-2 justify-end"
      @click.stop
    >
      <button
        class="flex items-center gap-1 px-2 py-1 rounded text-xs font-medium border transition-colors hover:bg-slate-50"
        style="
          color: var(--color-text-secondary);
          border-color: var(--color-border-subtle);
        "
        @click="emit('export', task.id)"
      >
        <PackageOpen class="size-3" />导出
      </button>
      <button
        class="flex items-center gap-1 px-2 py-1 rounded text-xs font-medium border transition-colors hover:bg-slate-50"
        style="
          color: var(--color-text-muted);
          border-color: var(--color-border-subtle);
        "
        @click="emit('delete', task.id)"
      >
        <Trash2 class="size-3" />删除
      </button>
    </div>
  </div>
</template>
