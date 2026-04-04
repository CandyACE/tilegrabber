<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  confirm as tauriConfirm,
  open as tauriOpen,
  save as tauriSave,
} from "@tauri-apps/plugin-dialog";
import { Plus, Inbox, Upload } from "lucide-vue-next";
import TaskCard from "./TaskCard.vue";
import type { Task, TaskStatus } from "./TaskCard.vue";
import { useExportJobs } from "~/composables/useExportJobs";

// ─── Tauri 后端返回的 Task 格式（camelCase） ───────────────────────────────
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
  createdAt: string;
  updatedAt: string;
}

interface ProgressPayload {
  task_id: string;
  total: number;
  downloaded: number;
  failed: number;
  speed: number;
  bytes_per_sec: number;
  eta_secs: number | null;
  status: string;
}

function toFrontendTask(b: BackendTask): Task {
  return {
    id: b.id,
    name: b.name,
    status: (b.status as TaskStatus) ?? "pending",
    sourceType: (() => {
      try {
        const src = JSON.parse(b.sourceConfig);
        return src.source_type ?? src.sourceType ?? "TMS";
      } catch {
        return "TMS";
      }
    })(),
    minZoom: b.minZoom,
    maxZoom: b.maxZoom,
    totalTiles: b.totalTiles,
    downloadedTiles: b.downloadedTiles,
    failedTiles: b.failedTiles,
    createdAt: b.createdAt,
    tileStorePath: b.tileStorePath,
    boundsWest: b.boundsWest,
    boundsEast: b.boundsEast,
    boundsSouth: b.boundsSouth,
    boundsNorth: b.boundsNorth,
  };
}

const props = defineProps<{
  open: boolean;
}>();

const emit = defineEmits<{
  "update:open": [value: boolean];
  "new-task": [];
  "open-task": [taskId: string];
}>();

type FilterKey = "all" | TaskStatus;

const activeFilter = ref<FilterKey>("all");

const filters: { key: FilterKey; label: string }[] = [
  { key: "all", label: "全部" },
  { key: "downloading", label: "进行中" },
  { key: "completed", label: "完成" },
  { key: "failed", label: "失败" },
];

// ─── 真实任务数据 ──────────────────────────────────────────────────────────
const tasks = ref<Task[]>([]);
// 记录各任务的动态速度/ETA（来自进度事件）
const taskLive = ref<
  Record<string, { speed: number; speedMb: number; eta: number | null }>
>({});

const { getActiveJobForTask } = useExportJobs();

async function loadTasks() {
  try {
    const list = await invoke<BackendTask[]>("list_tasks");
    tasks.value = list.map(toFrontendTask);
  } catch (e) {
    console.error("[TaskSidebar] list_tasks failed:", e);
  }
}

let unlistenProgress: UnlistenFn | null = null;

onMounted(async () => {
  // 先注册事件监听，避免在 loadTasks 期间丢失进度事件
  unlistenProgress = await listen<ProgressPayload>(
    "tilegrab-progress",
    (event) => {
      const p = event.payload;
      // 更新任务状态
      const idx = tasks.value.findIndex((t) => t.id === p.task_id);
      if (idx !== -1) {
        tasks.value[idx] = {
          ...tasks.value[idx],
          status: p.status as TaskStatus,
          totalTiles: p.total,
          downloadedTiles: p.downloaded,
          failedTiles: p.failed,
        };
        taskLive.value[p.task_id] = {
          speed: p.speed,
          speedMb: p.bytes_per_sec / (1024 * 1024),
          eta: p.eta_secs,
        };
      } else {
        // 收到未知任务的进度事件，重新加载任务列表
        loadTasks();
      }
      // 完成后刷新列表以获取最终状态
      if (
        ["completed", "completed_with_errors", "failed", "cancelled"].includes(
          p.status,
        )
      ) {
        loadTasks();
      }
    },
  );

  await loadTasks();
});

onUnmounted(() => {
  unlistenProgress?.();
});

// ─── 操作处理器 ────────────────────────────────────────────────────────────
async function handlePause(id: string) {
  await invoke("pause_download", { taskId: id }).catch(console.error);
}

async function handleResume(id: string) {
  await invoke("resume_download", { taskId: id }).catch(console.error);
}

async function handleCancel(id: string) {
  const ok = await tauriConfirm("取消后任务进度将保留，可随时重新开始。", {
    title: "确定取消该任务？",
    kind: "warning",
  });
  if (!ok) return;
  await invoke("cancel_download", { taskId: id }).catch(console.error);
  await loadTasks();
}

async function handleRetry(id: string) {
  await invoke("retry_failed", { taskId: id }).catch(console.error);
}

async function handleDelete(id: string) {
  const task = tasks.value.find((t) => t.id === id);
  const isExternal = task?.tileStorePath?.endsWith(".tgr") ?? false;

  const ok = await tauriConfirm(
    isExternal
      ? "任务记录将被删除。该任务使用外部文件，你需要另行决定是否删除 .tgr 文件。"
      : "任务及其所有已下载瓦片将被永久删除，此操作不可撤销。",
    {
      title: "确定删除该任务？",
      kind: "warning",
    },
  );
  if (!ok) return;

  let deleteFile = false;
  if (isExternal && task?.tileStorePath) {
    deleteFile = await tauriConfirm(
      `是否同时删除 .tgr 文件？\n${task.tileStorePath}`,
      { title: "同时删除外部文件？", kind: "warning" },
    );
  }

  await invoke("delete_task", { taskId: id, deleteFile }).catch(console.error);
  await loadTasks();
}

