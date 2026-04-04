<script setup lang="ts">
import { ref, computed, onMounted, nextTick } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { Plus, Layers, Download, Trash2, Map, Pencil, Check, X } from "lucide-vue-next";

// ─── 类型 ──────────────────────────────────────────────────────────────────

interface Layer {
  id: string;
  name: string;
  sourceConfig: string;
  sortOrder: number;
  createdAt: string;
  updatedAt: string;
}

interface SourceInfo {
  kind: string;
  minZoom: number;
  maxZoom: number;
}

// ─── Props & Emits ─────────────────────────────────────────────────────────

const props = defineProps<{
  open: boolean;
}>();

const emit = defineEmits<{
  "update:open": [value: boolean];
  "add-layer": [];
  "download-layer": [layerName: string, sourceConfig: string];
  "select-layer": [sourceConfig: string | null];
}>();

// ─── 数据 ──────────────────────────────────────────────────────────────────

const layers = ref<Layer[]>([]);
const selectedLayerId = ref<string | null>(null);

async function loadLayers() {
  try {
    layers.value = await invoke<Layer[]>("list_layers");
  } catch (e) {
    console.error("[LayerSidebar] list_layers failed:", e);
  }
}

onMounted(loadLayers);

// ─── 解析 sourceConfig 展示信息 ───────────────────────────────────────────

const KIND_LABELS: Record<string, string> = {
  lrc: "离线图集",
  lra: "离线图集",
  ovmap: "OVMap",
  wmts: "WMTS",
  tms: "TMS",
  webcapture: "Web截图",
  threedtiles: "3D Tiles",
};

function parseSource(sourceConfig: string): SourceInfo | null {
  try {
    const s = JSON.parse(sourceConfig);
    return {
      kind: KIND_LABELS[s.kind] ?? s.kind ?? "未知",
      minZoom: s.min_zoom ?? 0,
      maxZoom: s.max_zoom ?? 22,
    };
  } catch {
    return null;
  }
}

function formatDate(iso: string): string {
  try {
    const d = new Date(iso);
    return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
  } catch {
    return iso;
  }
}

// ─── 选中图层 ──────────────────────────────────────────────────────────────

function handleSelectLayer(layer: Layer) {
  if (editingId.value === layer.id) return; // 编辑状态不触发选中
  if (selectedLayerId.value === layer.id) {
    selectedLayerId.value = null;
    emit("select-layer", null);
  } else {
    selectedLayerId.value = layer.id;
    emit("select-layer", layer.sourceConfig);
  }
}

// ─── 重命名 ────────────────────────────────────────────────────────────────

const editingId = ref<string | null>(null);
const editingName = ref("");
const editInputRef = ref<HTMLInputElement | null>(null);

async function startRename(layer: Layer, event: MouseEvent) {
  event.stopPropagation();
  editingId.value = layer.id;
  editingName.value = layer.name;
  await nextTick();
  editInputRef.value?.focus();
  editInputRef.value?.select();
}

async function confirmRename(layer: Layer) {
  const trimmed = editingName.value.trim();
  if (trimmed && trimmed !== layer.name) {
    try {
      await invoke("rename_layer", { layerId: layer.id, name: trimmed });
      layer.name = trimmed;
    } catch (e) {
      console.error("[LayerSidebar] rename_layer failed:", e);
    }
  }
  editingId.value = null;
}

function cancelRename() {
  editingId.value = null;
}

function onEditKeydown(event: KeyboardEvent, layer: Layer) {
  if (event.key === "Enter") confirmRename(layer);
  else if (event.key === "Escape") cancelRename();
}

// ─── 删除图层 ──────────────────────────────────────────────────────────────

async function handleDelete(id: string, event: MouseEvent) {
  event.stopPropagation();
  try {
    await invoke("delete_layer", { layerId: id });
    if (selectedLayerId.value === id) {
      selectedLayerId.value = null;
      emit("select-layer", null);
    }
    await loadLayers();
  } catch (e) {
    console.error("[LayerSidebar] delete_layer failed:", e);
  }
}

// ─── 下载图层 ──────────────────────────────────────────────────────────────

function handleDownload(layer: Layer, event: MouseEvent) {
  event.stopPropagation();
  emit("download-layer", layer.name, layer.sourceConfig);
}

// ─── 侧边栏宽度 ───────────────────────────────────────────────────────────

const sidebarWidth = computed(() => (props.open ? "280px" : "0px"));

defineExpose({ loadLayers });
</script>

