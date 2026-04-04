<script setup lang="ts">
import { ref, onMounted } from "vue";
import { getVersion } from "@tauri-apps/api/app";
import { HelpCircle, ExternalLink } from "lucide-vue-next";

const appVersion = ref("");
onMounted(async () => {
  appVersion.value = await getVersion();
});

// 帮助内容按章节组织
const sections = [
  {
    title: "快速开始",
    items: [
      {
        q: "如何新建下载任务？",
        a: "点击地图工具栏的「新建任务」按鈕，在向导中选择地图源（.lrc / .lra / .ovmap 本地文件、WMTS URL 或 TMS/XYZ URL），预览无误后在地图上框选目标区域，设置层级范围并点击「开始下载」。",
      },
      {
        q: "如何在地图上框选下载区域？",
        a: "进入向导预览步骤后，点击地图右上角工具栏的「矩形」或「多边形」绘制工具。矩形：拖拽即可；多边形：逐点单击绘制任意形状，双击或点击起点完成闭合，裁剪时会按多边形边界精确裁切。松手后可在下方调整缩放层级并预览瓦片数量。",
      },
      {
        q: "如何查看下载进度？",
        a: "下载中的任务在左侧任务列表会实时更新进度条与速度。点击任意任务卡片可展开详情面板，显示实时速度（瓦片/秒 和 MB/秒）、ETA、已下载/失败瓦片数以及运行日志。",
      },
    ],
  },
  {
    title: "支持的数据源",
    items: [
      {
        q: ".lrc 文件",
        a: "Locaspace Viewer 图层配置文件，包含 TMS/WMTS URL 模板、范围和层级信息，GB18030 编码自动解析。",
      },
      {
        q: ".lra 文件",
        a: "gzip 压缩的 .lrc（或 JSON 格式 3D Tiles 索引），工具自动检测内部格式并解析。",
      },
      {
        q: ".ovmap 文件",
        a: "奥维地图导出的图层配置文件，工具自动解析内部瓦片 URL 模板与范围信息。",
      },
      {
        q: "WMTS URL",
        a: "输入标准 OGC WMTS 1.0.0 GetCapabilities 文档 URL，工具自动解析图层列表、范围和层级，并让用户选择所需图层。",
      },
      {
        q: "TMS / XYZ URL",
        a: "输入包含 {z}、{x}、{y} 占位符的瓦片 URL 模板，例如 https://tile.openstreetmap.org/{z}/{x}/{y}.png。",
      },
    ],
  },
  {
    title: "任务管理",
    items: [
      {
        q: "暂停与恢复",
        a: "在任务详情面板点击「暂停」/「继续下载」按钮。已下载的瓦片自动保留，恢复后从断点继续，不会重复下载。",
      },
      {
        q: "重试失败瓦片",
        a: "下载完成后如有失败瓦片，详情面板会显示「重试 N 个失败瓦片」按钮，仅对失败项重新发起请求。",
      },
      {
        q: "删除任务",
        a: "在任务详情面板点击「删除任务」，确认弹出的二次确认提示后才会执行删除。注意：同时会删除对应的瓦片存储文件，不可恢复。",
      },
      {
        q: "在地图上定位任务",
        a: "点击详情面板头部的定位按钮，地图会自动飞到该任务的下载边界范围。",
      },
      {
        q: "导出 / 导入任务包（.tgr）",
        a: "任务列表顶部的「导出」按钮可将任务连同全部已下载瓦片打包为 .tgr 文件（SQLite 格式，零拷贝速度极快）；「导入」按钮可直接加载他人分享的 .tgr 文件，导入后程序直接读取该文件，请勿移动或删除。外部 .tgr 任务卡片上有专属标识，删除时可选择是否同步删除文件。",
      },
    ],
  },
  {
    title: "导出",
    items: [
      {
        q: "MBTiles",
        a: "标准 SQLite 瓦片容器，兼容 QGIS、MapTiler、Mapbox 等工具。瓦片行号符合 TMS 规范（y 轴翻转）。",
      },
      {
        q: "目录结构（z/x/y）",
        a: "导出为 z/x/y.{ext} 目录树，适合自建服务器或 Leaflet.js 直接挂载使用。",
      },
      {
        q: "GeoTIFF",
        a: "将下载的瓦片拼合并导出为带地理坐标信息的 GeoTIFF 栅格文件，可直接在 QGIS/ArcGIS 中叠加。多边形任务会自动按边界进行像素级精确裁剪；超大范围自动使用 BigTIFF 格式（支持 >4 GB 文件）。",
      },
      {
        q: "如何导出？",
        a: "下载完成（或进行中）后，在任务详情面板点击「导出瓦片」，选择格式和输出路径，点击「开始导出」即可。",
      },
    ],
  },
  {
    title: "本地发布服务",
    items: [
      {
        q: "如何启动发布服务？",
        a: "点击顶部「发布」导航，设置端口号后点击「启动服务」。只有已完成下载的任务才会出现在发布列表中。",
      },
      {
        q: "TMS 端点",
        a: "http://localhost:{port}/tiles/{task_id}/{z}/{x}/{y}  —  可直接在 Leaflet / OpenLayers 中使用。",
      },
      {
        q: "WMTS 能力文档",
        a: "http://localhost:{port}/wmts/{task_id}?SERVICE=WMTS&REQUEST=GetCapabilities  —  QGIS 等 GIS 软件可直接加载。",
      },
      {
        q: "在 QGIS 中使用 TMS",
        a: "打开「数据源管理 → XYZ Tiles」，新建连接，输入 TMS URL，将 {z}/{x}/{y} 替换为 {0}/{1}/{2}。",
      },
    ],
  },
  {
    title: "下载设置",
    items: [
      {
        q: "并发数",
        a: "默认 32，可在「设置」中调整。一般地图源 16–32 为最佳区间；超过 64 容易触发 429 限速。",
      },
      {
        q: "请求间隔",
        a: "两次请求之间的随机延迟，默认 0–150 ms。适当抖动可降低被封禁概率，设为 0 可获得最快速度但风险稍高。",
      },
      {
        q: "下载规则（时间窗口 / 限速）",
        a: "在「设置 → 下载规则」中可配置允许下载的时段（如只在夜间下载）以及每秒最大瓦片数，适合在有带宽限制或访问协议限制的场景使用。",
      },
    ],
  },
] as const;
</script>

<template>
  <div class="flex flex-col h-full overflow-y-auto">
    <div class="flex flex-col gap-4 w-full max-w-2xl mx-auto px-6 py-6 text-sm">
      <!-- 标题栏 -->
      <div class="flex items-center gap-2 px-0.5">
        <HelpCircle :size="14" class="text-slate-400 shrink-0" />
        <span
          class="text-xs font-semibold text-slate-600 tracking-wide uppercase"
          >帮助文档</span
        >
      </div>

      <!-- App 介绍横幅 -->
      <div class="rounded-xl border border-blue-200 bg-blue-50 px-4 py-3">
        <div class="text-sm font-semibold text-blue-600 mb-1">御图</div>
        <div class="text-xs text-slate-500 leading-relaxed">
          跨平台地图瓦片下载工具，支持 TMS / WMTS / .lrc / .lra 多种数据源，
          提供断点续传、反封禁下载、MBTiles 导出和本地瓦片发布服务。
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
        御图 v{{ appVersion }} · 基于 Tauri v2 + MapLibre GL JS v4
      </div>
    </div>
  </div>
</template>
