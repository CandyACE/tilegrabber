import { ref } from "vue";

/**
 * 全局任务详情状态 — 在侧边栏与地图页面之间共享选中任务 ID 与状态
 */
const selectedTaskId = ref<string | null>(null);
const selectedTaskStatus = ref<string | null>(null);

export function useTaskDetail() {
  function openTask(id: string) {
    selectedTaskId.value = id;
  }

  function closeTask() {
    selectedTaskId.value = null;
    selectedTaskStatus.value = null;
  }

  return { selectedTaskId, selectedTaskStatus, openTask, closeTask };
}
