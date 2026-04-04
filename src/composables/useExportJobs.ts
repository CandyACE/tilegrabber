import { ref, computed } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";

export interface ExportRecord {
  jobId: string;
  taskId: string;
  format: string;
  destPath: string;
  done: number;
  total: number;
  status: "running" | "done" | "error";
  error: string | null;
  startedAt: Date;
  finishedAt?: Date;
}

// —— 模块级单例，全局共享 ——
const activeJobs = ref<Record<string, ExportRecord>>({});
const historyByTask = ref<Record<string, ExportRecord[]>>({});
let unlisten: UnlistenFn | null = null;

async function ensureListener() {
  if (unlisten) return;
  unlisten = await listen<{
    jobId: string;
    done: number;
    total: number;
    status: "running" | "done" | "error";
    destPath: string;
    error: string | null;
  }>("export-progress", (ev) => {
    const p = ev.payload;
    const job = activeJobs.value[p.jobId];
    if (!job) return;

    const updated: ExportRecord = {
      ...job,
      done: p.done,
      total: p.total,
      status: p.status,
      error: p.error ?? null,
    };

    if (p.status === "done" || p.status === "error") {
      updated.finishedAt = new Date();
      // 加入历史
      const prev = historyByTask.value[job.taskId] ?? [];
      historyByTask.value[job.taskId] = [updated, ...prev];
      // 从活跃中移除
      const next = { ...activeJobs.value };
      delete next[p.jobId];
      activeJobs.value = next;
    } else {
      activeJobs.value = { ...activeJobs.value, [p.jobId]: updated };
    }
  });
}

/** 按 taskId 索引当前活跃导出（一任务同时只有一个活跃导出） */
const activeJobByTask = computed<Record<string, ExportRecord>>(() => {
  const result: Record<string, ExportRecord> = {};
  for (const job of Object.values(activeJobs.value)) {
    result[job.taskId] = job;
  }
  return result;
});

export function useExportJobs() {
  // 确保全局监听已建立
  ensureListener();

  /** 注册一个刚启动的导出任务 */
  function registerJob(
    jobId: string,
    taskId: string,
    format: string,
    destPath: string,
  ) {
    activeJobs.value = {
      ...activeJobs.value,
      [jobId]: {
        jobId,
        taskId,
        format,
        destPath,
        done: 0,
        total: 0,
        status: "running",
        error: null,
        startedAt: new Date(),
      },
    };
  }

  /** 获取某任务当前活跃导出（如有） */
  function getActiveJobForTask(taskId: string): ExportRecord | undefined {
    return activeJobByTask.value[taskId];
  }

  /** 获取某任务导出历史（最新在前） */
  function getHistoryForTask(taskId: string): ExportRecord[] {
    return historyByTask.value[taskId] ?? [];
  }

  /** 在资源管理器中定位文件 */
  async function revealInExplorer(path: string) {
    await invoke("reveal_in_explorer", { path }).catch(console.error);
  }

  return {
    activeJobByTask,
    historyByTask,
    registerJob,
    getActiveJobForTask,
    getHistoryForTask,
    revealInExplorer,
  };
}
