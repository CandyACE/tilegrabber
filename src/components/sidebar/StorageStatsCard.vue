<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import { useI18n } from "vue-i18n";
import { invoke } from "@tauri-apps/api/core";
import { HardDrive, RefreshCw, Trash2, Folder, AlertTriangle } from "lucide-vue-next";

const { t } = useI18n();

interface TaskStorageRow {
  taskId: string;
  name: string;
  path: string;
  bytes: number;
  exists: boolean;
  isExternal: boolean;
  downloadedTiles: number;
  status: string;
}
interface OrphanFile {
  path: string;
  bytes: number;
}
interface StorageStats {
  tilesDir: string;
  totalBytes: number;
  availableBytes: number;
  capacityBytes: number;
  tasks: TaskStorageRow[];
  orphans: OrphanFile[];
  orphanBytes: number;
}

const stats = ref<StorageStats | null>(null);
const loading = ref(false);
const expanded = ref(false);
const cleaningOrphans = ref(false);

const showAllTasks = ref(false);

function formatBytes(bytes: number): string {
  if (!bytes || bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  let v = bytes;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v < 10 && i > 0 ? v.toFixed(2) : v.toFixed(1)} ${units[i]}`;
}

const usedRatio = computed(() => {
  if (!stats.value || stats.value.capacityBytes <= 0) return 0;
  const usedDisk = stats.value.capacityBytes - stats.value.availableBytes;
  return Math.max(0, Math.min(1, usedDisk / stats.value.capacityBytes));
});

const taskShareRatio = computed(() => {
  if (!stats.value || stats.value.capacityBytes <= 0) return 0;
  return Math.max(
    0,
    Math.min(1, stats.value.totalBytes / stats.value.capacityBytes),
  );
});

const visibleTasks = computed(() => {
  if (!stats.value) return [];
  const filtered = stats.value.tasks.filter((t) => t.exists && t.bytes > 0);
  return showAllTasks.value ? filtered : filtered.slice(0, 5);
});

const hiddenTasksCount = computed(() => {
  if (!stats.value) return 0;
  const total = stats.value.tasks.filter((t) => t.exists && t.bytes > 0).length;
  return Math.max(0, total - 5);
});

async function load() {
  loading.value = true;
  try {
    stats.value = await invoke<StorageStats>("storage_stats");
  } catch (e) {
    console.error("[StorageStatsCard] storage_stats failed:", e);
  } finally {
    loading.value = false;
  }
}

async function cleanupOrphans() {
  if (!stats.value || stats.value.orphans.length === 0) return;
  const paths = stats.value.orphans.map((o) => o.path);
  const msg = t("settings.storage.confirmCleanup", {
    count: paths.length,
    size: formatBytes(stats.value.orphanBytes),
  });
  if (!window.confirm(msg)) return;
  cleaningOrphans.value = true;
  try {
    await invoke<number>("cleanup_orphan_tiles", { paths });
    await load();
  } catch (e) {
    console.error("[StorageStatsCard] cleanup_orphan_tiles failed:", e);
  } finally {
    cleaningOrphans.value = false;
  }
}

async function openTilesDir() {
  if (!stats.value) return;
  try {
    await invoke("reveal_in_explorer", { path: stats.value.tilesDir });
  } catch (e) {
    console.error(e);
  }
}

onMounted(load);
</script>

<template>
  <div
    class="rounded-xl border bg-white overflow-hidden"
    style="border-color: var(--color-border-subtle)"
  >
    <!-- 组头 -->
    <div
      class="flex items-center gap-2 px-4 py-2.5 border-b cursor-pointer select-none"
      style="border-color: var(--color-border-subtle)"
      @click="expanded = !expanded"
    >
      <HardDrive :size="13" class="text-slate-400 shrink-0" />
      <span class="text-xs font-semibold text-slate-600">
        {{ t("settings.storage.title") }}
      </span>
      <span
        v-if="stats"
        class="text-[11px] text-slate-400 ml-2"
      >
        {{ formatBytes(stats.totalBytes) }} /
        {{ formatBytes(stats.capacityBytes) }}
      </span>
      <div class="ml-auto flex items-center gap-1">
        <button
          class="p-1 rounded hover:bg-slate-100 text-slate-400 hover:text-slate-600 transition-colors"
          :title="t('settings.storage.refresh')"
          @click.stop="load"
        >
          <RefreshCw :size="12" :class="loading ? 'animate-spin' : ''" />
        </button>
        <span class="text-[10px] text-slate-400">
          {{ expanded ? "▲" : "▼" }}
        </span>
      </div>
    </div>

    <div v-if="expanded && stats" class="px-4 py-3 flex flex-col gap-3">
      <!-- 磁盘容量条 -->
      <div class="flex flex-col gap-1">
        <div class="flex items-center justify-between text-[11px] text-slate-500">
          <span>{{ t("settings.storage.diskUsage") }}</span>
          <span>
            {{ t("settings.storage.available") }}:
            <span class="font-mono text-slate-700">{{
              formatBytes(stats.availableBytes)
            }}</span>
          </span>
        </div>
        <div
          class="relative h-2.5 rounded-full overflow-hidden"
          style="background: var(--color-border-subtle)"
        >
          <!-- 其他占用（磁盘 used - 本应用占用） -->
          <div
            class="absolute inset-y-0 left-0 bg-slate-300"
            :style="{ width: usedRatio * 100 + '%' }"
          />
          <!-- 本应用占用（叠加在最左侧） -->
          <div
            class="absolute inset-y-0 left-0 bg-blue-500"
            :style="{ width: taskShareRatio * 100 + '%' }"
          />
        </div>
        <div class="flex items-center gap-4 text-[10px] text-slate-500 mt-0.5">
          <span class="flex items-center gap-1">
            <span class="inline-block w-2 h-2 rounded bg-blue-500"></span>
            {{ t("settings.storage.appUsage") }}:
            <span class="font-mono text-slate-700">{{
              formatBytes(stats.totalBytes)
            }}</span>
          </span>
          <span class="flex items-center gap-1">
            <span class="inline-block w-2 h-2 rounded bg-slate-300"></span>
            {{ t("settings.storage.otherUsage") }}
          </span>
        </div>
      </div>

      <!-- 默认目录路径 -->
      <div class="flex items-center gap-2 px-2 py-1.5 rounded bg-slate-50 border border-slate-200">
        <Folder :size="12" class="text-slate-400 shrink-0" />
        <span
          class="text-[11px] text-slate-600 font-mono truncate flex-1"
          :title="stats.tilesDir"
        >
          {{ stats.tilesDir }}
        </span>
        <button
          class="text-[11px] px-2 py-0.5 rounded border border-slate-200 bg-white text-slate-500 hover:bg-slate-100 transition-colors shrink-0"
          @click="openTilesDir"
        >
          {{ t("settings.storage.openDir") }}
        </button>
      </div>

      <!-- 任务存储列表 -->
      <div v-if="visibleTasks.length > 0" class="flex flex-col gap-1">
        <div class="text-[11px] font-semibold text-slate-600 px-1">
          {{ t("settings.storage.perTask") }}
        </div>
        <ul class="flex flex-col gap-0.5">
          <li
            v-for="row in visibleTasks"
            :key="row.taskId"
            class="flex items-center gap-2 px-2 py-1 rounded hover:bg-slate-50"
          >
            <span
              class="inline-block w-2 h-2 rounded shrink-0"
              :class="row.isExternal ? 'bg-amber-400' : 'bg-blue-400'"
              :title="row.isExternal ? '外部 .tgr' : '内置 .tiles'"
            ></span>
            <span
              class="text-[11px] text-slate-700 truncate flex-1"
              :title="row.name"
            >
              {{ row.name || "(unnamed)" }}
            </span>
            <span class="text-[10px] text-slate-400 font-mono">
              {{ row.downloadedTiles.toLocaleString() }}
            </span>
            <span
              class="text-[11px] font-mono text-slate-700 w-16 text-right shrink-0"
            >
              {{ formatBytes(row.bytes) }}
            </span>
          </li>
        </ul>
        <button
          v-if="hiddenTasksCount > 0"
          class="text-[11px] text-blue-500 hover:underline self-start"
          @click="showAllTasks = !showAllTasks"
        >
          {{
            showAllTasks
              ? t("settings.storage.showLess")
              : t("settings.storage.showMore", { count: hiddenTasksCount })
          }}
        </button>
      </div>

      <!-- 孤儿文件 -->
      <div
        v-if="stats.orphans.length > 0"
        class="rounded border border-amber-200 bg-amber-50 p-2 flex flex-col gap-1.5"
      >
        <div class="flex items-center gap-1.5">
          <AlertTriangle :size="12" class="text-amber-500 shrink-0" />
          <span class="text-[11px] font-semibold text-amber-700">
            {{
              t("settings.storage.orphansTitle", {
                count: stats.orphans.length,
                size: formatBytes(stats.orphanBytes),
              })
            }}
          </span>
          <button
            class="ml-auto text-[11px] px-2 py-0.5 rounded border border-amber-300 bg-white text-amber-700 hover:bg-amber-100 transition-colors disabled:opacity-50"
            :disabled="cleaningOrphans"
            @click="cleanupOrphans"
          >
            <Trash2 :size="10" class="inline mr-0.5" />
            {{
              cleaningOrphans
                ? t("settings.storage.cleaning")
                : t("settings.storage.cleanupAll")
            }}
          </button>
        </div>
        <p class="text-[10px] text-amber-700/80 leading-relaxed">
          {{ t("settings.storage.orphansHint") }}
        </p>
      </div>
    </div>
  </div>
</template>
