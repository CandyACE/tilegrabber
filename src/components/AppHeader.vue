<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import {
  PanelLeft,
  Map,
  Settings,
  HelpCircle,
  Share2,
  Info,
  Minus,
  Square,
  X as XIcon,
  ListTodo,
} from "lucide-vue-next";
import { useI18n } from "vue-i18n";
import logoUrl from "~/assets/logo.png";
import { useUpdateState } from "~/composables/useUpdateState";

const { t } = useI18n();

const props = defineProps<{
  sidebarOpen: boolean;
  activeNav: string;
}>();

const emit = defineEmits<{
  "update:sidebarOpen": [value: boolean];
  "nav-change": [key: string];
}>();

const { hasUpdate, clearUpdateBadge } = useUpdateState();

function onNavClick(key: string) {
  if (key === "settings") clearUpdateBadge();
  emit("nav-change", key);
}

const navItems = computed(() => [
  { key: "map" as const, label: t('nav.map'), icon: Map },
  { key: "tasks" as const, label: t('nav.tasks'), icon: ListTodo },
  { key: "publish" as const, label: t('nav.publish'), icon: Share2 },
  { key: "settings" as const, label: t('nav.settings'), icon: Settings },
  { key: "help" as const, label: t('nav.help'), icon: HelpCircle },
  { key: "about" as const, label: t('nav.about'), icon: Info },
]);

function toggleSidebar() {
  emit("update:sidebarOpen", !props.sidebarOpen);
}

// ─── 发布服务状态轮询 ──────────────────────────────────────────────────────────
const serverRunning = ref(false);
let serverPollTimer: ReturnType<typeof setInterval> | null = null;

async function checkServerStatus() {
  try {
    const status = await invoke<{ running: boolean }>("get_server_status");
    serverRunning.value = status.running;
  } catch {
    serverRunning.value = false;
  }
}

// ─── 窗口控制 ──────────────────────────────────────────────────────────────────
const isMaximized = ref(false);
const appVersion = ref("");
let unlistenResize: (() => void) | null = null;

onMounted(async () => {
  const win = getCurrentWindow();
  isMaximized.value = await win.isMaximized();
  unlistenResize = await win.onResized(async () => {
    isMaximized.value = await win.isMaximized();
  });
  appVersion.value = await getVersion();
  // 初始检查发布服务状态，每 5 秒轮询一次
  await checkServerStatus();
  serverPollTimer = setInterval(checkServerStatus, 5000);
});

onUnmounted(() => {
  unlistenResize?.();
  if (serverPollTimer) clearInterval(serverPollTimer);
});

async function minimize() {
  await getCurrentWindow().minimize();
}
async function toggleMaximize() {
  await getCurrentWindow().toggleMaximize();
}
async function closeWindow() {
  await getCurrentWindow().close();
}
</script>

<template>
  <header
    class="flex shrink-0 items-center h-11 border-b select-none"
    style="
      background: var(--color-surface);
      border-color: var(--color-border-subtle);
    "
  >
    <!-- 左侧：侧边栏开关 + Logo -->
    <div class="flex items-center gap-2 px-3 shrink-0">
      <button
        class="p-1.5 rounded-md transition-colors hover:bg-slate-100 active:bg-slate-200"
        :title="sidebarOpen ? t('header.collapseSidebar') : t('header.expandSidebar')"
        @click="toggleSidebar"
      >
        <PanelLeft class="size-4 text-slate-500" />
      </button>
      <div
        class="flex items-center gap-1.5 cursor-default"
        data-tauri-drag-region
        @dblclick="toggleMaximize"
      >
        <img
          :src="logoUrl"
          class="size-6 rounded-md object-contain"
          alt="御图"
        />
        <span
          class="text-sm font-semibold tracking-tight"
          style="color: var(--color-text-primary)"
        >
          {{ t('appName') }}
        </span>
      </div>
    </div>

    <!-- 中间左侧拖拽区 -->
    <div
      class="flex-1 h-full"
      data-tauri-drag-region
      @dblclick="toggleMaximize"
    />

    <!-- 主导航（居中，不撑开） -->
    <nav class="flex items-center gap-0.5 shrink-0">
      <button
        v-for="item in navItems"
        :key="item.key"
        class="relative flex items-center gap-1.5 px-3 py-1.5 rounded-md text-sm font-medium transition-colors"
        :class="
          activeNav === item.key
            ? 'text-blue-600 bg-blue-50'
            : 'text-slate-500 hover:text-slate-800 hover:bg-slate-100'
        "
        @click="onNavClick(item.key)"
      >
        <component :is="item.icon" class="size-4" />
        {{ item.label }}
        <!-- 更新可用时，在「设置」按钮右上角显示红点 -->
        <span
          v-if="item.key === 'settings' && hasUpdate"
          class="absolute top-0.5 right-0.5 size-2 rounded-full bg-red-500"
          :title="t('header.updateAvailable')"
        />
      </button>
    </nav>

    <!-- 中间右侧拖拽区 -->
    <div
      class="flex-1 h-full"
      data-tauri-drag-region
      @dblclick="toggleMaximize"
    />

    <!-- 右侧：状态 + 窗口控制 -->
    <div class="flex items-center">
      <!-- 发布服务运行状态 -->
      <div
        class="flex items-center gap-1.5 px-2 py-0.5 mr-2 rounded-full text-xs font-medium transition-colors"
        :style="
          serverRunning
            ? 'background: var(--color-badge-green-bg); color: #1a7f37'
            : 'background: var(--color-badge-gray-bg, #f1f5f9); color: #64748b'
        "
      >
        <span
          class="size-1.5 rounded-full inline-block"
          :class="serverRunning ? 'bg-green-500 animate-pulse' : 'bg-slate-400'"
        />
        {{ serverRunning ? t('header.apiRunning') : t('header.apiStopped') }}
      </div>
      <span
        class="text-xs font-mono mr-3"
        style="color: var(--color-text-muted)"
        >v{{ appVersion }}</span
      >

      <!-- 最小化 -->
      <button
        class="flex items-center justify-center w-11 h-11 text-slate-500 hover:bg-slate-100 transition-colors"
        :title="t('header.minimize')"
        @click="minimize"
      >
        <Minus class="size-3.5" />
      </button>
      <!-- 最大化 / 还原 -->
      <button
        class="flex items-center justify-center w-11 h-11 text-slate-500 hover:bg-slate-100 transition-colors"
        :title="isMaximized ? t('header.restore') : t('header.maximize')"
        @click="toggleMaximize"
      >
        <Square class="size-3.5" />
      </button>
      <!-- 关闭 -->
      <button
        class="flex items-center justify-center w-11 h-11 text-slate-500 hover:bg-red-500 hover:text-white transition-colors"
        :title="t('header.close')"
        @click="closeWindow"
      >
        <XIcon class="size-3.5" />
      </button>
    </div>
  </header>
</template>
