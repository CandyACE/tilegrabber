<script setup lang="ts">
import { ref, computed, watch } from "vue";
import { Clock, Gauge, RotateCcw, Save, CheckCircle2 } from "lucide-vue-next";
import { Switch } from "@/components/ui/switch";

// ─── Props / Emits ───────────────────────────────────────────────────────────

const props = defineProps<{
  settings: Record<string, string>;
}>();

const emit = defineEmits<{
  change: [settings: Record<string, string>];
}>();

// ─── 本地状态（双向绑定父级 settings） ─────────────────────────────────────

function getBool(key: string, def = false): boolean {
  const v = props.settings[key];
  if (v === undefined) return def;
  return v === "true";
}
function getNum(key: string, def = 0): number {
  const v = props.settings[key];
  return v !== undefined ? Number(v) : def;
}

const timeWindowEnabled = computed({
  get: () => getBool("rules.time_window_enabled"),
  set: (v) =>
    emit("change", {
      ...props.settings,
      "rules.time_window_enabled": v ? "true" : "false",
    }),
});

const startHour = computed({
  get: () => getNum("rules.time_window_start", 22),
  set: (v) =>
    emit("change", { ...props.settings, "rules.time_window_start": String(v) }),
});

const endHour = computed({
  get: () => getNum("rules.time_window_end", 8),
  set: (v) =>
    emit("change", { ...props.settings, "rules.time_window_end": String(v) }),
});

const maxTilesPerSec = computed({
  get: () => getNum("rules.max_tiles_per_sec", 0),
  set: (v) =>
    emit("change", { ...props.settings, "rules.max_tiles_per_sec": String(v) }),
});

const burstPauseMs = computed({
  get: () => getNum("rules.burst_pause_ms", 0),
  set: (v) =>
    emit("change", { ...props.settings, "rules.burst_pause_ms": String(v) }),
});

// ─── 时间窗口描述文本 ─────────────────────────────────────────────────────────

const windowDesc = computed(() => {
  const s = startHour.value.toString().padStart(2, "0");
  const e = endHour.value.toString().padStart(2, "0");
  return `${s}:00 — ${e}:00`;
});

// ─── 速率描述文本 ─────────────────────────────────────────────────────────────

const rateDesc = computed(() => {
  if (maxTilesPerSec.value === 0) return "不限速";
  return `≤ ${maxTilesPerSec.value} 瓦片/秒`;
});

// ─── 小时选项 ─────────────────────────────────────────────────────────────────

const hourOptions = Array.from({ length: 24 }, (_, i) => ({
  value: i,
  label: i.toString().padStart(2, "0") + ":00",
}));
</script>