<template>
  <aside
    class="flex flex-col shrink-0 overflow-hidden transition-all duration-200 ease-in-out border-r"
    :style="{
      width: sidebarWidth,
      background: 'var(--color-app-bg)',
      borderColor: 'var(--color-border-subtle)',
      minWidth: open ? '280px' : '0px',
    }"
  >
    <div class="flex flex-col h-full w-70">
      <!-- 头部 -->
      <div
        class="flex items-center justify-between px-4 py-3 border-b shrink-0"
        style="border-color: var(--color-border-subtle)"
      >
        <h2
          class="text-sm font-semibold"
          style="color: var(--color-text-primary)"
        >
          图层列表
        </h2>
        <button
          class="flex items-center gap-1.5 px-2.5 py-1 rounded-md text-xs font-medium text-white transition-colors hover:opacity-90 active:opacity-80"
          style="background: var(--color-accent)"
          @click="emit('add-layer')"
        >
          <Plus class="size-3.5" />
          添加图层
        </button>
      </div>

      <!-- 图层列表 -->
      <div class="flex-1 overflow-y-auto px-3 py-2 space-y-2">
        <template v-if="layers.length > 0">
          <div
            v-for="layer in layers"
            :key="layer.id"
            class="group relative rounded-xl cursor-pointer transition-all border"
            :class="
              selectedLayerId === layer.id
                ? 'border-blue-200 shadow-sm'
                : 'border-transparent hover:border-slate-200'
            "
            :style="
              selectedLayerId === layer.id
                ? 'background: #eff6ff'
                : 'background: var(--color-surface)'
            "
            @click="handleSelectLayer(layer)"
          >
            <!-- 卡片主体 -->
            <div class="flex items-start gap-2.5 px-3 py-2.5">
              <!-- 图标 -->
              <div
                class="flex items-center justify-center size-8 rounded-lg shrink-0 mt-0.5"
                :class="
                  selectedLayerId === layer.id
                    ? 'bg-blue-100 text-blue-500'
                    : 'bg-slate-100 text-slate-400'
                "
              >
                <Map class="size-4" />
              </div>

              <!-- 文字区 -->
              <div class="flex-1 min-w-0">
                <!-- 名称行 -->
                <div class="flex items-center gap-0.5">
                  <template v-if="editingId === layer.id">
                    <input
                      ref="editInputRef"
                      v-model="editingName"
                      class="flex-1 min-w-0 text-sm font-semibold border border-blue-300 rounded px-1.5 py-0.5 outline-none focus:ring-1 focus:ring-blue-400"
                      style="color: var(--color-text-primary)"
                      @keydown="onEditKeydown($event, layer)"
                      @click.stop
                    />
                    <button
                      title="确认"
                      class="shrink-0 flex items-center justify-center size-6 rounded text-green-600 hover:bg-green-100 transition-colors"
                      @click.stop="confirmRename(layer)"
                    ><Check class="size-3.5" /></button>
                    <button
                      title="取消"
                      class="shrink-0 flex items-center justify-center size-6 rounded text-slate-400 hover:bg-slate-100 transition-colors"
                      @click.stop="cancelRename"
                    ><X class="size-3.5" /></button>
                  </template>
                  <template v-else>
                    <span
                      class="flex-1 text-sm font-semibold truncate mr-0.5"
                      :class="selectedLayerId === layer.id ? 'text-blue-800' : 'text-slate-800'"
                    >{{ layer.name }}</span>
                    <button
                      title="重命名"
                      class="shrink-0 flex items-center justify-center size-6 rounded transition-colors"
                      :class="
                        selectedLayerId === layer.id
                          ? 'text-blue-300 hover:text-blue-600 hover:bg-blue-100'
                          : 'text-slate-200 group-hover:text-slate-400 hover:bg-slate-100'
                      "
                      @click="startRename(layer, $event)"
                    ><Pencil class="size-3" /></button>
                    <button
                      title="下载此图层"
                      class="shrink-0 flex items-center justify-center size-6 rounded transition-colors"
                      :class="
                        selectedLayerId === layer.id
                          ? 'text-blue-300 hover:text-blue-600 hover:bg-blue-100'
                          : 'text-slate-200 group-hover:text-slate-400 hover:bg-slate-100'
                      "
                      @click="handleDownload(layer, $event)"
                    ><Download class="size-3" /></button>
                    <button
                      title="删除图层"
                      class="shrink-0 flex items-center justify-center size-6 rounded transition-colors"
                      :class="
                        selectedLayerId === layer.id
                          ? 'text-blue-300 hover:text-red-500 hover:bg-red-50'
                          : 'text-slate-200 group-hover:text-slate-400 hover:text-red-500 hover:bg-red-50'
                      "
                      @click="handleDelete(layer.id, $event)"
                    ><Trash2 class="size-3" /></button>
                  </template>
                </div>

                <!-- 元信息：单行，超长截断 -->
                <p class="text-[11px] text-slate-400 truncate mt-0.5 leading-none">
                  <template v-if="parseSource(layer.sourceConfig)">
                    <span
                      class="font-medium"
                      :class="selectedLayerId === layer.id ? 'text-blue-500' : 'text-slate-500'"
                    >{{ parseSource(layer.sourceConfig)?.kind }}</span>
                    <span class="mx-1 text-slate-300">·</span>
                    <span>z{{ parseSource(layer.sourceConfig)?.minZoom }}–{{ parseSource(layer.sourceConfig)?.maxZoom }}</span>
                    <span class="mx-1 text-slate-300">·</span>
                  </template>
                  <span>{{ formatDate(layer.createdAt) }}</span>
                </p>
              </div>
            </div>
          </div>
        </template>

        <!-- 空状态 -->
        <div
          v-else
          class="flex flex-col items-center justify-center py-12 text-center"
        >
          <Layers class="size-10 mb-3 text-slate-300" />
          <p
            class="text-sm font-medium mb-1"
            style="color: var(--color-text-secondary)"
          >
            暂无图层
          </p>
          <p class="text-xs" style="color: var(--color-text-muted)">
            点击「添加图层」按钮导入数据源
          </p>
        </div>
      </div>
    </div>
  </aside>
</template>