<script setup lang="ts">
import { ref, onMounted, onUnmounted } from "vue";
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

// ─── 类型 ────────────────────────────────────────────────────────────────────
type Settings = Record<string, string>;

// ─── CPU 核数（用于建议并发数） ────────────────────────────────────────────────
const cpuCores = navigator.hardwareConcurrency || 4;
const suggestedConcurrency = Math.max(8, Math.min(32, cpuCores * 2));

// ─── 分组定义 ─────────────────────────────────────────────────────────────────
const groups = [
  {
    id: "app",
    label: "应用",
    icon: Settings2,
    fields: [
      {
        key: "app.tiles_dir",
        label: "瓦片存储目录",
        type: "path",
        hint: "瓦片文件的保存位置。留空时自动存入「文档/御图/tiles」（用户目录，始终可写）",
      },
      {
        key: "app.float_window",
        label: "显示悬浮速度窗口",
        type: "toggle",
        hint: "主界面最小化到托盘后，显示可拖拽的悬浮速度窗口；双击可唤起主界面",
      },
      {
        key: "app.close_action",
        label: "点击关闭按钮时",
        type: "select",
        hint: "按下窗口关闭按钮（或 Alt+F4）时的行为",
        options: [
          { value: "ask", label: "每次询问" },
          { value: "tray", label: "最小化到系统托盘" },
          { value: "quit", label: "直接退出" },
        ],
      },
    ],
  },
  {
    id: "download",
    label: "下载配置",
    icon: Download,
    fields: [
      {
        key: "download.concurrency",
        label: "并发数",
        type: "number",
        min: 1,
        max: 64,
        hint: `同时下载的瓦片线程数（${cpuCores} 核 CPU，建议 ${suggestedConcurrency}+），过高可能触发服务器限流`,
      },
      {
        key: "download.timeout_secs",
        label: "超时（秒）",
        type: "number",
        min: 5,
        max: 120,
        hint: "单个瓦片请求的最大等待时间",
      },
      {
        key: "download.max_retries",
        label: "最大重试次数",
        type: "number",
        min: 0,
        max: 10,
        hint: "失败后自动重试的最大次数",
      },
      {
        key: "download.retry_delay_ms",
        label: "重试延迟基数（ms）",
        type: "number",
        min: 100,
        max: 10000,
        hint: "首次重试等待时间，之后指数增长",
      },
      {
        key: "download.delay_min_ms",
        label: "请求间隔下限（ms）",
        type: "number",
        min: 0,
        max: 5000,
        hint: "相邻两次瓦片请求的最小随机延迟",
      },
      {
        key: "download.delay_max_ms",
        label: "请求间隔上限（ms）",
        type: "number",
        min: 0,
        max: 5000,
        hint: "相邻两次瓦片请求的最大随机延迟",
      },
    ],
  },
  {
    id: "server",
    label: "发布服务",
    icon: Server,
    fields: [
      {
        key: "server.default_port",
        label: "默认端口",
        type: "number",
        min: 1024,
        max: 65535,
        hint: "瓦片发布服务的默认监听端口",
      },
    ],
  },
];

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
  } catch (e) {
    errorMsg.value = String(e);
  }
}

async function saveSettings() {
  saving.value = true;
  errorMsg.value = "";
  try {
    await invoke("set_all_settings", { settings: settings.value });
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
  try {
    updateResult.value = await invoke<UpdateCheckResult>("check_for_update");
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
          >应用设置</span
        >
        <div class="ml-auto flex items-center gap-1.5">
          <button
            @click="resetDefaults"
            class="flex items-center gap-1 px-2 py-1 rounded-lg text-xs text-slate-500 hover:text-slate-700 hover:bg-slate-100 transition-colors"
            title="恢复默认"
          >
            <RotateCcw :size="11" />
            恢复默认
          </button>
          <button
            @click="saveSettings"
            :disabled="saving"
            class="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium transition-colors bg-blue-50 text-blue-600 hover:bg-blue-100 border border-blue-200 disabled:opacity-50 disabled:cursor-not-allowed"
          >
            <Save :size="11" />
            {{ saved ? "已保存" : "保存" }}
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
                :placeholder="'默认：安装目录/tiles'"
                class="min-w-0 flex-1 px-2 py-1 rounded-md bg-slate-100 border border-slate-200 text-slate-700 text-xs focus:outline-none focus:ring-1 focus:ring-blue-500/60"
              />
              <button
                @click="pickFolder(field.key)"
                class="shrink-0 p-1.5 rounded-md bg-slate-100 border border-slate-200 text-slate-500 hover:bg-slate-200 hover:text-slate-700 transition-colors"
                title="选择目录"
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
            >下载规则（反封禁）</span
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
          <span class="text-xs font-semibold text-slate-600">自动更新</span>
        </div>

        <div class="px-4 py-4 flex flex-col gap-3">
          <!-- 版本信息 + 检查按钮 -->
          <div class="flex items-center gap-3">
            <div class="flex-1 text-xs text-slate-500">
              <span>当前版本：</span>
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
              {{ updateChecking ? "检查中…" : "检查更新" }}
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
                <div class="flex items-center gap-2">
                  <ArrowUpCircle :size="14" class="text-blue-500 shrink-0" />
                  <span class="text-xs font-semibold text-blue-700"
                    >发现新版本 v{{ updateResult.latestVersion }}</span
                  >
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
                        ? `下载中… ${downloadPercent}%`
                        : "下载完成，正在启动安装程序…"
                    }}</span>
                    <span v-if="totalBytes > 0"
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
                <div v-if="installError" class="text-xs text-red-500">
                  {{ installError }}
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
                    下载并安装
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
                    打开下载页面
                  </button>
                </div>
              </div>

              <!-- 已是最新 -->
              <div
                v-else-if="!updateResult.error"
                class="rounded-lg bg-green-50 border border-green-200 px-3 py-2 flex items-center gap-2"
              >
                <span class="text-xs text-green-700"
                  >✓ 已是最新版本 v{{
                    updateResult.latestVersion ?? updateResult.currentVersion
                  }}</span
                >
              </div>

              <!-- 错误 / 未配置 -->
              <div
                v-else
                class="rounded-lg bg-slate-50 border px-3 py-2 text-xs text-slate-500"
                style="border-color: var(--color-border-subtle)"
              >
                {{ updateResult.error }}
              </div>
            </div>
          </Transition>

          <!-- 接口错误 -->
          <div v-if="updateError" class="text-xs text-red-500">
            {{ updateError }}
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
