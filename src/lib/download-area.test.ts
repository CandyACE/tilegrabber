import { describe, expect, it } from "vitest";
import { mergeDownloadAreas, presetDownloadArea } from "./download-area";
import regions from "~/assets/regions/china.json";

describe("下载区域组合", () => {
  it("保留原矩形与分离多面，不把区域之间的空隙变成下载面", () => {
    const result = mergeDownloadAreas([
      { west: 0, east: 1, south: 0, north: 1, polygons: null },
      { west: 10, east: 11, south: 10, north: 11, polygons: [[[10,10],[11,10],[11,11],[10,11]]] },
    ]);
    expect(result).toMatchObject({ west: 0, east: 11, south: 0, north: 11 });
    expect(result.polygons).toHaveLength(2);
    expect(result.polygons![0]).toContainEqual([0, 0]);
    expect(() => mergeDownloadAreas([])).toThrow();
  });

  it("全国及 34 个省级边界均有效且保留离岛", () => {
    expect(regions).toHaveLength(35);
    expect(new Set(regions.map(r => r.id)).size).toBe(35);
    for (const region of regions) {
      // JSON 导入没有元组类型，测试通过运行时坐标校验再交给业务转换。
      const area = presetDownloadArea(region as Parameters<typeof presetDownloadArea>[0]);
      expect(area.west).toBeLessThan(area.east);
      expect(area.south).toBeLessThan(area.north);
      expect(area.polygons!.length).toBeGreaterThan(0);
      for (const ring of area.polygons!) {
        expect(ring.length).toBeGreaterThanOrEqual(4);
        expect(ring.every(([lng, lat]) => Number.isFinite(lng) && Math.abs(lng) <= 180 && Number.isFinite(lat) && Math.abs(lat) <= 90)).toBe(true);
      }
    }
    expect(regions.find(r => r.id === "浙江省")!.polygons.length).toBeGreaterThan(1);
  });

  it("预置边界始终保留源坐标，并从原坐标计算包围框", () => {
    const region: Parameters<typeof presetDownloadArea>[0] = {
      id: "测试", polygons: [[[116.4, 39.9], [116.5, 39.9], [116.4, 40], [116.4, 39.9]]],
    };
    const area = presetDownloadArea(region);
    expect(area.polygons).toEqual(region.polygons);
    expect(area).toMatchObject({ west: 116.4, east: 116.5, south: 39.9, north: 40 });
  });
});
