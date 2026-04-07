import { ref, readonly } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface UpdateCheckResult {
  currentVersion: string;
  latestVersion: string | null;
  hasUpdate: boolean;
  releaseUrl: string | null;
  downloadUrl: string | null;
  releaseNotes: string | null;
  error: string | null;
}

// 单例状态：整个应用共享一份后台检查结果
const autoCheckResult = ref<UpdateCheckResult | null>(null);
const hasUpdate = ref(false);

let _unlisten: UnlistenFn | null = null;
let _initialized = false;

/** 初始化监听器（在 App.vue onMounted 调用一次即可）*/
export async function initUpdateListener() {
  if (_initialized) return;
  _initialized = true;
  _unlisten = await listen<UpdateCheckResult>("update-available", (event) => {
    autoCheckResult.value = event.payload;
    hasUpdate.value = event.payload.hasUpdate;
  });
}

/** 清理监听器（App.vue onUnmounted 调用）*/
export function destroyUpdateListener() {
  _unlisten?.();
  _unlisten = null;
  _initialized = false;
}

/** 前端手动检查后，可以把结果写入这里，同步徽章状态 */
export function setUpdateResult(result: UpdateCheckResult) {
  autoCheckResult.value = result;
  hasUpdate.value = result.hasUpdate;
}

export function clearUpdateBadge() {
  hasUpdate.value = false;
}

export function useUpdateState() {
  return {
    autoCheckResult: readonly(autoCheckResult),
    hasUpdate: readonly(hasUpdate),
    setUpdateResult,
    clearUpdateBadge,
  };
}
