<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import { getVersion } from "@tauri-apps/api/app";
import { useI18n } from "vue-i18n";
import { HelpCircle, ExternalLink } from "lucide-vue-next";

const { t, tm } = useI18n();
const appVersion = ref("");
onMounted(async () => {
  appVersion.value = await getVersion();
});

const sections = computed(() =>
  tm('help.sections') as Array<{
    title: string
    items: Array<{ q: string; a: string }>
  }>
)
</script>

<template>
  <div class="flex flex-col h-full overflow-y-auto">
    <div class="flex flex-col gap-4 w-full max-w-2xl mx-auto px-6 py-6 text-sm">
      <!-- 标题栏 -->
      <div class="flex items-center gap-2 px-0.5">
        <HelpCircle :size="14" class="text-slate-400 shrink-0" />
        <span
          class="text-xs font-semibold text-slate-600 tracking-wide uppercase"
          >{{ t('help.title') }}</span
        >
      </div>

      <!-- App 介绍横幅 -->
      <div class="rounded-xl border border-blue-200 bg-blue-50 px-4 py-3">
        <div class="text-sm font-semibold text-blue-600 mb-1">{{ t('appName') }}</div>
        <div class="text-xs text-slate-500 leading-relaxed">
          {{ t('help.appDesc') }}
        </div>
      </div>

      <!-- 章节 -->
      <div
        v-for="section in sections"
        :key="section.title"
        class="rounded-xl border bg-white overflow-hidden"
        style="border-color: var(--color-border-subtle)"
      >
        <!-- 章节标题 -->
        <div
          class="px-4 py-2.5 border-b text-xs font-semibold text-slate-600"
          style="border-color: var(--color-border-subtle)"
        >
          {{ section.title }}
        </div>

        <!-- Q&A 列表 -->
        <div class="divide-y divide-slate-100">
          <div v-for="item in section.items" :key="item.q" class="px-4 py-3">
            <div class="text-xs font-semibold text-slate-700 mb-1">
              {{ item.q }}
            </div>
            <div class="text-xs text-slate-500 leading-relaxed">
              {{ item.a }}
            </div>
          </div>
        </div>
      </div>

      <!-- 版本信息 -->
      <div class="text-center text-xs text-slate-400 py-2">
        {{ t('help.version', { version: appVersion }) }}
      </div>
    </div>
  </div>
</template>
