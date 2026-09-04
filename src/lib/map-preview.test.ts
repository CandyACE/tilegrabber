import { describe, expect, it } from "vitest";
import {
  getMinimumMapZoom,
  isChinaPreviewCenter,
  isNearlyGlobalBounds,
} from "./map-preview";

describe("地图图层预览相机工具", () => {
  it("识别解析器生成的默认全球范围", () => {
    expect(
      isNearlyGlobalBounds({
        west: -180,
        east: 180,
        south: -85.051129,
        north: 85.051129,
      }),
    ).toBe(true);
    expect(
      isNearlyGlobalBounds({
        west: 120.1,
        east: 120.5,
        south: 31,
        north: 31.5,
      }),
    ).toBe(false);
  });

  it("按 MapLibre 的 512 基准换算图源最小显示层级", () => {
    expect(getMinimumMapZoom(3, 256)).toBe(2);
    expect(getMinimumMapZoom(3, 512)).toBe(3);
    expect(getMinimumMapZoom(3, 1024)).toBe(4);
    expect(getMinimumMapZoom(3, 0)).toBe(2);
  });

  it("GCJ02 图源要求视点位于中国区域", () => {
    expect(isChinaPreviewCenter(104.1954, 35.8617, "GCJ02")).toBe(true);
    expect(isChinaPreviewCenter(0, 0, "GCJ02")).toBe(false);
    expect(isChinaPreviewCenter(0, 0, "WGS84")).toBe(true);
  });
});
