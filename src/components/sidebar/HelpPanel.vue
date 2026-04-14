<script setup lang="ts">
import { ref, computed } from "vue";
import { getVersion } from "@tauri-apps/api/app";
import { useI18n } from "vue-i18n";
import {
  HelpCircle,
  Zap,
  Layers,
  Database,
  ListTodo,
  FileOutput,
  Server,
  SlidersHorizontal,
  ChevronDown,
} from "lucide-vue-next";

const { t, tm } = useI18n();
const appVersion = ref("");
getVersion().then((v) => (appVersion.value = v));

const sections = computed(() =>
  tm('help.sections') as Array<{
    title: string
    items: Array<{ q: string; a: string }>
  }>
)

const SECTION_ICONS = [Zap, Layers, Database, ListTodo, FileOutput, Server, SlidersHorizontal];
const SECTION_COLORS = [
  { bg: 'bg-amber-50', icon: 'text-amber-500', border: 'border-amber-200', badge: 'bg-amber-100 text-amber-700' },
  { bg: 'bg-purple-50', icon: 'text-purple-500', border: 'border-purple-200', badge: 'bg-purple-100 text-purple-700' },
  { bg: 'bg-sky-50', icon: 'text-sky-500', border: 'border-sky-200', badge: 'bg-sky-100 text-sky-700' },
  { bg: 'bg-green-50', icon: 'text-green-500', border: 'border-green-200', badge: 'bg-green-100 text-green-700' },
  { bg: 'bg-orange-50', icon: 'text-orange-500', border: 'border-orange-200', badge: 'bg-orange-100 text-orange-700' },
  { bg: 'bg-indigo-50', icon: 'text-indigo-500', border: 'border-indigo-200', badge: 'bg-indigo-100 text-indigo-700' },
  { bg: 'bg-slate-50', icon: 'text-slate-500', border: 'border-slate-200', badge: 'bg-slate-100 text-slate-600' },
];

// 每个 Q&A 独立展开状态
const openItems = ref<Record<string, boolean>>({});

function toggleItem(key: string) {
  openItems.value[key] = !openItems.value[key];
}
</script>

<template>
  <div class="flex flex-col h-full overflow-y-auto" style="background: var(--color-app-bg)">
    <div class="flex flex-col gap-5 w-full max-w-2xl mx-auto px-6 py-6 text-sm">

      <!-- 标题栏 -->
      <div class="flex items-center gap-2 px-0.5">
        <HelpCircle :size="14" class="text-slate-400 shrink-0" />
        <span class="text-xs font-semibold text-slate-600 tracking-wide uppercase">{{ t('help.title') }}</span>
      </div>

      <!-- App 介绍横幅 -->
      <div class="rounded-xl border border-blue-200 bg-blue-50 px-4 py-3">
        <div class="text-sm font-semibold text-blue-600 mb-1">{{ t('appName') }}</div>
        <div class="text-xs text-slate-500 leading-relaxed">{{ t('help.appDesc') }}</div>
        <div class="mt-2 text-[10px] text-slate-400">{{ t('help.version', { version: appVersion }) }}</div>
      </div>

      <!-- 章节列表 -->
      <div
        v-for="(section, si) in sections"
        :key="section.title"
        class="rounded-2xl border overflow-hidden shadow-sm"
        :class="[SECTION_COLORS[si % SECTION_COLORS.length].border, 'bg-white']"
      >
        <!-- 章节标题 -->
        <div
          class="flex items-center gap-2.5 px-4 py-3 border-b"
          :class="[SECTION_COLORS[si % SECTION_COLORS.length].bg, SECTION_COLORS[si % SECTION_COLORS.length].border]"
        >
          <div
            class="size-6 rounded-lg flex items-center justify-center shrink-0"
            :class="SECTION_COLORS[si % SECTION_COLORS.length].bg"
          >
            <component
              :is="SECTION_ICONS[si % SECTION_ICONS.length]"
              :size="14"
              :class="SECTION_COLORS[si % SECTION_COLORS.length].icon"
            />
          </div>
          <span class="text-xs font-bold text-slate-700">{{ section.title }}</span>
          <span
            class="ml-auto text-[10px] font-medium px-1.5 py-0.5 rounded-full"
            :class="SECTION_COLORS[si % SECTION_COLORS.length].badge"
          >{{ section.items.length }}</span>
        </div>

        <!-- Q&A 手风琴列表 -->
        <div class="divide-y divide-slate-100">
          <div
            v-for="(item, ii) in section.items"
            :key="item.q"
          >
            <!-- 问题行（可点击展开） -->
            <button
              class="w-full flex items-start gap-2.5 px-4 py-3 text-left transition-colors hover:bg-slate-50 focus:outline-none group"
              @click="toggleItem(`${si}-${ii}`)"
            >
              <span
                class="shrink-0 mt-0.5 text-[10px] font-bold w-4 h-4 rounded-full flex items-center justify-center"
                :class="[SECTION_COLORS[si % SECTION_COLORS.length].badge]"
              >Q</span>
              <span class="flex-1 text-xs font-semibold text-slate-700 leading-relaxed">{{ item.q }}</span>
              <ChevronDown
                :size="13"
                class="shrink-0 mt-0.5 text-slate-400 transition-transform duration-200"
                :class="openItems[`${si}-${ii}`] ? 'rotate-180' : ''"
              />
            </button>

            <!-- 回答（展开时显示） -->
            <Transition name="faq-expand">
              <div
                v-if="openItems[`${si}-${ii}`]"
                class="px-4 pb-3 flex gap-2.5"
              >
                <span class="shrink-0 text-[10px] font-bold w-4 h-4 rounded-full bg-emerald-100 text-emerald-700 flex items-center justify-center mt-0.5">A</span>
                <p class="text-xs text-slate-500 leading-relaxed">{{ item.a }}</p>
              </div>
            </Transition>
          </div>
        </div>
      </div>

      <!-- 底部版本占位 -->
      <div class="h-3" />
    </div>
  </div>
</template>

<style scoped>
.faq-expand-enter-active,
.faq-expand-leave-active {
  transition: all 0.2s ease;
  overflow: hidden;
}
.faq-expand-enter-from,
.faq-expand-leave-to {
  opacity: 0;
  max-height: 0;
  padding-bottom: 0;
}
.faq-expand-enter-to,
.faq-expand-leave-from {
  opacity: 1;
  max-height: 300px;
}
</style>