async function handleExport(id: string) {
  const task = tasks.value.find((t) => t.id === id);
  const defaultName = task
    ? `${task.name.replace(/[\\/:*?"<>|]/g, "_")}.tgr`
    : `task-${id.slice(0, 8)}.tgr`;
  const destPath = await tauriSave({
    title: "导出任务包",
    defaultPath: defaultName,
    filters: [{ name: "御图 任务包", extensions: ["tgr"] }],
  });
  if (!destPath) return;
  try {
    await invoke("export_task", { taskId: id, destPath });
    alert(
      `导出成功！\n\n注意：导入后程序将直接读取此文件，请勿移动或删除：\n${destPath}`,
    );
  } catch (e) {
    alert(`导出失败：${e}`);
  }
}

async function handleImport() {
  const srcPath = await tauriOpen({
    title: "导入任务包",
    filters: [{ name: "御图 任务包", extensions: ["tgr"] }],
    multiple: false,
  });
  if (!srcPath) return;
  const path = Array.isArray(srcPath) ? srcPath[0] : srcPath;
  try {
    await invoke("import_task", { srcPath: path });
    await loadTasks();
  } catch (e) {
    alert(`导入失败：${e}`);
  }
}

function handleOpen(id: string) {
  emit("open-task", id);
}

// ─── 过滤 ─────────────────────────────────────────────────────────────────
const filteredTasks = computed(() => {
  if (activeFilter.value === "all") return tasks.value;
  if (activeFilter.value === "downloading") {
    return tasks.value.filter(
      (t) => t.status === "downloading" || t.status === "paused",
    );
  }
  return tasks.value.filter((t) => t.status === activeFilter.value);
});

const countByFilter = computed(() => {
  const result: Record<FilterKey, number> = {
    all: tasks.value.length,
    downloading: 0,
    completed: 0,
    failed: 0,
  };
  tasks.value.forEach((t) => {
    if (t.status === "downloading" || t.status === "paused")
      result.downloading++;
    else if (t.status === "completed" || t.status === "completed_with_errors")
      result.completed++;
    else if (t.status === "failed") result.failed++;
  });
  return result;
});

// 侧边栏宽度动画
const sidebarWidth = computed(() => (props.open ? "280px" : "0px"));

defineExpose({ loadTasks });
</script>

<template>
  <aside
    class="flex flex-col shrink-0 overflow-hidden transition-all duration-200 ease-in-out border-r"
    :style="{
      width: sidebarWidth,
      background: 'var(--color-app-bg)',
      borderColor: 'var(--color-border-subtle)',
      minWidth: open ? '280px' : '0px',
    }"
  >
    <!-- 内容容器（固定宽度，防止折叠时文字乱跑） -->
    <div class="flex flex-col h-full w-70">
      <!-- 侧边栏头部 -->
      <div
        class="flex items-center justify-between px-4 py-3 border-b shrink-0"
        style="border-color: var(--color-border-subtle)"
      >
        <h2
          class="text-sm font-semibold"
          style="color: var(--color-text-primary)"
        >
          下载任务
        </h2>
        <div class="flex items-center gap-1">
          <button
            class="flex items-center gap-1 px-2 py-1 rounded-md text-xs font-medium transition-colors hover:bg-slate-100"
            style="color: var(--color-text-secondary)"
            title="导入任务包"
            @click="handleImport"
          >
            <Upload class="size-3.5" />
          </button>
          <button
            class="flex items-center gap-1.5 px-2.5 py-1 rounded-md text-xs font-medium text-white transition-colors hover:opacity-90 active:opacity-80"
            style="background: var(--color-accent)"
            @click="emit('new-task')"
          >
            <Plus class="size-3.5" />
            创建下载任务
          </button>
        </div>
      </div>

      <!-- 过滤标签 -->
      <div
        class="flex items-center gap-0.5 px-3 py-2 border-b shrink-0"
        style="border-color: var(--color-border-subtle)"
      >
        <button
          v-for="f in filters"
          :key="f.key"
          class="flex items-center gap-1 px-2.5 py-1 rounded-md text-xs font-medium transition-colors whitespace-nowrap"
          :class="
            activeFilter === f.key
              ? 'text-blue-600 bg-blue-50'
              : 'text-slate-500 hover:text-slate-700 hover:bg-slate-100'
          "
          @click="activeFilter = f.key"
        >
          {{ f.label }}
          <span
            v-if="countByFilter[f.key] > 0"
            class="px-1.5 py-0.5 rounded-full text-[10px] leading-none"
            :class="
              activeFilter === f.key
                ? 'bg-blue-100 text-blue-700'
                : 'bg-slate-200 text-slate-500'
            "
          >
            {{ countByFilter[f.key] }}
          </span>
        </button>
      </div>

      <!-- 任务列表 -->
      <div class="flex-1 overflow-y-auto px-3 py-2 space-y-2">
        <template v-if="filteredTasks.length > 0">
          <TaskCard
            v-for="task in filteredTasks"
            :key="task.id"
            :task="task"
            :live="taskLive[task.id]"
            :exportJob="getActiveJobForTask(task.id)"
            @pause="handlePause"
            @resume="handleResume"
            @cancel="handleCancel"
            @retry="handleRetry"
            @delete="handleDelete"
            @export="handleExport"
            @open="handleOpen"
          />
        </template>

        <!-- 空状态 -->
        <div
          v-else
          class="flex flex-col items-center justify-center py-12 text-center"
        >
          <Inbox class="size-10 mb-3 text-slate-300" />
          <p
            class="text-sm font-medium mb-1"
            style="color: var(--color-text-secondary)"
          >
            暂无任务
          </p>
          <p class="text-xs" style="color: var(--color-text-muted)">
            点击「创建下载任务」按钮开始
          </p>
        </div>
      </div>
    </div>
  </aside>
</template>
