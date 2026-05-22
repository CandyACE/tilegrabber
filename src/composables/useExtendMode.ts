import { ref, computed } from "vue";
import type { Bounds } from "~/types/tile-source";

export interface ExtendModeContext {
  taskId: string;
  taskName: string;
  originalBounds: Bounds;
  originalPolygon: [number, number][] | null;
  originalMinZoom: number;
  originalMaxZoom: number;
}

const context = ref<ExtendModeContext | null>(null);

export function useExtendMode() {
  const isActive = computed(() => context.value !== null);

  function start(ctx: ExtendModeContext) {
    context.value = ctx;
  }

  function cancel() {
    context.value = null;
  }

  function unionBounds(a: Bounds, b: Bounds): Bounds {
    return {
      west: Math.min(a.west, b.west),
      east: Math.max(a.east, b.east),
      south: Math.min(a.south, b.south),
      north: Math.max(a.north, b.north),
    };
  }

  return { context, isActive, start, cancel, unionBounds };
}
