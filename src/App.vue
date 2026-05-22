<script setup lang="ts">
import { ref, computed, nextTick, onMounted, onUnmounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { BoxSelect, Trash2, X, WifiOff, HardDrive } from "lucide-vue-next";
import { useI18n } from "vue-i18n";
import type { Map as MaplibreMap, LngLatBoundsLike } from "maplibre-gl";
import type { Bounds, CrsType, TileSource } from "~/types/tile-source";
import { useWizardState } from "~/composables/useWizardState";
import { useTaskDetail } from "~/composables/useTaskDetail";
import { useExtendMode } from "~/composables/useExtendMode";
import { useToast } from "~/composables/useToast";
import { initUpdateListener, destroyUpdateListener } from "~/composables/useUpdateState";
import { i18n } from "~/i18n";
import AppHeader from "~/components/AppHeader.vue";
import TaskSidebar from "~/components/sidebar/TaskSidebar.vue";
import LayerSidebar from "~/components/sidebar/LayerSidebar.vue";
import DownloadSetupPanel from "~/components/sidebar/DownloadSetupPanel.vue";
import MapContainer from "~/components/map/MapContainer.vue";
import TilePreviewLayer from "~/components/map/TilePreviewLayer.vue";
import BoundsRectOverlay from "~/components/map/BoundsRectOverlay.vue";
import AreaDraw from "~/components/map/AreaDraw.vue";
import TileGrid from "~/components/map/TileGrid.vue";
import TaskDetail from "~/components/sidebar/TaskDetail.vue";
import TaskBoundsOverlay from "~/components/map/TaskBoundsOverlay.vue";
import LocalTaskTileLayer from "~/components/map/LocalTaskTileLayer.vue";
import DownloadProgressLayer from "~/components/map/DownloadProgressLayer.vue";
import FailedTilesLayer from "~/components/map/FailedTilesLayer.vue";
import ClipProgressLayer from "~/components/map/ClipProgressLayer.vue";
import SplashScreen from "~/components/SplashScreen.vue";
import DisclaimerDialog from "~/components/DisclaimerDialog.vue";
import CloseActionDialog from "~/components/CloseActionDialog.vue";
import PublishPanel from "~/components/sidebar/PublishPanel.vue";
import RemotePanel from "~/components/sidebar/RemotePanel.vue";
import SettingsPanel from "~/components/sidebar/SettingsPanel.vue";
import HelpPanel from "~/components/sidebar/HelpPanel.vue";
import AboutPanel from "~/components/sidebar/AboutPanel.vue";
import NewTaskWizard from "~/components/wizard/NewTaskWizard.vue";
import ToastContainer from "~/components/ToastContainer.vue";
import { useSettingsStore } from "~/composables/useSettingsStore";

// ─── 启动时缓存设置（单次 IPC，供 onMounted 和 onSplashDone 共用） ──────────
let cachedSettings: Record<string, string> = {};

const { t } = useI18n();

// ─── 免责声明 ──────────────────────────────────────────────────────────────
const showDisclaimer = ref(false);

async function onDisclaimerAgree() {
  showDisclaimer.value = false;
  await invoke("set_setting", { key: "app.disclaimer_agreed", value: "true" });
}

async function onDisclaimerDisagree() {
  await invoke("quit_app");
}

// ─── 关闭动作对话框 ────────────────────────────────────────────────────────
const showCloseDialog = ref(false);
let pendingCloseEvent: { preventDefault: () => void } | null = null;

async function handleCloseRequest(event?: { preventDefault: () => void }) {
  if (event) {
    event.preventDefault();
    pendingCloseEvent = event;
  }
  const action = await invoke<string | null>("get_setting", {
    key: "app.close_action",
  });
  if (action === "quit") {
    await invoke("quit_app");
  } else if (action === "tray") {
    await minimizeToTray();
  } else {
    showCloseDialog.value = true;
  }
}

async function applyFloatWindowSetting(enabled: string | undefined) {
  const floatWin = await WebviewWindow.getByLabel("float");
  if (enabled === "true") {
    await floatWin?.show();
  } else {
    await floatWin?.hide();
  }
}

async function minimizeToTray() {
  await getCurrentWindow().hide();
}

async function onCloseDialogConfirm(
  action: "quit" | "tray",
  remember: boolean,
) {
  showCloseDialog.value = false;
  if (remember) {
    await invoke("set_setting", { key: "app.close_action", value: action });
  }
  if (action === "quit") {
    await invoke("quit_app");
  } else {
    await minimizeToTray();
  }
}

function onCloseDialogCancel() {
  showCloseDialog.value = false;
  pendingCloseEvent = null;
}

// ─── 闪屏 ──────────────────────────────────────────────────────────────────
const showSplash = ref(true);
const appVisible = ref(false);

let unlistenClose: (() => void) | null = null;
let unlistenNetwork: (() => void) | null = null;
let unlistenDiskFull: (() => void) | null = null;

// ─── 网络状态横幅 ──────────────────────────────────────────────────────────
const networkOffline = ref(false);

// ─── 磁盘满警告 Toast ─────────────────────────────────────────────────────
const diskFullWarning = ref(false);
let diskFullTimer: ReturnType<typeof setTimeout> | null = null;

function showDiskFullWarning() {
  diskFullWarning.value = true;
  if (diskFullTimer) clearTimeout(diskFullTimer);
  diskFullTimer = setTimeout(() => { diskFullWarning.value = false; }, 8000);
}

onMounted(async () => {
  // 单次 IPC 获取所有设置：语言立即应用，其余缓存供 onSplashDone 使用
  try {
    cachedSettings = await invoke<Record<string, string>>("get_all_settings");
    const lang = cachedSettings["app.language"];
    if (lang && lang !== "auto" && lang !== "") {
      i18n.global.locale.value = lang as "zh-CN" | "en";
    }
  } catch {
    // ignore - fallback to navigator.language detection already done in i18n.ts
  }
  // 并行：注册关闭事件监听 + 后台更新检查监听 + 网络状态监听 + 磁盘满监听
  const [unlistenFn, , unlistenNetFn, unlistenDiskFn] = await Promise.all([
    getCurrentWindow().onCloseRequested((event) => handleCloseRequest(event)),
    initUpdateListener(),
    listen<{ online: boolean }>("tilegrab-network-status", (ev) => {
      networkOffline.value = !ev.payload.online;
    }),
    listen<{ task_id: string }>("tilegrab-disk-full", () => {
      showDiskFullWarning();
    }),
  ]);
  unlistenClose = unlistenFn;
  unlistenNetwork = unlistenNetFn;
  unlistenDiskFull = unlistenDiskFn;
});

onUnmounted(() => {
  unlistenClose?.();
  unlistenNetwork?.();
  unlistenDiskFull?.();
  destroyUpdateListener();
});

async function onSplashDone() {
  // splash 内部 CSS 动画已在 3.8s 完成淡出，直接移除节点并显示主界面
  // 不额外延迟，避免透明 DOM 仍覆盖 titlebar 导致短时间无法拖动窗口
  appVisible.value = true;
  showSplash.value = false;
  // 使用已缓存设置，无需额外 IPC 调用
  await applyFloatWindowSetting(cachedSettings["app.float_window"]);
  if (cachedSettings["app.disclaimer_agreed"] !== "true") {
    showDisclaimer.value = true;
  }
}

// ─── 布局 & 导航 ────────────────────────────────────────────────────────────
const sidebarOpen = ref(true);
const activeNav = ref("map");
const { settingsDirty } = useSettingsStore();

function onNavChange(key: string) {
  // 离开设置页时检查未保存更改
  if (activeNav.value === "settings" && key !== "settings" && settingsDirty.value) {
    const ok = window.confirm(t("settings.leaveConfirm"));
    if (!ok) return;
    settingsDirty.value = false; // 用户选择放弃
  }
  const prevNav = activeNav.value;
  // 再次点击同一项则切回地图视图
  activeNav.value = activeNav.value === key ? "map" : key;
  if (["publish", "remote", "settings", "help", "about", "tasks", "map"].includes(key))
    sidebarOpen.value = true;
  // 离开地图 tab 时清除图层预览
  if (prevNav === "map" && activeNav.value !== "map") {
    selectedLayerSource.value = null;
  }
  // 离开任务 tab 时关闭任务详情
  if (prevNav === "tasks" && activeNav.value !== "tasks") {
    closeTask();
  }
}

// ─── 向导 ──────────────────────────────────────────────────────────────────
const { showWizard, openWizard, closeWizard } = useWizardState();
const previewSource = ref<TileSource | null>(null);
const downloadSetupMode = computed(() => !!previewSource.value);

// ─── F2 扩展任务（地图绘制模式） ────────────────────────────────────────────
const {
  context: extendCtx,
  isActive: extendActive,
  cancel: cancelExtend,
  unionBounds,
} = useExtendMode();
const extendBusy = ref(false);
const extendError = ref<string>("");
const { addToast: pushToast2 } = useToast();

async function confirmExtend() {
  if (!extendCtx.value) return;
  if (!drawnBounds.value) {
    extendError.value = t("extendMode.errorNoArea");
    return;
  }
  extendBusy.value = true;
  extendError.value = "";
  try {
    const merged = unionBounds(extendCtx.value.originalBounds, drawnBounds.value);
    const [added] = await invoke<[number, number]>("update_task_geometry", {
      args: {
        taskId: extendCtx.value.taskId,
        boundsWest: merged.west,
        boundsEast: merged.east,
        boundsSouth: merged.south,
        boundsNorth: merged.north,
        minZoom: extendCtx.value.originalMinZoom,
        maxZoom: extendCtx.value.originalMaxZoom,
        polygonWgs84: null,
      },
    });
    pushToast2(t("extendMode.success", { added }), "success");
    cancelExtend();
    clearDrawn();
  } catch (e: any) {
    extendError.value = String(e);
  } finally {
    extendBusy.value = false;
  }
}

function abortExtend() {
  cancelExtend();
  clearDrawn();
  extendError.value = "";
}

// 区分向导上下文：task = 创建下载任务，layer = 添加图层
const wizardContext = ref<"task" | "layer">("task");
const layerSidebarRef = ref<InstanceType<typeof LayerSidebar> | null>(null);

// 图层侧边栏选中的图层 source（仅地图Tab生效）
const selectedLayerSource = ref<TileSource | null>(null);

// 地图实际显示的 source：下载配置模式优先，其次是图层选中
const mapPreviewSource = computed<TileSource | null>(
  () => previewSource.value ?? selectedLayerSource.value,
);

function openTaskWizard() {
  wizardContext.value = "task";
  openWizard();
}

function openLayerWizard() {
  wizardContext.value = "layer";
  openWizard();
}

function onWizardConfirm(source: TileSource) {
  if (wizardContext.value === "layer") {
    onWizardConfirmLayer(source);
  } else {
    // 原有：创建下载任务流程
    setTimeout(() => {
      previewSource.value = source;
      closeWizard();
      nextTick(() => areaDrawRef.value?.toggle());
    }, 0);
  }
}

async function onWizardConfirmLayer(source: TileSource) {
  closeWizard();
  try {
    await invoke("create_layer", {
      newLayer: {
        name: source.name,
        sourceConfig: JSON.stringify(source),
      },
    });
    await layerSidebarRef.value?.loadLayers();
  } catch (e) {
    console.error("[App] create_layer failed:", e);
  }
}

// ─── 图层侧边栏事件处理 ─────────────────────────────────────────────────────

function onLayerSelect(sourceConfig: string | null) {
  if (!sourceConfig) {
    selectedLayerSource.value = null;
    return;
  }
  try {
    selectedLayerSource.value = JSON.parse(sourceConfig) as TileSource;
  } catch (e) {
    console.error("[App] onLayerSelect parse failed:", e);
    selectedLayerSource.value = null;
  }
}

function onLayerDownload(layerName: string, sourceConfig: string) {
  try {
    const source = JSON.parse(sourceConfig) as TileSource;
    // 用用户设置的图层名覆盖数据源原始名称
    source.name = layerName;
    // 切换到地图 tab，进入下载配置模式
    activeNav.value = "map";
    setTimeout(() => {
      previewSource.value = source;
      selectedLayerSource.value = null;
      nextTick(() => areaDrawRef.value?.toggle());
    }, 0);
  } catch (e) {
    console.error("[App] onLayerDownload parse failed:", e);
  }
}

function exitSetupMode() {
  previewSource.value = null;
  clearDrawn();
}

// ─── 地图 ──────────────────────────────────────────────────────────────────
const mapRef = ref<MaplibreMap | null>(null);

function onMapReady(map: MaplibreMap) {
  mapRef.value = map;
}

// ─── 绘制区域 ───────────────────────────────────────────────────────────────
const areaDrawRef = ref<InstanceType<typeof AreaDraw> | null>(null);
const drawActive = ref(false);
const drawnBounds = ref<Bounds | null>(null);
const drawnPolygon = ref<[number, number][] | null>(null);
const drawMode = ref<"rectangle" | "polygon">("rectangle");
const isImportedBounds = ref(false);

function toggleDraw() {
  areaDrawRef.value?.toggle();
}

function onBoundsChange(bounds: Bounds) {
  drawnBounds.value = bounds;
  isImportedBounds.value = false;
}

function onPolygonChange(polygon: [number, number][] | null) {
  drawnPolygon.value = polygon;
}

function onDrawActiveChange(active: boolean) {
  drawActive.value = active;
}

function clearDrawn() {
  areaDrawRef.value?.clear();
  drawnBounds.value = null;
  drawnPolygon.value = null;
  isImportedBounds.value = false;
  activeTaskId.value = null;
}

function onDrawModeChange(mode: "rectangle" | "polygon") {
  drawMode.value = mode;
  // 如果当前正在绘制，切换模式👉 由 AreaDraw 内的 watch 负责实时切换
}

function onImportBounds(bounds: Bounds, polygon: [number, number][] | null) {
  areaDrawRef.value?.clear();
  drawnBounds.value = bounds;
  drawnPolygon.value = polygon ?? null;
  isImportedBounds.value = true;
  // 如果地图已就绪，气飞到导入范围
  if (mapRef.value) {
    mapRef.value.fitBounds(
      [
        [bounds.west, bounds.south],
        [bounds.east, bounds.north],
      ],
      { padding: 60, duration: 700 },
    );
  }
}

// ─── 层级范围 ──────────────────────────────────────────────────────────────
const gridZoom = computed(() => 14);
const currentCrs = computed<CrsType>(
  () => previewSource.value?.crs ?? "WEB_MERCATOR",
);
const showGrid = computed(() => !!drawnBounds.value && !!mapRef.value);

// ─── 任务创建 + 下载 ────────────────────────────────────────────────────────
const activeTaskId = ref<string | null>(null);

async function createAndDownload(config: {
  name: string;
  minZoom: number;
  maxZoom: number;
  clipToBounds: boolean;
  remoteTarget?: { url: string; token: string; name: string } | null;
}) {
  if (!drawnBounds.value || !previewSource.value) return;
  const source = previewSource.value;
  const name = config.name || `${source.name} Z${config.minZoom}-${config.maxZoom}`;

  if (config.remoteTarget) {
    // ── 提交到远端 ──────────────────────────────────────────────────────────
    try {
      const { url, token } = config.remoteTarget;
      const resp = await fetch(`${url.replace(/\/$/, "")}/remote/tasks`, {
        method: "POST",
        headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` },
        body: JSON.stringify({
          name,
          sourceConfig: JSON.stringify(source),
          boundsWest: drawnBounds.value.west,
          boundsEast: drawnBounds.value.east,
          boundsSouth: drawnBounds.value.south,
          boundsNorth: drawnBounds.value.north,
          minZoom: config.minZoom,
          maxZoom: config.maxZoom,
          clipToBounds: config.clipToBounds,
          polygonWgs84: drawnPolygon.value
            ? JSON.stringify(drawnPolygon.value)
            : null,
        }),
      });
      if (!resp.ok) throw new Error(`HTTP ${resp.status}: ${await resp.text()}`);
      exitSetupMode();
      activeNav.value = "remote";
      sidebarOpen.value = true;
    } catch (e: unknown) {
      console.error("[App] remote submit failed:", e);
    }
    return;
  }

  // ── 本地下载 ────────────────────────────────────────────────────────────
  try {
    const taskId = await invoke<string>("create_task", {
      newTask: {
        name,
        sourceConfig: JSON.stringify(source),
        boundsWest: drawnBounds.value.west,
        boundsEast: drawnBounds.value.east,
        boundsSouth: drawnBounds.value.south,
        boundsNorth: drawnBounds.value.north,
        minZoom: config.minZoom,
        maxZoom: config.maxZoom,
        clipToBounds: config.clipToBounds,
        polygonWgs84: drawnPolygon.value
          ? JSON.stringify(drawnPolygon.value)
          : null,
      },
    });
    activeTaskId.value = taskId;
    await invoke("start_download", { taskId });
  } catch (e: unknown) {
    console.error("[App] createAndDownload failed:", e);
  }
  // 关闭下载配置面板，打开任务列表
  exitSetupMode();
  activeNav.value = "tasks";
  sidebarOpen.value = true;
}

// ─── 任务详情 + 地图定位 ────────────────────────────────────────────────────
const { selectedTaskId, selectedTaskStatus, openTask, closeTask } =
  useTaskDetail();

const sidebarRef = ref<InstanceType<typeof TaskSidebar> | null>(null);

function handleOpenTask(id: string) {
  openTask(id);
  sidebarOpen.value = true;
}

function handleDetailFlyTo(bounds: {
  west: number;
  east: number;
  south: number;
  north: number;
}) {
  if (!mapRef.value) return;
  const lngLatBounds: LngLatBoundsLike = [
    [bounds.west, bounds.south],
    [bounds.east, bounds.north],
  ];
  mapRef.value.fitBounds(lngLatBounds, { padding: 80, duration: 800 });
}

function handleDetailDeleted() {
  closeTask();
  sidebarRef.value?.loadTasks();
}
</script>

<template>
  <!-- 免责声明对话框 -->
  <DisclaimerDialog
    :open="showDisclaimer"
    @agree="onDisclaimerAgree"
    @disagree="onDisclaimerDisagree"
  />

  <!-- 关闭行为对话框 -->
  <CloseActionDialog
    :open="showCloseDialog"
    @confirm="onCloseDialogConfirm"
    @cancel="onCloseDialogCancel"
  />

  <!-- 电影闪屏 -->
  <Transition name="splash-out">
    <SplashScreen v-if="showSplash" @done="onSplashDone" />
  </Transition>

  <!-- 主界面（闪屏消失后浮现） -->
  <div
    class="flex h-screen flex-col overflow-hidden"
    style="background: var(--color-app-bg)"
    :class="appVisible ? 'app-visible' : 'app-hidden'"
  >
    <!-- 顶部导航栏 -->
    <AppHeader
      v-model:sidebar-open="sidebarOpen"
      :active-nav="activeNav"
      @nav-change="onNavChange"
    />

    <!-- 网络离线横幅 -->
    <Transition name="banner-slide">
      <div
        v-if="networkOffline"
        class="flex items-center gap-2 px-4 py-2 text-xs font-medium shrink-0 z-50"
        style="background: #fef9c3; color: #854d0e; border-bottom: 1px solid #fde047"
      >
        <WifiOff class="size-3.5 shrink-0" />
        <span>{{ t('app.networkOffline') }}</span>
      </div>
    </Transition>

    <!-- 磁盘空间不足警告 -->
    <Transition name="banner-slide">
      <div
        v-if="diskFullWarning"
        class="flex items-center gap-2 px-4 py-2 text-xs font-medium shrink-0 z-50"
        style="background: #fff1f2; color: #be123c; border-bottom: 1px solid #fda4af"
      >
        <HardDrive class="size-3.5 shrink-0" />
        <span>{{ t('app.diskFull') }}</span>
      </div>
    </Transition>

    <!-- 主体区域 -->
    <div class="flex flex-1 min-h-0 overflow-hidden relative">
      <!-- ── 功能面板：覆盖在主体上方，地图始终保持渲染 ──────────────── -->
      <Transition name="panel-fade">
        <div
          v-if="
            activeNav === 'publish' ||
            activeNav === 'remote' ||
            activeNav === 'settings' ||
            activeNav === 'help' ||
            activeNav === 'about'
          "
          class="absolute inset-0 z-20 flex flex-col overflow-hidden"
          style="background: var(--color-app-bg)"
        >
          <PublishPanel
            v-if="activeNav === 'publish'"
            class="flex-1 flex flex-col overflow-hidden"
          />
          <RemotePanel
            v-else-if="activeNav === 'remote'"
            class="flex-1 flex flex-col overflow-hidden"
            @start-task="activeNav = 'map'"
          />
          <SettingsPanel
            v-else-if="activeNav === 'settings'"
            class="flex-1 flex flex-col overflow-hidden"
          />
          <HelpPanel
            v-else-if="activeNav === 'help'"
            class="flex-1 flex flex-col overflow-hidden"
          />
          <AboutPanel
            v-else-if="activeNav === 'about'"
            class="flex-1 flex flex-col overflow-hidden"
          />
        </div>
      </Transition>

      <!-- ── 左侧面板：下载配置 / 图层列表 / 任务列表 ────────────── -->
      <DownloadSetupPanel
        v-if="downloadSetupMode && previewSource"
        :source="previewSource"
        :bounds="drawnBounds"
        :draw-active="drawActive"
        @close="exitSetupMode"
        @toggle-draw="toggleDraw"
        @clear-bounds="clearDrawn"
        @start-download="createAndDownload"
        @draw-mode-change="onDrawModeChange"
        @import-bounds="onImportBounds"
      />
      <!-- 地图 Tab：图层列表 -->
      <LayerSidebar
        v-else-if="activeNav === 'map'"
        ref="layerSidebarRef"
        v-model:open="sidebarOpen"
        @add-layer="openLayerWizard"
        @select-layer="onLayerSelect"
        @download-layer="onLayerDownload"
      />
      <!-- 任务 Tab：任务列表（包裹在 relative 容器中，任务详情可覆盖其上） -->
      <div v-else-if="activeNav === 'tasks'" class="relative shrink-0 h-full overflow-hidden">
        <TaskSidebar
          ref="sidebarRef"
          v-model:open="sidebarOpen"
          @new-task="openTaskWizard"
          @open-task="handleOpenTask"
        />
        <!-- 任务详情覆盖面板：从左侧滑入，覆盖任务列表 -->
        <Transition name="slide-from-left">
          <TaskDetail
            v-if="selectedTaskId"
            :task-id="selectedTaskId"
            class="absolute inset-0 z-10"
            @close="closeTask"
            @deleted="handleDetailDeleted"
            @fly-to="handleDetailFlyTo"
          />
        </Transition>
      </div>

      <!-- ── 地图区域（始终渲染，不随导航销毁） ────────────────────── -->
      <main class="flex-1 min-w-0 relative overflow-hidden">
        <div class="relative w-full h-full">
          <MapContainer @ready="onMapReady" />

          <!-- ─── 地图工具栏（下载配置 + 扩展任务） ─── -->
          <div
            v-if="(downloadSetupMode || extendActive) && mapRef"
            class="absolute top-3 left-3 z-10 flex flex-col gap-1.5"
          >
            <button
              :title="drawActive ? '取消框选' : '框选区域'"
              class="flex items-center justify-center size-9 rounded-lg shadow-sm border text-sm font-medium transition-colors"
              :class="
                drawActive
                  ? 'bg-blue-600 border-blue-700 text-white'
                  : 'bg-white border-slate-200 text-slate-600 hover:bg-slate-50'
              "
              @click="toggleDraw"
            >
              <X v-if="drawActive" class="size-4" />
              <BoxSelect v-else class="size-4" />
            </button>

            <button
              v-if="drawnBounds && !drawActive"
              title="清除选区"
              class="flex items-center justify-center size-9 rounded-lg shadow-sm border bg-white border-slate-200 text-slate-500 hover:bg-red-50 hover:text-red-500 hover:border-red-200 text-sm transition-colors"
              @click="clearDrawn"
            >
              <Trash2 class="size-4" />
            </button>
          </div>

          <!-- ─── F2 扩展任务 顶部横幅 ─── -->
          <div
            v-if="extendActive && extendCtx && mapRef"
            class="absolute top-3 left-1/2 -translate-x-1/2 z-20 flex items-center gap-3 px-4 py-2 rounded-lg shadow-lg bg-white border border-blue-200"
          >
            <div class="flex flex-col">
              <span class="text-xs font-semibold text-blue-700">
                {{ t("extendMode.banner", { name: extendCtx.taskName }) }}
              </span>
              <span class="text-[11px] text-slate-500">
                {{ t("extendMode.hint") }}
              </span>
              <span v-if="extendError" class="text-[11px] text-red-600">{{ extendError }}</span>
            </div>
            <button
              class="px-3 py-1 text-xs rounded-md border border-slate-200 text-slate-600 hover:bg-slate-50"
              @click="abortExtend"
            >
              {{ t("extendMode.cancel") }}
            </button>
            <button
              class="px-3 py-1 text-xs rounded-md bg-blue-600 text-white hover:bg-blue-700 disabled:opacity-50"
              :disabled="extendBusy || !drawnBounds"
              @click="confirmExtend"
            >
              {{ extendBusy ? t("extendMode.running") : t("extendMode.confirm") }}
            </button>
          </div>

          <!-- ─── 非可视组件 ─── -->
          <TilePreviewLayer
            v-if="mapRef"
            :map="mapRef"
            :source="mapPreviewSource"
          />

          <AreaDraw
            v-if="mapRef"
            ref="areaDrawRef"
            :map="mapRef"
            :mode="drawMode"
            @bounds-change="onBoundsChange"
            @polygon-change="onPolygonChange"
            @active-change="onDrawActiveChange"
          />

          <!-- 导入 KML/GeoJSON 时的范围可视化覆盖层 -->
          <BoundsRectOverlay
            v-if="mapRef"
            :map="mapRef"
            :bounds="isImportedBounds ? drawnBounds : null"
            :polygon="isImportedBounds ? drawnPolygon : null"
          />

          <!-- 扩展任务模式：显示原始区域轮廓 -->
          <BoundsRectOverlay
            v-if="mapRef && extendActive && extendCtx"
            :map="mapRef"
            :bounds="extendCtx.originalBounds"
            :polygon="extendCtx.originalPolygon"
          />

          <TileGrid
            v-if="mapRef && showGrid"
            :map="mapRef"
            :bounds="drawnBounds"
            :zoom="gridZoom"
            :crs="currentCrs"
            :visible="showGrid"
          />

          <TaskBoundsOverlay
            v-if="mapRef"
            :map="mapRef"
            :task-id="selectedTaskId"
          />

          <!-- 选中任务时在地图叠加已下载的瓦片图层 -->
          <!-- 下载中/暂停：显示逐瓦片进度格网 -->
          <DownloadProgressLayer
            v-if="
              mapRef &&
              selectedTaskId &&
              (selectedTaskStatus === 'downloading' ||
                selectedTaskStatus === 'paused')
            "
            :map="mapRef"
            :task-id="selectedTaskId"
            :task-status="selectedTaskStatus"
          />
          <!-- 裁剪中：紫色高亮正在处理的瓦片 -->
          <ClipProgressLayer
            v-if="mapRef && selectedTaskId"
            :map="mapRef"
            :task-id="selectedTaskId"
            :task-status="selectedTaskStatus"
          />
          <!-- 其他状态：显示实际瓦片内容 -->
          <LocalTaskTileLayer
            v-if="
              mapRef &&
              !(
                selectedTaskStatus === 'downloading' ||
                selectedTaskStatus === 'paused' ||
                selectedTaskStatus === 'processing'
              )
            "
            :map="mapRef"
            :task-id="selectedTaskId"
          />
          <!-- E 套件：失败瓦片可视化（用户在 TaskDetail 选层级触发） -->
          <FailedTilesLayer v-if="mapRef" :map="mapRef" />

          <NewTaskWizard
            v-if="showWizard"
            @confirm="onWizardConfirm"
            @close="closeWizard"
          />
        </div>
      </main>
    </div>
  </div>
  <ToastContainer />
</template>

<style scoped>
.app-hidden {
  opacity: 0;
  pointer-events: none;
}
.app-visible {
  opacity: 1;
  transition: opacity 0.6s ease-out;
}

.splash-out-leave-active {
  /* splash 内部动画已完成，无需额外等待，立即移除 DOM */
  transition: none;
}

.banner-slide-enter-active,
.banner-slide-leave-active {
  transition: max-height 0.25s ease, opacity 0.25s ease;
  overflow: hidden;
}
.banner-slide-enter-from,
.banner-slide-leave-to {
  max-height: 0;
  opacity: 0;
}
.banner-slide-enter-to,
.banner-slide-leave-from {
  max-height: 40px;
  opacity: 1;
}
</style>
