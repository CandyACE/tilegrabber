/**
 * 失败瓦片可视化共享状态（E 套件）
 *
 * 由 TaskDetail.vue 设置当前要查看的 (taskId, zoom)，
 * 由 FailedTilesLayer.vue 监听并在地图上绘制覆盖。
 */
import { ref } from "vue";

export interface FailedTilesView {
  taskId: string;
  zoom: number;
}

const view = ref<FailedTilesView | null>(null);

export function useFailedTilesView() {
  function show(taskId: string, zoom: number) {
    view.value = { taskId, zoom };
  }
  function hide() {
    view.value = null;
  }
  return { view, show, hide };
}
