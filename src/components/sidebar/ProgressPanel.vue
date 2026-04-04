<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// ─── 类型 ──────────────────────────────────────────────────────────────────
interface ProgressPayload {
  task_id: string;
  total: number;
  downloaded: number;
  failed: number;
  speed: number;
  eta_secs: number | null;
  status: string;
}

const props = defineProps<{
  taskId: string;
  taskName?: string;
}>();

const emit = defineEmits<{
  done: [];
}>();

// ─── 状态 ──────────────────────────────────────────────────────────────────
const payload = ref<ProgressPayload | null>(null);
let unlisten: UnlistenFn | null = null;

const percent = computed(() => {
  if (!payload.value || payload.value.total === 0) return 0;
  // failed 瓦片同样算作"已处理"，避免有失败时进度条永远到不了 100%
  return Math.min(
    100,
    Math.floor(
      ((payload.value.downloaded + payload.value.failed) /
        payload.value.total) *
        100,
    ),
  );
});

const progressBarColor = computed(() => {
  switch (payload.value?.status) {
    case "completed":
      return "var(--color-progress-green, #2da44e)";
    case "completed_with_errors":
      return "var(--color-progress-amber, #d4a72c)";
    case "failed":
      return "var(--color-progress-red, #cf222e)";
    case "cancelled":
      return "var(--color-border, #d0d7de)";
    case "processing":
      return "#8250df";
    default:
      return "var(--color-progress-blue, #0969da)";
  }
});

const statusLabel = computed(() => {
  switch (payload.value?.status) {
    case "downloading":
      return "下载中";
    case "processing":
      return "裁剪中";
    case "paused":
      return "已暂停";
    case "completed":
      return "已完成";
    case "completed_with_errors":
      return "完成（含失败）";
    case "failed":
      return "失败";
    case "cancelled":
      return "已取消";
    default:
      return "等待中";
  }
});

const isDownloading = computed(
  () =>
    payload.value?.status === "downloading" ||
    payload.value?.status === "processing",
);
const isPaused = computed(() => payload.value?.status === "paused");
const isDone = computed(() =>
  ["completed", "completed_with_errors", "failed", "cancelled"].includes(
    payload.value?.status ?? "",
  ),
);

function formatCount(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(n);
}

function formatEta(secs: number | null): string {
  if (secs === null || secs < 0) return "—";
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = secs % 60;
  if (h > 0) return `${h}h ${m}m`;
  if (m > 0) return `${m}m ${s.toFixed(1)}s`;
  return `${s.toFixed(1)}s`;
}

// ─── 控制 ──────────────────────────────────────────────────────────────────
const confirmingCancel = ref(false);

async function pause() {
  await invoke("pause_download", { taskId: props.taskId }).catch(console.error);
}

async function resume() {
  await invoke("resume_download", { taskId: props.taskId }).catch(
    console.error,
  );
}

function requestCancel() {
  confirmingCancel.value = true;
}

async function doCancel() {
  confirmingCancel.value = false;
  await invoke("cancel_download", { taskId: props.taskId }).catch(
    console.error,
  );
}

// ─── 事件监听 ──────────────────────────────────────────────────────────────
onMounted(async () => {
  unlisten = await listen<ProgressPayload>("tilegrab-progress", (event) => {
    if (event.payload.task_id === props.taskId) {
      payload.value = event.payload;
      if (isDone.value) emit("done");
    }
  });
});

onUnmounted(() => {
  unlisten?.();
});
</script>

