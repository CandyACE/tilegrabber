<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch } from "vue";
import { useI18n } from "vue-i18n";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import {
  Save,
  RotateCcw,
  Settings2,
  Download,
  Server,
  ShieldAlert,
  RefreshCw,
  ArrowUpCircle,
  FolderOpen,
} from "lucide-vue-next";
import { open as openDirectoryPicker } from "@tauri-apps/plugin-dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import RulesConfig from "./RulesConfig.vue";
import { useUpdateState } from "~/composables/useUpdateState";

const { t, locale } = useI18n()

// ─── 类型 ────────────────────────────────────────────────────────────────────
type Settings = Record<string, string>;

// ─── 分组定义 ─────────────────────────────────────────────────────────────────
const cpuCores = navigator.hardwareConcurrency || 4
const suggestedConcurrency = Math.max(8, Math.min(32, cpuCores * 2))

const groups = computed(() => [
  {
    id: 'app',
    label: t('settings.groups.app'),
    icon: Settings2,
    fields: [
      {
        key: 'app.tiles_dir',
        label: t('settings.fields.app_tiles_dir.label'),
        type: 'path',
        hint: t('settings.fields.app_tiles_dir.hint'),
      },
      {
        key: 'app.float_window',
        label: t('settings.fields.app_float_window.label'),
        type: 'toggle',
        hint: t('settings.fields.app_float_window.hint'),
      },
      {
        key: 'app.close_action',
        label: t('settings.fields.app_close_action.label'),
        type: 'select',
        hint: t('settings.fields.app_close_action.hint'),
        options: [
          { value: 'ask', label: t('settings.fields.app_close_action.optAsk') },
          { value: 'tray', label: t('settings.fields.app_close_action.optTray') },
          { value: 'quit', label: t('settings.fields.app_close_action.optQuit') },
        ],
      },
      {
        key: 'app.language',
        label: t('settings.fields.app_language.label'),
        type: 'select',
        hint: t('settings.fields.app_language.hint'),
        options: [
          { value: 'auto', label: t('settings.fields.app_language.optAuto') },
          { value: 'zh-CN', label: '中文' },
          { value: 'en', label: 'English' },
        ],
      },
    ],
  },
  {
    id: 'download',
    label: t('settings.groups.download'),
    icon: Download,
    fields: [
      { key: 'download.concurrency', label: t('settings.fields.download_concurrency.label'), type: 'number', min: 1, max: 64, hint: t('settings.fields.download_concurrency.hint', { cores: cpuCores, suggested: suggestedConcurrency }) },
      { key: 'download.timeout_secs', label: t('settings.fields.download_timeout_secs.label'), type: 'number', min: 5, max: 120, hint: t('settings.fields.download_timeout_secs.hint') },
      { key: 'download.max_retries', label: t('settings.fields.download_max_retries.label'), type: 'number', min: 0, max: 10, hint: t('settings.fields.download_max_retries.hint') },
      { key: 'download.retry_delay_ms', label: t('settings.fields.download_retry_delay_ms.label'), type: 'number', min: 100, max: 10000, hint: t('settings.fields.download_retry_delay_ms.hint') },
      { key: 'download.delay_min_ms', label: t('settings.fields.download_delay_min_ms.label'), type: 'number', min: 0, max: 5000, hint: t('settings.fields.download_delay_min_ms.hint') },
      { key: 'download.delay_max_ms', label: t('settings.fields.download_delay_max_ms.label'), type: 'number', min: 0, max: 5000, hint: t('settings.fields.download_delay_max_ms.hint') },
    ],
  },
  {
    id: 'server',
    label: t('settings.groups.server'),
    icon: Server,
    fields: [
      { key: 'server.default_port', label: t('settings.fields.server_default_port.label'), type: 'number', min: 1024, max: 65535, hint: t('settings.fields.server_default_port.hint') },
    ],
  },
])

// ─── 状态 ────────────────────────────────────────────────────────────────────
const settings = ref<Settings>({});
const saving = ref(false);
const saved = ref(false);
const errorMsg = ref("");

onMounted(async () => {
  await loadSettings();
});

async function loadSettings() {
  try {
    const result = await invoke<Settings>("get_all_settings");
    // 兼容旧版本空字符串存储
    if (!result["app.close_action"]) result["app.close_action"] = "ask";
    settings.value = result;
    // Apply saved language setting
    const lang = result['app.language']
    if (lang && lang !== 'auto' && lang !== '') {
      locale.value = lang
    }
  } catch (e) {
    errorMsg.value = String(e);
  }
}