<template>
  <div class="flex flex-col gap-4">
    <!-- ── 时间窗口 ─────────────────────────────────────────────────────────── -->
    <div
      class="rounded-xl border p-4 flex flex-col gap-3"
      style="
        border-color: var(--color-border-subtle);
        background: var(--color-surface-raised);
      "
    >
      <!-- 标题行 -->
      <div class="flex items-center gap-2">
        <Clock :size="14" class="text-blue-500 shrink-0" />
        <span class="text-xs font-semibold text-slate-700 tracking-wide"
          >下载时间窗口</span
        >
        <div class="ml-auto flex items-center gap-2">
          <span
            class="text-xs"
            :class="timeWindowEnabled ? 'text-blue-600' : 'text-slate-400'"
          >
            {{ timeWindowEnabled ? "已启用" : "已禁用" }}
          </span>
          <Switch
            :model-value="timeWindowEnabled"
            @update:model-value="(v) => (timeWindowEnabled = v as boolean)"
            class="scale-90"
          />
        </div>
      </div>

      <!-- 描述 -->
      <p class="text-xs text-slate-500 leading-relaxed">
        只在指定时间段内运行下载，适合夜间低峰期下载大量瓦片，避免占用白天带宽。
      </p>

      <!-- 时间范围选择 -->
      <Transition name="slide-fade">
        <div v-if="timeWindowEnabled" class="flex flex-col gap-2 pt-1">
          <div class="flex items-center gap-3">
            <div class="flex flex-col gap-1 flex-1">
              <label class="text-xs text-slate-500">开始时间</label>
              <select
                :value="startHour"
                @change="
                  startHour = Number(($event.target as HTMLSelectElement).value)
                "
                class="w-full text-xs rounded-lg border px-2 py-1.5 bg-white appearance-none cursor-pointer focus:outline-none focus:ring-2 focus:ring-blue-500/40"
                style="border-color: var(--color-border-subtle)"
              >
                <option
                  v-for="h in hourOptions"
                  :key="h.value"
                  :value="h.value"
                >
                  {{ h.label }}
                </option>
              </select>
            </div>
            <div class="mt-5 text-slate-400 text-sm shrink-0">–</div>
            <div class="flex flex-col gap-1 flex-1">
              <label class="text-xs text-slate-500">结束时间</label>
              <select
                :value="endHour"
                @change="
                  endHour = Number(($event.target as HTMLSelectElement).value)
                "
                class="w-full text-xs rounded-lg border px-2 py-1.5 bg-white appearance-none cursor-pointer focus:outline-none focus:ring-2 focus:ring-blue-500/40"
                style="border-color: var(--color-border-subtle)"
              >
                <option
                  v-for="h in hourOptions"
                  :key="h.value"
                  :value="h.value"
                >
                  {{ h.label }}
                </option>
              </select>
            </div>
          </div>
          <p class="text-xs text-blue-600 font-medium">
            将在 {{ windowDesc }} 内下载
            <span v-if="startHour > endHour" class="text-slate-400 font-normal"
              >（跨午夜）</span
            >
          </p>
        </div>
      </Transition>
    </div>

    <!-- ── 速率限制 ─────────────────────────────────────────────────────────── -->
    <div
      class="rounded-xl border p-4 flex flex-col gap-3"
      style="
        border-color: var(--color-border-subtle);
        background: var(--color-surface-raised);
      "
    >
      <div class="flex items-center gap-2">
        <Gauge :size="14" class="text-amber-500 shrink-0" />
        <span class="text-xs font-semibold text-slate-700 tracking-wide"
          >下载速率限制</span
        >
        <span
          class="ml-auto text-xs font-medium"
          :class="maxTilesPerSec > 0 ? 'text-amber-600' : 'text-slate-400'"
        >
          {{ rateDesc }}
        </span>
      </div>

      <p class="text-xs text-slate-500 leading-relaxed">
        限制每秒最大瓦片下载数，防止请求过于密集触发服务器封禁。设为 0
        表示不限速。
      </p>

      <!-- 滑块 -->
      <div class="flex items-center gap-3">
        <input
          type="range"
          :value="maxTilesPerSec"
          @input="
            maxTilesPerSec = Number(($event.target as HTMLInputElement).value)
          "
          min="0"
          max="20"
          step="1"
          class="flex-1 h-1.5 accent-amber-500 cursor-pointer"
        />
        <div class="flex items-center gap-1 shrink-0">
          <button
            class="w-6 h-6 flex items-center justify-center rounded border text-xs hover:bg-slate-100 active:bg-slate-200 transition-colors"
            style="border-color: var(--color-border-subtle)"
            :disabled="maxTilesPerSec <= 0"
            @click="maxTilesPerSec = Math.max(0, maxTilesPerSec - 1)"
          >
            −
          </button>
          <span
            class="w-8 text-center text-xs font-mono font-medium text-slate-700"
          >
            {{ maxTilesPerSec === 0 ? "∞" : maxTilesPerSec }}
          </span>
          <button
            class="w-6 h-6 flex items-center justify-center rounded border text-xs hover:bg-slate-100 active:bg-slate-200 transition-colors"
            style="border-color: var(--color-border-subtle)"
            :disabled="maxTilesPerSec >= 20"
            @click="maxTilesPerSec = Math.min(20, maxTilesPerSec + 1)"
          >
            +
          </button>
        </div>
      </div>
    </div>

    <!-- ── 批次间停顿 ──────────────────────────────────────────────────────── -->
    <div
      class="rounded-xl border p-4 flex flex-col gap-3"
      style="
        border-color: var(--color-border-subtle);
        background: var(--color-surface-raised);
      "
    >
      <div class="flex items-center gap-2">
        <span class="text-xs font-semibold text-slate-700 tracking-wide"
          >批次间停顿时间（ms）</span
        >
        <span class="ml-auto text-xs font-mono text-slate-500">
          {{ burstPauseMs === 0 ? "随机 600–2200ms" : `${burstPauseMs} ms` }}
        </span>
      </div>

      <p class="text-xs text-slate-500">
        每下完一批瓦片后的等待时间，模拟用户地图拖拽行为。0 =
        使用内置随机停顿（推荐）。
      </p>

      <div class="flex items-center gap-3">
        <input
          type="range"
          :value="burstPauseMs"
          @input="
            burstPauseMs = Number(($event.target as HTMLInputElement).value)
          "
          min="0"
          max="5000"
          step="100"
          class="flex-1 h-1.5 accent-blue-500 cursor-pointer"
        />
        <span class="w-16 text-right text-xs font-mono text-slate-600 shrink-0">
          {{ burstPauseMs === 0 ? "自动" : burstPauseMs + "ms" }}
        </span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.slide-fade-enter-active,
.slide-fade-leave-active {
  transition: all 0.2s ease;
  overflow: hidden;
}
.slide-fade-enter-from,
.slide-fade-leave-to {
  opacity: 0;
  max-height: 0;
}
.slide-fade-enter-to,
.slide-fade-leave-from {
  max-height: 200px;
}
</style>
