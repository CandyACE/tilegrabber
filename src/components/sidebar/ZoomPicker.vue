<script setup lang="ts">
/**
 * ZoomPicker.vue — 层级范围选择器
 *
 * 双端滑块选择 min/max zoom，实时调用 Tauri 命令计算瓦片数量。
 */
import { ref, computed, watch } from "vue";
import { Loader, TriangleAlert, CircleX } from "lucide-vue-next";
import { invoke } from "@tauri-apps/api/core";
import type { Bounds, CrsType, TileCount } from "~/types/tile-source";

const props = defineProps<{
  bounds: Bounds | null;
  crs?: CrsType;
  maxZoomLimit?: number;
}>();

// v-model: { min, max }
const minZoom = defineModel<number>("minZoom", { default: 8 });
const maxZoom = defineModel<number>("maxZoom", { default: 12 });

const maxLimit = computed(() => props.maxZoomLimit ?? 22);

// 瓦片计数
const tileCount = ref<TileCount | null>(null);
const counting = ref(false);

async function refreshCount() {
  if (!props.bounds) {
    tileCount.value = null;
    return;
  }
  counting.value = true;
  try {
    tileCount.value = await invoke<TileCount>("calculate_tile_count", {
      bounds: props.bounds,
      minZoom: minZoom.value,
      maxZoom: maxZoom.value,
      crs: props.crs ?? null,
    });
  } catch (e) {
    console.warn("calculate_tile_count failed:", e);
    tileCount.value = null;
  } finally {
    counting.value = false;
  }
}

// 保证 min <= max
function onMinChange(v: number) {
  if (v > maxZoom.value) maxZoom.value = v;
  minZoom.value = v;
}
function onMaxChange(v: number) {
  if (v < minZoom.value) minZoom.value = v;
  maxZoom.value = v;
}

// bounds 或层级变化时重新计算
watch(
  [() => props.bounds, minZoom, maxZoom],
  () => {
    refreshCount();
  },
  { immediate: true },
);

// 格式化瓦片数量
function formatCount(n: number): string {
  if (n < 1000) return String(n);
  if (n < 1_000_000) return `${(n / 1000).toFixed(1)}K`;
  if (n < 1_000_000_000) return `${(n / 1_000_000).toFixed(2)}M`;
  return `${(n / 1_000_000_000).toFixed(2)}G`;
}

// 警告级别
const countWarning = computed(() => {
  if (!tileCount.value) return null;
  const t = tileCount.value.total;
  if (t > 100_000_000) return "error";
  if (t > 10_000_000) return "warn";
  return null;
});
</script>

<template>
  <div class="flex flex-col gap-3 select-none">
    <!-- 层级范围滑块 -->
    <div class="flex flex-col gap-1.5">
      <div
        class="flex items-center justify-between text-xs text-(--color-text-secondary)"
      >
        <span>层级范围</span>
        <span class="font-mono font-medium text-(--color-text-primary)">
          Z{{ minZoom }} — Z{{ maxZoom }}
        </span>
      </div>

      <!-- Min zoom -->
      <div class="flex items-center gap-2">
        <span
          class="w-6 text-right text-xs font-mono text-(--color-text-secondary)"
          >{{ minZoom }}</span
        >
        <input
          type="range"
          :min="0"
          :max="maxLimit"
          :value="minZoom"
          class="flex-1 accent-(--color-accent)"
          @input="onMinChange(+($event.target as HTMLInputElement).value)"
        />
        <span class="w-5 text-xs text-(--color-text-secondary)">最小</span>
      </div>

      <!-- Max zoom -->
      <div class="flex items-center gap-2">
        <span
          class="w-6 text-right text-xs font-mono text-(--color-text-secondary)"
          >{{ maxZoom }}</span
        >
        <input
          type="range"
          :min="0"
          :max="maxLimit"
          :value="maxZoom"
          class="flex-1 accent-(--color-accent)"
          @input="onMaxChange(+($event.target as HTMLInputElement).value)"
        />
        <span class="w-5 text-xs text-(--color-text-secondary)">最大</span>
      </div>
    </div>

    <!-- 瓦片数量 -->
    <div
      class="rounded-lg px-3 py-2 text-xs"
      :class="{
        'bg-(--color-canvas-subtle) text-(--color-text-secondary)':
          !countWarning,
        'bg-amber-50 text-amber-700 border border-amber-200':
          countWarning === 'warn',
        'bg-red-50 text-red-700 border border-red-200':
          countWarning === 'error',
      }"
    >
      <div v-if="!bounds" class="text-(--color-text-muted)">
        请先在地图上框选下载区域
      </div>
      <div v-else-if="tileCount" class="flex flex-col gap-1">
        <div class="flex items-center justify-between">
          <span class="flex items-center gap-1">
            预计瓦片数量
            <Loader v-if="counting" class="size-3 animate-spin opacity-50" />
          </span>
          <span class="font-mono font-semibold text-sm">
            {{ formatCount(tileCount.total) }}
          </span>
        </div>
        <!-- 按层级明细（固定高度，避免拖动滑块时表格抖动） -->
        <div class="mt-1 h-[10lh] overflow-y-auto">
          <div class="grid grid-cols-2 gap-x-2 gap-y-0.5">
            <template v-for="zc in tileCount.per_zoom" :key="zc.zoom">
              <span class="font-mono">Z{{ zc.zoom }}</span>
              <span class="text-right font-mono">{{
                formatCount(zc.count)
              }}</span>
            </template>
          </div>
        </div>
        <div
          v-if="countWarning === 'warn'"
          class="mt-1 flex items-center gap-1 text-amber-700"
        >
          <TriangleAlert class="size-3" />
          瓦片数量较多，下载可能耗时较长
        </div>
        <div
          v-if="countWarning === 'error'"
          class="mt-1 flex items-center gap-1 text-red-700"
        >
          <CircleX class="size-3" />
          瓦片数量过大，请缩小范围或降低最大层级
        </div>
      </div>
      <div v-else class="text-(--color-text-muted)">无法计算瓦片数量</div>
    </div>
  </div>
</template>
