<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from "vue";
import maplibregl from "maplibre-gl";
import type { Map as MaplibreMap } from "maplibre-gl";

const mapContainer = ref<HTMLDivElement>();
let map: MaplibreMap | null = null;

const emit = defineEmits<{
  ready: [map: MaplibreMap];
}>();

// 当前光标坐标（状态栏显示用）
const cursorCoords = ref<{ lng: number; lat: number } | null>(null);
const zoom = ref(4);

onMounted(() => {
  if (!mapContainer.value) return;

  map = new maplibregl.Map({
    container: mapContainer.value,
    style: {
      version: 8,
      glyphs: "https://demotiles.maplibre.org/font/{fontstack}/{range}.pbf",
      sources: {
        basemap: {
          type: "raster",
          tiles: [
            "https://tiles1.geovisearth.com/base/v1/vec/{z}/{x}/{y}?format=png&tmsIds=w&token=5853926b4d949104dd7742408f27de6321c106f67a76c3763fbbd5e4e5605b81",
            "https://tiles2.geovisearth.com/base/v1/vec/{z}/{x}/{y}?format=png&tmsIds=w&token=5853926b4d949104dd7742408f27de6321c106f67a76c3763fbbd5e4e5605b81",
            "https://tiles3.geovisearth.com/base/v1/vec/{z}/{x}/{y}?format=png&tmsIds=w&token=5853926b4d949104dd7742408f27de6321c106f67a76c3763fbbd5e4e5605b81",
          ],
          tileSize: 256,
          attribution:
            '© <a href="https://www.geovisearth.com/" target="_blank">星图地球</a>',
          maxzoom: 18,
        },
      },
      layers: [
        {
          id: "basemap",
          type: "raster",
          source: "basemap",
        },
      ],
    },
    center: [104.1954, 35.8617], // 中国中心
    zoom: 4,
    maxZoom: 22,
    pitch: 0,
    maxPitch: 0,
    attributionControl: false,
  });

  // 控件
  map.addControl(
    new maplibregl.NavigationControl({ showCompass: true }),
    "top-right",
  );
  map.addControl(
    new maplibregl.AttributionControl({ compact: true }),
    "bottom-right",
  );
  map.addControl(
    new maplibregl.ScaleControl({ maxWidth: 120, unit: "metric" }),
    "bottom-left",
  );

  // 监听事件
  map.on("mousemove", (e) => {
    cursorCoords.value = { lng: e.lngLat.lng, lat: e.lngLat.lat };
  });
  map.on("mouseleave", () => {
    cursorCoords.value = null;
  });
  map.on("zoom", () => {
    zoom.value = Math.round((map?.getZoom() ?? 4) * 10) / 10;
  });

  // 地图加载完成后通知父组件
  map.on("load", () => {
    if (map) emit("ready", map);
  });

  // 禁止右键旋转（dragRotate 同时处理旋转和倾斜）
  map.dragRotate.disable();
});

onUnmounted(() => {
  map?.remove();
  map = null;
});

// 暴露 map 实例供父组件使用
defineExpose({ map: computed(() => map) });
</script>

<template>
  <div class="relative w-full h-full">
    <!-- MapLibre 容器 -->
    <div ref="mapContainer" class="absolute inset-0" />

    <!-- 状态栏 -->
    <div
      class="absolute bottom-0 left-0 right-0 flex items-center justify-between px-3 py-1.5 text-xs font-mono pointer-events-none"
      style="
        background: rgba(255, 255, 255, 0.88);
        backdrop-filter: blur(4px);
        border-top: 1px solid var(--color-border-subtle);
        color: var(--color-text-secondary);
      "
    >
      <!-- 坐标显示 -->
      <span v-if="cursorCoords">
        {{ cursorCoords.lng.toFixed(6) }}°E &nbsp;
        {{ cursorCoords.lat.toFixed(6) }}°N
      </span>
      <span v-else class="text-slate-400">将鼠标移到地图上查看坐标</span>

      <!-- 缩放级别 -->
      <span>Z{{ zoom }}</span>
    </div>
  </div>
</template>