<template>
  <div class="flex flex-col gap-2 select-none">
    <!-- 任务名称 + 状态标记 -->
    <div class="flex items-center justify-between">
      <span
        class="text-xs font-semibold text-(--color-text-primary) truncate max-w-40"
      >
        {{ taskName ?? taskId }}
      </span>
      <span
        class="inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[10px] font-medium"
        :class="{
          'bg-blue-50 text-blue-700': payload?.status === 'downloading',
          'bg-purple-50 text-purple-700': payload?.status === 'processing',
          'bg-amber-50 text-amber-700': isPaused,
          'bg-green-50 text-green-700': payload?.status === 'completed',
          'bg-orange-50 text-orange-700':
            payload?.status === 'completed_with_errors',
          'bg-red-50 text-red-700': payload?.status === 'failed',
          'bg-slate-100 text-slate-500':
            !payload || payload.status === 'cancelled',
        }"
      >
        <span
          v-if="isDownloading"
          class="i-lucide-loader-circle animate-spin size-2.5"
        />
        {{ statusLabel }}
      </span>
    </div>

    <!-- 进度条 -->
    <div class="h-1.5 w-full rounded-full overflow-hidden bg-slate-100">
      <div
        class="h-full rounded-full transition-all duration-300"
        :style="{ width: `${percent}%`, backgroundColor: progressBarColor }"
      />
    </div>

    <!-- 数字统计 -->
    <div
      class="flex items-center justify-between text-[11px] text-(--color-text-secondary)"
    >
      <span v-if="payload?.status === 'processing'">
        已裁剪 {{ formatCount(payload?.downloaded ?? 0) }}
        /
        {{ formatCount(payload?.total ?? 0) }} 瓦片
      </span>
      <span v-else>
        {{ formatCount(payload?.downloaded ?? 0) }}
        /
        {{ formatCount(payload?.total ?? 0) }} 瓦片
        <span v-if="(payload?.failed ?? 0) > 0" class="text-red-500 ml-1">
          ({{ payload!.failed }} 失败)
        </span>
      </span>
      <span class="flex items-center gap-2 font-mono">
        <span v-if="payload?.status === 'downloading'">
          {{ payload!.speed.toFixed(1) }} t/s
        </span>
        <span
          v-if="payload?.status === 'downloading' && payload?.eta_secs !== null"
        >
          ETA {{ formatEta(payload!.eta_secs) }}
        </span>
        <span v-if="payload?.status !== 'downloading'">{{ percent }}%</span>
      </span>
    </div>

    <!-- 操作按钮 -->
    <div v-if="!isDone" class="mt-0.5">
      <!-- 正常操作行 -->
      <div v-if="!confirmingCancel" class="flex items-center gap-2">
        <button
          v-if="payload?.status === 'downloading'"
          class="flex-1 flex items-center justify-center gap-1 rounded-md px-2 py-1 text-xs border border-amber-300 bg-amber-50 text-amber-700 hover:bg-amber-100 transition-colors"
          @click="pause"
        >
          <span class="i-lucide-pause size-3" />
          暂停
        </button>

        <button
          v-if="isPaused"
          class="flex-1 flex items-center justify-center gap-1 rounded-md px-2 py-1 text-xs border border-blue-300 bg-blue-50 text-blue-700 hover:bg-blue-100 transition-colors"
          @click="resume"
        >
          <span class="i-lucide-play size-3" />
          继续
        </button>

        <button
          class="flex items-center justify-center gap-1 rounded-md px-2 py-1 text-xs border border-slate-200 bg-white text-slate-500 hover:border-red-200 hover:text-red-500 transition-colors"
          @click="requestCancel"
        >
          <span class="i-lucide-x size-3" />
          取消
        </button>
      </div>

      <!-- 内联取消确认 -->
      <Transition name="fade-slide-up">
        <div
          v-if="confirmingCancel"
          class="rounded-lg border border-red-200 bg-red-50 p-2.5 space-y-2"
        >
          <p class="text-[11px] text-red-700 font-medium">
            确定取消此下载任务？
          </p>
          <div class="flex gap-1.5">
            <button
              class="flex-1 rounded-md px-2 py-1 text-xs border border-slate-200 bg-white text-slate-600 hover:bg-slate-50 transition-colors"
              @click="confirmingCancel = false"
            >
              继续下载
            </button>
            <button
              class="flex-1 rounded-md px-2 py-1 text-xs border border-red-300 bg-red-100 text-red-700 hover:bg-red-200 transition-colors font-medium"
              @click="doCancel"
            >
              确定取消
            </button>
          </div>
        </div>
      </Transition>
    </div>
  </div>
</template>
