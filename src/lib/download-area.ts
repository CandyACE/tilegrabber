import type { Bounds } from "~/types/tile-source";

export type AreaPolygons = [number, number][][];
export interface DownloadArea extends Bounds { polygons: AreaPolygons | null }
export interface PresetRegion { id: string; polygons: AreaPolygons }

/** 将所有区域转为外环并集；矩形也需转成环，避免合并时丢失原选框。 */
export function mergeDownloadAreas(areas: DownloadArea[]): DownloadArea {
  if (!areas.length) throw new Error("没有可合并的区域");
  const polygons: AreaPolygons = [];
  for (const area of areas) {
    if (![area.west, area.east, area.south, area.north].every(Number.isFinite)
      || area.west > area.east || area.south > area.north) throw new Error("区域范围无效");
    const faces: AreaPolygons = area.polygons?.length ? area.polygons : [[
      [area.west, area.south], [area.east, area.south],
      [area.east, area.north], [area.west, area.north], [area.west, area.south],
    ]];
    polygons.push(...faces);
  }
  const result = {
    west: Math.min(...areas.map(a => a.west)), east: Math.max(...areas.map(a => a.east)),
    south: Math.min(...areas.map(a => a.south)), north: Math.max(...areas.map(a => a.north)), polygons,
  };
  console.info("[download-area] 区域合并完成", { areas: areas.length, polygons: polygons.length });
  return result;
}

/** 预置行政区边界已确认无需纠偏，保留原坐标并计算下载包围框。 */
export function presetDownloadArea(region: PresetRegion): DownloadArea {
  let west = Infinity, east = -Infinity, south = Infinity, north = -Infinity;
  const polygons = region.polygons.map(ring => ring.map(([lng, lat]): [number, number] => {
    west = Math.min(west, lng); east = Math.max(east, lng);
    south = Math.min(south, lat); north = Math.max(north, lat);
    return [lng, lat];
  }));
  console.info("[download-area] 已读取预置行政区", { region: region.id, coordinateMode: "original" });
  return { west, east, south, north, polygons };
}
