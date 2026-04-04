<script setup lang="ts">
import { ref, onMounted, onUnmounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

interface ProgressPayload {
  task_id: string;
  bytes_per_sec: number;
  status: string;
}

// bytes/sec per active task
const speedMap = ref<Record<string, number>>({});

const totalSpeed = ref(0);

function formatSpeed(bps: number): string {
  if (bps <= 0) return "—";
  if (bps < 1024) return `${bps.toFixed(0)} B/s`;
  if (bps < 1024 * 1024) return `${(bps / 1024).toFixed(1)} KB/s`;
  return `${(bps / 1024 / 1024).toFixed(2)} MB/s`;
}

let unlisten: UnlistenFn | null = null;

onMounted(async () => {
  // 禁用右键菜单
  document.addEventListener("contextmenu", (e) => e.preventDefault());

  unlisten = await listen<ProgressPayload>("tilegrab-progress", (event) => {
    const { task_id, bytes_per_sec, status } = event.payload;
    if (
      [
        "completed",
        "completed_with_errors",
        "failed",
        "cancelled",
        "paused",
      ].includes(status)
    ) {
      delete speedMap.value[task_id];
    } else {
      speedMap.value[task_id] = bytes_per_sec;
    }
    totalSpeed.value = Object.values(speedMap.value).reduce((a, b) => a + b, 0);
  });
});

onUnmounted(() => {
  unlisten?.();
});

async function showMain() {
  await invoke("show_main_window");
}
</script>

<template>
  <div class="float-root" data-tauri-drag-region @dblclick.stop="showMain">
    <div class="float-icon" data-tauri-drag-region>
      <svg
        width="16"
        height="16"
        viewBox="0 0 16 16"
        fill="none"
        xmlns="http://www.w3.org/2000/svg"
        data-tauri-drag-region
      >
        <rect width="16" height="16" rx="3" fill="#3b82f6" />
        <path
          d="M3 11 L6 7 L9 9 L12 5"
          stroke="white"
          stroke-width="1.5"
          stroke-linecap="round"
          stroke-linejoin="round"
        />
      </svg>
    </div>
    <div class="float-info" data-tauri-drag-region>
      <span class="float-label" data-tauri-drag-region>下载速度</span>
      <span class="float-speed" data-tauri-drag-region>{{
        formatSpeed(totalSpeed)
      }}</span>
    </div>
  </div>
</template>

<style>
* {
  margin: 0;
  padding: 0;
  box-sizing: border-box;
}

.float-root {
  width: 200px;
  height: 56px;
  background: rgba(15, 23, 42, 0.88);
  backdrop-filter: blur(12px);
  -webkit-backdrop-filter: blur(12px);
  border-radius: 14px;
  border: 1px solid rgba(255, 255, 255, 0.12);
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 0 14px;
  cursor: default;
  box-shadow: 0 4px 20px rgba(0, 0, 0, 0.35);
  user-select: none;
}

.float-icon {
  flex-shrink: 0;
  display: flex;
  align-items: center;
}

.float-info {
  display: flex;
  flex-direction: column;
  gap: 1px;
  min-width: 0;
}

.float-label {
  font-family: -apple-system, "Microsoft YaHei", sans-serif;
  font-size: 10px;
  color: rgba(148, 163, 184, 0.9);
  line-height: 1.2;
}

.float-speed {
  font-family: "IBM Plex Mono", "Cascadia Code", monospace;
  font-size: 14px;
  font-weight: 600;
  color: #e2e8f0;
  line-height: 1.2;
  white-space: nowrap;
}
</style>
