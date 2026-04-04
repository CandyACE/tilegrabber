<script setup lang="ts">
/**
 * AreaDraw.vue — terra-draw 绘制工具（支持矩形与多边形）
 *
 * 非可视组件（无模板），通过 defineExpose 暴露控制方法。
 * 在地图上激活绘制模式，完成后 emit bounds 坐标（bbox）。
 */
import { ref, watch, onUnmounted } from "vue";
import type { Map as MaplibreMap } from "maplibre-gl";
import {
  TerraDraw,
  TerraDrawRectangleMode,
  TerraDrawPolygonMode,
  TerraDrawSelectMode,
} from "terra-draw";
import { TerraDrawMapLibreGLAdapter } from "terra-draw-maplibre-gl-adapter";
import type { Bounds } from "~/types/tile-source";

const props = defineProps<{
  map: MaplibreMap;
  /** 当前绘制模式，默认矩形 */
  mode?: "rectangle" | "polygon";
}>();

const emit = defineEmits<{
  "bounds-change": [bounds: Bounds];
  "active-change": [active: boolean];
  /** 实际绘制的多边形顶点坐标（[lng, lat] 数组），矩形绘制时也会发送；清除时发 null */
  "polygon-change": [polygon: [number, number][] | null];
}>();

const isActive = ref(false);
let draw: TerraDraw | null = null;
/** 绘制工具的图层 ID 列表（记录后可随时重新提升至顶层） */
let terraDrawLayerIds: string[] = [];

/** 将已知的 terra-draw 图层移到地图图层栈顶端（解决被底图遮盖问题） */
function liftDrawLayers() {
  for (const id of terraDrawLayerIds) {
    try {
      props.map.moveLayer(id);
    } catch {
      /* 忽略已销毁的地图 */
    }
  }
}

/** 绘制工具统一配色（高对比橙色，在浅色和深色底图上均清晰可见） */
const DRAW_COLOR = "#ff6a00";

function initDraw() {
  if (draw) return;

  // 记录 draw.start() 前所有已有图层
  const beforeIds = new Set(
    (props.map.getStyle()?.layers ?? []).map((l) => l.id),
  );

  draw = new TerraDraw({
    adapter: new TerraDrawMapLibreGLAdapter({
      map: props.map as any,
    }),
    modes: [
      new TerraDrawSelectMode(),
      new TerraDrawRectangleMode({
        styles: {
          fillColor: DRAW_COLOR,
          fillOpacity: 0.18,
          outlineColor: DRAW_COLOR,
          outlineWidth: 3,
        },
      }),
      new TerraDrawPolygonMode({
        styles: {
          fillColor: DRAW_COLOR,
          fillOpacity: 0.18,
          outlineColor: DRAW_COLOR,
          outlineWidth: 3,
          closingPointColor: DRAW_COLOR,
          closingPointOutlineColor: "#ffffff",
        },
      }),
    ],
  });

  draw.start();

  // 记录 terra-draw 新增的图层 ID 列表
  const allLayers = props.map.getStyle()?.layers ?? [];
  terraDrawLayerIds = allLayers
    .filter((l) => !beforeIds.has(l.id))
    .map((l) => l.id);

  // 将 terra-draw 图层提升到顶部
  liftDrawLayers();

  // 绘制完成时提取坐标
  draw.on("finish", (_id: string) => {
    const snapshot = draw!.getSnapshot();
    // 取最后一个 Polygon feature
    const polys = snapshot.filter((f) => f.geometry.type === "Polygon");
    if (polys.length === 0) return;

    const coords = (polys[polys.length - 1]!.geometry as GeoJSON.Polygon)
      .coordinates[0]!;
    if (!coords || coords.length < 4) return;

    // 发送实际多边形坐标（去掉最后一个重复的闭合点）
    const polygonCoords = coords.slice(0, -1) as [number, number][];
    emit("polygon-change", polygonCoords);

    // 从多边形顶点提取 bbox
    const lngs = coords.map((c) => c[0]!);
    const lats = coords.map((c) => c[1]!);
    const bounds: Bounds = {
      west: Math.min(...lngs),
      east: Math.max(...lngs),
      south: Math.min(...lats),
      north: Math.max(...lats),
    };

    emit("bounds-change", bounds);
    // 绘制完成后自动退出绘制模式（回到 select）
    draw!.setMode("select");
    isActive.value = false;
    emit("active-change", false);
  });
}

// 当 mode prop 在绘制过程中改变时，实时切换底层 terra-draw 模式
watch(
  () => props.mode,
  (newMode) => {
    if (!draw || !isActive.value) return;
    draw.setMode(newMode === "polygon" ? "polygon" : "rectangle");
  },
);

/** 切换绘制模式（矩形或多边形，由 mode prop 决定） */
function toggle() {
  if (!draw) initDraw();

  if (isActive.value) {
    draw!.setMode("select");
    isActive.value = false;
    emit("active-change", false);
  } else {
    // 每次激活时都重新把绘制图层提升到最顶层
    // （防止图源切换等操作后底图图层覆盖绘制层）
    liftDrawLayers();
    draw!.setMode(props.mode === "polygon" ? "polygon" : "rectangle");
    isActive.value = true;
    emit("active-change", true);
  }
}

/** 清除所有绘制图形 */
function clear() {
  if (draw) {
    draw.setMode("select"); // 先退出绘制模式，还原鼠标光标
    draw.clear();
  }
  emit("polygon-change", null);
  isActive.value = false;
  emit("active-change", false);
}

onUnmounted(() => {
  draw?.stop();
  draw = null;
});

defineExpose({ toggle, clear, isActive });
</script>

<template>
  <!-- 非可视组件：无 template 内容 -->
  <slot />
</template>
