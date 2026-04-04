<script setup lang="ts">
import { ref } from "vue";
import { getVersion } from "@tauri-apps/api/app";
import {
  ExternalLink,
  MapPin,
  Building2,
  Mail,
  Globe,
  Shield,
  Coffee,
} from "lucide-vue-next";
import logoUrl from "~/assets/logo.png";

const appVersion = ref("");
getVersion().then((v) => {
  appVersion.value = v;
});

const features = [
  "支持 TMS / XYZ / WMTS 瓦片图源",
  "多线程并发下载，断点续传",
  "导出 MBTiles 或目录结构",
  "内置 TMS / WMTS 发布服务",
  "支持 .lrc / .lra / .ovmap 图层配置文件",
  "支持天地图、星图、自定义图源",
];
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
          关于御图
        </h2>
        <p class="text-xs" style="color: var(--color-text-muted)">
          离线地图瓦片管理工具
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
              御图
            </h1>
            <p class="text-sm" style="color: var(--color-text-secondary)">
              离线地图瓦片管理工具
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
                稳定版
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
            核心功能
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

        <!-- 公司信息 -->
        <section class="space-y-3">
          <h3
            class="text-sm font-semibold flex items-center gap-2"
            style="color: var(--color-text-primary)"
          >
            <Building2 class="size-4" style="color: var(--color-accent)" />
            关于公司
          </h3>
          <div
            class="rounded-xl border overflow-hidden"
            style="border-color: var(--color-border-subtle)"
          >
            <div
              class="flex items-start gap-3 p-4 border-b"
              style="
                background: var(--color-surface);
                border-color: var(--color-border-subtle);
              "
            >
              <Building2
                class="size-4 mt-0.5 shrink-0"
                style="color: var(--color-accent)"
              />
              <div>
                <p
                  class="text-xs font-medium"
                  style="color: var(--color-text-muted)"
                >
                  公司名称
                </p>
                <p
                  class="text-sm mt-0.5"
                  style="color: var(--color-text-primary)"
                >
                  苏州御空未来科技有限公司
                </p>
              </div>
            </div>
            <div
              class="flex items-start gap-3 p-4 border-b"
              style="
                background: var(--color-surface);
                border-color: var(--color-border-subtle);
              "
            >
              <Globe
                class="size-4 mt-0.5 shrink-0"
                style="color: var(--color-accent)"
              />
              <div>
                <p
                  class="text-xs font-medium"
                  style="color: var(--color-text-muted)"
                >
                  行业领域
                </p>
                <p
                  class="text-sm mt-0.5"
                  style="color: var(--color-text-primary)"
                >
                  无人机 · 地图数据 · 低空智联 · AI
                </p>
              </div>
            </div>
            <div
              class="flex items-start gap-3 p-4"
              style="background: var(--color-surface)"
            >
              <Mail
                class="size-4 mt-0.5 shrink-0"
                style="color: var(--color-accent)"
              />
              <div>
                <p
                  class="text-xs font-medium"
                  style="color: var(--color-text-muted)"
                >
                  联系方式
                </p>
                <p
                  class="text-sm mt-0.5"
                  style="color: var(--color-text-primary)"
                >
                  400@yukongcloud.com
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
            © 2026 苏州御空未来科技有限公司
          </p>
          <p>保留所有权利。本软件仅供授权用户使用。</p>
          <p class="pt-1">
            地图数据版权归各数据提供方所有，使用前请确认已获得相应授权。
          </p>
        </section>
      </div>
    </div>
  </div>
</template>
