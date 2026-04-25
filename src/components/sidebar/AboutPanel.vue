<script setup lang="ts">
import { ref, computed } from "vue";
import { getVersion } from "@tauri-apps/api/app";
import { useI18n } from "vue-i18n";
import {
  ExternalLink,
  Github,
  Shield,
  Coffee,
  BookOpen,
} from "lucide-vue-next";
import logoUrl from "~/assets/logo.png";

const { t, tm } = useI18n();

const appVersion = ref("");
getVersion().then((v) => {
  appVersion.value = v;
});

const features = computed(() => tm('about.features') as string[])
</script>

<template>
  <div class="flex-1 flex flex-col overflow-hidden">
    <!-- 顶部标题栏 -->
    <div
      class="flex items-center gap-3 px-6 py-4 border-b shrink-0"
      style="border-color: var(--color-border-subtle)"
    >
      <div
        class="flex items-center justify-center size-9 rounded-xl"
        style="background: var(--color-accent-muted, #eff6ff)"
      >
        <Shield class="size-5" style="color: var(--color-accent)" />
      </div>
      <div>
        <h2
          class="text-base font-semibold"
          style="color: var(--color-text-primary)"
        >
          {{ t('about.title') }}
        </h2>
        <p class="text-xs" style="color: var(--color-text-muted)">
          {{ t('about.subtitle') }}
        </p>
      </div>
    </div>

    <!-- 正文内容 -->
    <div class="flex-1 overflow-y-auto">
      <div class="max-w-xl mx-auto px-6 py-6 space-y-8">
        <!-- 软件卡片 -->
        <div
          class="flex items-center gap-5 p-5 rounded-2xl border"
          style="
            background: var(--color-surface);
            border-color: var(--color-border-subtle);
          "
        >
          <img
            :src="logoUrl"
            alt="御图"
            class="size-16 rounded-2xl object-contain shrink-0 shadow-sm"
          />
          <div class="space-y-1 min-w-0">
            <h1
              class="text-xl font-bold tracking-tight"
              style="color: var(--color-text-primary)"
            >
              {{ t('appName') }}
            </h1>
            <p class="text-sm" style="color: var(--color-text-secondary)">
              {{ t('about.subtitle') }}
            </p>
            <div class="flex items-center gap-2 pt-1">
              <span
                class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs font-medium"
                style="
                  background: var(--color-badge-blue-bg, #dbeafe);
                  color: #1d4ed8;
                "
              >
                v{{ appVersion }}
              </span>
              <span
                class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs font-medium"
                style="
                  background: var(--color-badge-green-bg, #dcfce7);
                  color: #15803d;
                "
              >
                {{ t('about.stableVersion') }}
              </span>
            </div>
          </div>
        </div>

        <!-- 功能特性 -->
        <section class="space-y-3">
          <h3
            class="text-sm font-semibold flex items-center gap-2"
            style="color: var(--color-text-primary)"
          >
            <Coffee class="size-4" style="color: var(--color-accent)" />
            {{ t('about.featuresTitle') }}
          </h3>
          <ul class="grid grid-cols-1 gap-2">
            <li
              v-for="feat in features"
              :key="feat"
              class="flex items-center gap-2.5 text-sm px-3.5 py-2.5 rounded-lg"
              style="
                background: var(--color-surface);
                color: var(--color-text-secondary);
                border: 1px solid var(--color-border-subtle);
              "
            >
              <span
                class="size-1.5 rounded-full shrink-0"
                style="background: var(--color-accent)"
              />
              {{ feat }}
            </li>
          </ul>
        </section>

        <!-- 开源项目 -->
        <section class="space-y-3">
          <h3
            class="text-sm font-semibold flex items-center gap-2"
            style="color: var(--color-text-primary)"
          >
            <BookOpen class="size-4" style="color: var(--color-accent)" />
            {{ t('about.openSourceTitle') }}
          </h3>
          <div
            class="rounded-xl border overflow-hidden"
            style="border-color: var(--color-border-subtle)"
          >
            <a
              href="https://github.com/CandyACE/tilegrabber"
              target="_blank"
              rel="noopener noreferrer"
              class="flex items-center gap-3 p-4 border-b transition-colors hover:opacity-80"
              style="
                background: var(--color-surface);
                border-color: var(--color-border-subtle);
                text-decoration: none;
              "
            >
              <Github
                class="size-4 mt-0.5 shrink-0"
                style="color: var(--color-accent)"
              />
              <div class="flex-1 min-w-0">
                <p
                  class="text-xs font-medium"
                  style="color: var(--color-text-muted)"
                >
                  {{ t('about.githubLabel') }}
                </p>
                <p
                  class="text-sm mt-0.5 truncate"
                  style="color: var(--color-text-primary)"
                >
                  github.com/CandyACE/tilegrabber
                </p>
              </div>
              <ExternalLink class="size-3.5 shrink-0" style="color: var(--color-text-muted)" />
            </a>
            <div
              class="flex items-start gap-3 p-4"
              style="background: var(--color-surface)"
            >
              <Shield
                class="size-4 mt-0.5 shrink-0"
                style="color: var(--color-accent)"
              />
              <div>
                <p
                  class="text-xs font-medium"
                  style="color: var(--color-text-muted)"
                >
                  {{ t('about.licenseLabel') }}
                </p>
                <p
                  class="text-sm mt-0.5"
                  style="color: var(--color-text-primary)"
                >
                  MIT License
                </p>
              </div>
            </div>
          </div>
        </section>

        <!-- 版权声明 -->
        <section
          class="rounded-xl p-4 text-center text-xs space-y-1"
          style="
            background: var(--color-surface);
            color: var(--color-text-muted);
            border: 1px solid var(--color-border-subtle);
          "
        >
          <p class="font-medium" style="color: var(--color-text-secondary)">
            {{ t('about.copyright') }}
          </p>
          <p>{{ t('about.mapDataNote') }}</p>
        </section>
      </div>
    </div>
  </div>
</template>
