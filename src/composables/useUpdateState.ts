import { ref, readonly } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { check, type Update } from "@tauri-apps/plugin-updater";

/**
 * 兼容旧版的轻量检查结果。`downloadUrl` / `releaseUrl` 在新机制下未使用，
 * 始终为 null —— 安装走 plugin-updater 内部流程。
 */
export interface UpdateCheckResult {
  currentVersion: string;
  latestVersion: string | null;
  hasUpdate: boolean;
  releaseUrl: string | null;
  downloadUrl: string | null;
  releaseNotes: string | null;
  error: string | null;
}

// 单例状态：整个应用共享一份后台检查结果与 Update 句柄
const autoCheckResult = ref<UpdateCheckResult | null>(null);
const hasUpdate = ref(false);
// Plugin 返回的 Update 句柄（含签名验证后的下载/安装方法）
const pendingUpdate = ref<Update | null>(null);

let _unlisten: UnlistenFn | null = null;
let _initialized = false;

function toResult(update: Update | null, currentVersion: string, error?: unknown): UpdateCheckResult {
  if (error) {
    return {
      currentVersion,
      latestVersion: null,
      hasUpdate: false,
      releaseUrl: null,
      downloadUrl: null,
      releaseNotes: null,
      error: String(error),
    };
  }
  if (!update) {
    return {
      currentVersion,
      latestVersion: currentVersion,
      hasUpdate: false,
      releaseUrl: null,
      downloadUrl: null,
      releaseNotes: null,
      error: null,
    };
  }
  return {
    currentVersion: update.currentVersion,
    latestVersion: update.version,
    hasUpdate: true,
    releaseUrl: null,
    downloadUrl: null,
    releaseNotes: update.body ?? null,
    error: null,
  };
}

/** 主动检查更新，返回结果并写入共享状态。 */
export async function runUpdateCheck(): Promise<UpdateCheckResult> {
  let result: UpdateCheckResult;
  try {
    const update = await check();
    // 释放上一次的句柄（避免泄漏）
    if (pendingUpdate.value && pendingUpdate.value !== update) {
      pendingUpdate.value.close().catch(() => {});
    }
    pendingUpdate.value = update;
    result = toResult(update, update?.currentVersion ?? "");
  } catch (e) {
    result = toResult(null, "", e);
  }
  autoCheckResult.value = result;
  hasUpdate.value = result.hasUpdate;
  return result;
}

/** 取出当前可用的 Update 句柄（供 SettingsPanel 调用 downloadAndInstall）。 */
export function getPendingUpdate(): Update | null {
  return pendingUpdate.value;
}

/** 安装完成后清理句柄。 */
export function clearPendingUpdate() {
  if (pendingUpdate.value) {
    pendingUpdate.value.close().catch(() => {});
    pendingUpdate.value = null;
  }
}

/** 初始化监听器（在 App.vue onMounted 调用一次即可）*/
export async function initUpdateListener() {
  if (_initialized) return;
  _initialized = true;
  // 后端启动 12 秒后会 emit "update-check-trigger"，由前端真正调用 plugin check()
  _unlisten = await listen<void>("update-check-trigger", () => {
    runUpdateCheck().catch(() => {});
  });
}

/** 清理监听器（App.vue onUnmounted 调用）*/
export function destroyUpdateListener() {
  _unlisten?.();
  _unlisten = null;
  _initialized = false;
}

/** 前端手动检查后，可以把结果写入这里，同步徽章状态（保留以兼容旧调用）。 */
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
    runUpdateCheck,
    getPendingUpdate,
    clearPendingUpdate,
  };
}