async function saveSettings() {
  saving.value = true;
  errorMsg.value = "";
  try {
    await invoke("set_all_settings", { settings: settings.value });
    // Apply language change immediately
    const lang = settings.value['app.language']
    if (lang && lang !== 'auto' && lang !== '') {
      locale.value = lang
    } else if (!lang || lang === 'auto') {
      const nav = navigator.language || ''
      locale.value = nav.startsWith('zh') ? 'zh-CN' : 'en'
    }
    // 即时应用悬浮窗开关
    const floatWin = await WebviewWindow.getByLabel("float");
    if (settings.value["app.float_window"] === "true") {
      await floatWin?.show();
    } else {
      await floatWin?.hide();
    }
    saved.value = true;
    setTimeout(() => {
      saved.value = false;
    }, 2000);
  } catch (e) {
    errorMsg.value = String(e);
  } finally {
    saving.value = false;
  }
}

async function resetDefaults() {
  try {
    // 删除所有自定义设置，重新加载（后端会返回默认值）
    const empties: Settings = {};
    for (const k of Object.keys(settings.value)) {
      empties[k] = "";
    }
    await invoke("set_all_settings", { settings: empties });
    await loadSettings();
  } catch (e) {
    errorMsg.value = String(e);
  }
}

// ─── 路径选择 ─────────────────────────────────────────────
async function pickFolder(key: string) {
  const selected = await openDirectoryPicker({
    directory: true,
    multiple: false,
  });
  if (selected) {
    settings.value = { ...settings.value, [key]: selected as string };
  }
}

// ─── 辅助 ────────────────────────────────────────────────────────────────────
function numVal(key: string): number {
  return Number(settings.value[key] ?? 0);
}
function setNum(key: string, v: string) {
  settings.value = { ...settings.value, [key]: v };
}
function toggleVal(key: string): boolean {
  return settings.value[key] === "true";
}
function setToggle(key: string, v: boolean) {
  settings.value = { ...settings.value, [key]: v ? "true" : "false" };
}

// ─── 自动更新 ──────────────────────────────────────────────────────────────────────────────
import { marked } from "marked";

function renderMarkdown(md: string): string {
  return marked.parse(md, { async: false }) as string;
}

interface UpdateCheckResult {
  currentVersion: string;
  latestVersion: string | null;
  hasUpdate: boolean;
  releaseUrl: string | null;
  downloadUrl: string | null;
  releaseNotes: string | null;
  error: string | null;
}

const updateChecking = ref(false);
const updateResult = ref<UpdateCheckResult | null>(null);
const updateError = ref("");
// 标记结果是否来自后台自动检测
const updateFromAutoCheck = ref(false);

// 接收后台静默检查结果：面板挂载时若已有结果则直接填充
const { autoCheckResult, setUpdateResult } = useUpdateState();

onMounted(() => {
  if (autoCheckResult.value && !updateResult.value) {
    updateResult.value = autoCheckResult.value;
    updateFromAutoCheck.value = true;
  }
});

// 若后台检查在面板已打开后才返回，也能即时更新
watch(autoCheckResult, (val) => {
  if (val && !updateResult.value) {
    updateResult.value = val;
    updateFromAutoCheck.value = true;
  }
});

// 下载安装状态
const downloading = ref(false);
const downloadPercent = ref(0);
const downloadedBytes = ref(0);
const totalBytes = ref(0);
const installError = ref("");
let unlistenProgress: UnlistenFn | null = null;

onUnmounted(() => {
  unlistenProgress?.();
});

async function checkUpdate() {
  updateChecking.value = true;
  updateResult.value = null;
  updateError.value = "";
  updateFromAutoCheck.value = false;
  try {
    const result = await invoke<UpdateCheckResult>("check_for_update");
    updateResult.value = result;
    setUpdateResult(result);
  } catch (e) {
    updateError.value = String(e);
  } finally {
    updateChecking.value = false;
  }
}

async function startDownloadAndInstall(url: string) {
  downloading.value = true;
  downloadPercent.value = 0;
  downloadedBytes.value = 0;
  totalBytes.value = 0;
  installError.value = "";

  // 监听进度事件
  unlistenProgress?.();
  unlistenProgress = await listen<{
    downloaded: number;
    total: number;
    percent: number;
  }>("update-download-progress", (event) => {
    downloadPercent.value = event.payload.percent;
    downloadedBytes.value = event.payload.downloaded;
    totalBytes.value = event.payload.total;
  });

  try {
    await invoke("download_and_install_update", { url });
    // 正常情况下 invoke 返回后 app 已 exit，此行不会执行
  } catch (e) {
    installError.value = String(e);
    downloading.value = false;
  } finally {
    unlistenProgress?.();
    unlistenProgress = null;
  }
}

function formatBytes(bytes: number): string {
  if (bytes === 0) return "0 B";
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}
</script>

<template>
  <div class="flex flex-col h-full overflow-y-auto">
    <div class="flex flex-col gap-4 w-full max-w-2xl mx-auto px-6 py-6 text-sm">
      <!-- 标题栏 -->
      <div class="flex items-center gap-2 px-0.5">
        <Settings2 :size="14" class="text-slate-400 shrink-0" />
        <span
          class="text-xs font-semibold text-slate-600 tracking-wide uppercase"
          >{{ t('settings.title') }}</span
        >
        <div class="ml-auto flex items-center gap-1.5">
          <button
            @click="resetDefaults"
            class="flex items-center gap-1 px-2 py-1 rounded-lg text-xs text-slate-500 hover:text-slate-700 hover:bg-slate-100 transition-colors"
            title="恢复默认"
          >
            <RotateCcw :size="11" />
            {{ t('settings.resetDefaults') }}
          </button>
          <button
            @click="saveSettings"
            :disabled="saving"
            class="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium transition-colors bg-blue-50 text-blue-600 hover:bg-blue-100 border border-blue-200 disabled:opacity-50 disabled:cursor-not-allowed"
          >
            <Save :size="11" />
            {{ saved ? t('settings.saved') : t('settings.save') }}
          </button>
        </div>
      </div>

      <!-- 错误提示 -->
      <div
        v-if="errorMsg"
        class="px-3 py-2 rounded-lg bg-red-50 border border-red-200 text-xs text-red-500"
      >
        {{ errorMsg }}
      </div>

      <!-- 分组卡片 -->
      <div
        v-for="group in groups"
        :key="group.id"
        class="rounded-xl border bg-white overflow-hidden"
        style="border-color: var(--color-border-subtle)"
      >
        <!-- 组头 -->
        <div
          class="flex items-center gap-2 px-4 py-2.5 border-b"
          style="border-color: var(--color-border-subtle)"
        >
          <component
            :is="group.icon"
            :size="13"
            class="text-slate-400 shrink-0"
          />
          <span class="text-xs font-semibold text-slate-600">{{
            group.label
          }}</span>
        </div>

        <!-- 字段列表 -->
        <div class="divide-y divide-slate-100">
          <div
            v-for="field in group.fields"
            :key="field.key"
            class="flex items-center gap-3 px-4 py-3"
          >
            <div class="flex-1 min-w-0">
              <div class="text-xs text-slate-700">{{ field.label }}</div>
              <div v-if="field.hint" class="text-xs text-slate-400 mt-0.5">
                {{ field.hint }}
              </div>
            </div>

            <!-- 路径选择 -->
            <div
              v-if="field.type === 'path'"
              class="flex items-center gap-1.5 shrink-0 max-w-[220px]"
            >
              <input
                :value="settings[field.key] ?? ''"
                @input="
                  settings[field.key] = (
                    $event.target as HTMLInputElement
                  ).value
                "
                type="text"
                :placeholder="t('settings.fields.app_tiles_dir.placeholder')"
                class="min-w-0 flex-1 px-2 py-1 rounded-md bg-slate-100 border border-slate-200 text-slate-700 text-xs focus:outline-none focus:ring-1 focus:ring-blue-500/60"
              />
              <button
                @click="pickFolder(field.key)"
                class="shrink-0 p-1.5 rounded-md bg-slate-100 border border-slate-200 text-slate-500 hover:bg-slate-200 hover:text-slate-700 transition-colors"
                :title="t('settings.selectFolder')"
              >
                <FolderOpen :size="12" />
              </button>
            </div>

            <!-- 数字输入 -->
            <input
              v-if="field.type === 'number'"
              :value="numVal(field.key)"
              @input="
                setNum(field.key, ($event.target as HTMLInputElement).value)
              "
              type="number"
              :min="(field as { min?: number }).min"
              :max="(field as { max?: number }).max"
              class="w-20 shrink-0 px-2 py-1 rounded-md bg-slate-100 border border-slate-200 text-slate-700 text-xs text-right focus:outline-none focus:ring-1 focus:ring-blue-500/60"
            />

            <!-- 下拉选择 -->
            <Select
              v-else-if="field.type === 'select'"
              :model-value="settings[field.key] ?? ''"
              @update:model-value="
                (v) => v !== undefined && (settings[field.key] = v)
              "
            >
              <SelectTrigger size="sm" class="shrink-0 h-7 text-xs min-w-24">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem
                  v-for="opt in (
                    field as { options?: { value: string; label: string }[] }
                  ).options ?? []"
                  :key="opt.value"
                  :value="opt.value"
                  class="text-xs"
                >
                  {{ opt.label }}
                </SelectItem>
              </SelectContent>
            </Select>

            <!-- 开关 -->
            <Switch
              v-else-if="field.type === 'toggle'"
              :model-value="toggleVal(field.key)"
              @update:model-value="setToggle(field.key, $event as boolean)"
            />
          </div>
        </div>
      </div>

      <!-- ── 下载规则 ─────────────────────────────────────────────────────── -->
      <div
        class="rounded-xl border bg-white overflow-hidden"
        style="border-color: var(--color-border-subtle)"
      >
        <div
          class="flex items-center gap-2 px-4 py-2.5 border-b"
          style="border-color: var(--color-border-subtle)"
        >
          <ShieldAlert :size="13" class="text-slate-400 shrink-0" />
          <span class="text-xs font-semibold text-slate-600"
            >{{ t('settings.groups.rules') }}</span
          >
        </div>
        <div class="p-4">
          <RulesConfig :settings="settings" @change="(s) => (settings = s)" />
        </div>
      </div>

      <!-- ── 自动更新 ─────────────────────────────────────────────────────── -->
      <div
        class="rounded-xl border bg-white overflow-hidden"
        style="border-color: var(--color-border-subtle)"
      >
        <div
          class="flex items-center gap-2 px-4 py-2.5 border-b"
          style="border-color: var(--color-border-subtle)"
        >
          <ArrowUpCircle :size="13" class="text-slate-400 shrink-0" />
          <span class="text-xs font-semibold text-slate-600">{{ t('settings.groups.update') }}</span>
        </div>

        <div class="px-4 py-4 flex flex-col gap-3">
          <!-- 版本信息 + 检查按钮 -->
          <div class="flex items-center gap-3">
            <div class="flex-1 text-xs text-slate-500">
              <span>{{ t('settings.update.currentVersion') }}</span>
              <span class="font-mono font-medium text-slate-700">
                {{ updateResult?.currentVersion ?? "—" }}
              </span>
            </div>
            <button
              @click="checkUpdate"
              :disabled="updateChecking"
              class="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium bg-slate-100 text-slate-600 hover:bg-slate-200 active:bg-slate-300 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <RefreshCw
                :size="12"
                :class="{ 'animate-spin': updateChecking }"
              />
              {{ updateChecking ? t('settings.update.checking') : t('settings.update.checkAgain') }}
            </button>
          </div>

          <!-- 检查结果 -->
          <Transition name="fade-in">
            <div v-if="updateResult">
              <!-- 有更新 -->
              <div
                v-if="updateResult.hasUpdate"
                class="rounded-lg bg-blue-50 border border-blue-200 px-3 py-3 flex flex-col gap-2"
              >
                <div class="flex items-center justify-between gap-2">
                  <div class="flex items-center gap-2">
                    <ArrowUpCircle :size="14" class="text-blue-500 shrink-0" />
                    <span class="text-xs font-semibold text-blue-700">
                      {{ t('settings.update.newVersion', { version: updateResult.latestVersion }) }}
                    </span>
                  </div>
                  <!-- 来源标签 -->
                  <span
                    v-if="updateFromAutoCheck"
                    class="text-[10px] px-1.5 py-0.5 rounded-full bg-blue-100 text-blue-500 leading-none shrink-0"
                  >
                    {{ t('settings.update.autoDetected') }}
                  </span>
                </div>

                <!-- 更新说明 -->
                <div
                  v-if="updateResult.releaseNotes"
                  class="text-[11px] font-medium text-slate-500 -mb-1"
                >
                  {{ t('settings.update.releaseNotes') }}
                </div>
                <div
                  v-if="updateResult.releaseNotes"
                  class="md-release-notes text-xs text-slate-600 leading-relaxed"
                  v-html="renderMarkdown(updateResult.releaseNotes)"
                />

                <!-- 下载进度条 -->
                <div v-if="downloading" class="flex flex-col gap-1.5">
                  <div
                    class="flex items-center justify-between text-xs text-slate-500"
                  >
                    <span>{{
                      downloadPercent < 100
                        ? t('settings.update.downloading', { percent: downloadPercent })
                        : t('settings.update.downloadDone')
                    }}</span>
                    <span v-if="totalBytes > 0" class="font-mono text-[10px]"
                      >{{ formatBytes(downloadedBytes) }} /
                      {{ formatBytes(totalBytes) }}</span
                    >
                  </div>
                  <div class="h-1.5 rounded-full bg-blue-100 overflow-hidden">
                    <div
                      class="h-full rounded-full bg-blue-500 transition-all duration-200"
                      :style="{ width: `${downloadPercent}%` }"
                    />
                  </div>
                </div>

                <!-- 下载错误 -->
                <div
                  v-if="installError"
                  class="text-xs text-red-600 bg-red-50 rounded px-2 py-1"
                >
                  {{ t('settings.update.installError', { error: installError }) }}
                </div>

                <!-- 操作按钮 -->
                <div v-if="!downloading" class="flex items-center gap-2">
                  <!-- 有直链：直接在应用内下载安装 -->
                  <button
                    v-if="updateResult.downloadUrl"
                    @click="startDownloadAndInstall(updateResult.downloadUrl!)"
                    class="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium bg-blue-500 text-white hover:bg-blue-600 active:bg-blue-700 transition-colors"
                  >
                    <Download :size="12" />
                    {{ t('settings.update.downloadInstall') }}
                  </button>
                  <!-- 无直链：回退到打开浏览器 -->
                  <button
                    v-else-if="updateResult.releaseUrl"
                    @click="
                      invoke('open_release_url', {
                        url: updateResult.releaseUrl,
                      })
                    "
                    class="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium bg-blue-50 text-blue-600 hover:bg-blue-100 border border-blue-200 transition-colors"
                  >
                    <Download :size="12" />
                    {{ t('settings.update.goToDownload') }}
                  </button>
                </div>
              </div>

              <!-- 已是最新 -->
              <div
                v-else-if="!updateResult.error"
                class="rounded-lg bg-green-50 border border-green-200 px-3 py-2 flex items-center justify-between gap-2"
              >
                <div class="flex items-center gap-1.5">
                  <span class="text-green-600 text-xs">✓</span>
                  <span class="text-xs text-green-700">
                    {{ t('settings.update.upToDate', { version: updateResult.latestVersion ?? updateResult.currentVersion }) }}
                  </span>
                </div>
                <span
                  v-if="updateFromAutoCheck"
                  class="text-[10px] px-1.5 py-0.5 rounded-full bg-green-100 text-green-500 leading-none shrink-0"
                >
                  {{ t('settings.update.manualCheck') }}
                </span>
              </div>

              <!-- 检查出错 -->
              <div
                v-else
                class="rounded-lg bg-amber-50 border border-amber-200 px-3 py-2 flex items-start gap-2"
              >
                <span class="text-amber-500 text-xs mt-px shrink-0">⚠</span>
                <span class="text-xs text-amber-700">
                  {{ t('settings.update.checkError', { error: updateResult.error }) }}
                </span>
              </div>
            </div>
          </Transition>

          <!-- 接口/网络错误 -->
          <div
            v-if="updateError"
            class="rounded-lg bg-red-50 border border-red-200 px-3 py-2 flex items-start gap-2"
          >
            <span class="text-red-500 text-xs mt-px shrink-0">✕</span>
            <span class="text-xs text-red-700">
              {{ t('settings.update.networkError', { error: updateError }) }}
            </span>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.fade-in-enter-active {
  transition:
    opacity 0.2s ease,
    transform 0.2s ease;
}
.fade-in-enter-from {
  opacity: 0;
  transform: translateY(-4px);
}

/* Markdown 更新日志样式 */
.md-release-notes :deep(h1),
.md-release-notes :deep(h2),
.md-release-notes :deep(h3) {
  font-weight: 600;
  color: #1e40af;
  margin-top: 0.5em;
  margin-bottom: 0.25em;
}
.md-release-notes :deep(h1) {
  font-size: 0.85rem;
}
.md-release-notes :deep(h2) {
  font-size: 0.8rem;
}
.md-release-notes :deep(h3) {
  font-size: 0.75rem;
}
.md-release-notes :deep(ul),
.md-release-notes :deep(ol) {
  padding-left: 1.2em;
  margin: 0.25em 0;
}
.md-release-notes :deep(li) {
  margin: 0.1em 0;
  list-style-type: disc;
}
.md-release-notes :deep(p) {
  margin: 0.25em 0;
}
.md-release-notes :deep(code) {
  background: #dbeafe;
  color: #1d4ed8;
  padding: 0.1em 0.3em;
  border-radius: 3px;
  font-size: 0.9em;
  font-family: monospace;
}
.md-release-notes :deep(strong) {
  font-weight: 600;
  color: #1e3a8a;
}
</style>
