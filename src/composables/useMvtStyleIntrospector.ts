/**
 * useMvtStyleIntrospector
 *
 * 给定任务 ID，下载本地存储中的一块矢量瓦片（pbf）、解析其 source-layer 列表
 * 并按几何类型分类。供 LocalTaskTileLayer 自动生成「骨架样式」使用。
 *
 * 实现要点：
 * - 选取已下载瓦片数量最多的 zoom（一般是 maxZoom），尝试从该层的边界中心点取样
 * - 若中心瓦片缺失（边角任务），fallback 顺序：扫描 z 范围内任意一块已下载瓦片
 * - 解析使用 @mapbox/vector-tile + pbf
 */
import { invoke } from "@tauri-apps/api/core";
import { VectorTile } from "@mapbox/vector-tile";
import Protobuf from "pbf";

export interface MvtLayerSummary {
  id: string;
  hasPolygon: boolean;
  hasLine: boolean;
  hasPoint: boolean;
}

async function tryFetchTile(
  taskId: string,
  z: number,
  x: number,
  y: number,
): Promise<Uint8Array | null> {
  try {
    const raw = await invoke<number[] | Uint8Array>("get_stored_tile", {
      taskId,
      z,
      x,
      y,
    });
    if (raw instanceof Uint8Array) return raw;
    if (Array.isArray(raw)) return new Uint8Array(raw);
    return null;
  } catch {
    return null;
  }
}

async function findSampleTile(
  taskId: string,
  minZoom: number,
  maxZoom: number,
): Promise<Uint8Array | null> {
  // 从最大 zoom 向下尝试，每层先试 0,0 与若干随机 (x,y)
  for (let z = maxZoom; z >= minZoom; z--) {
    const max = (1 << z) - 1;
    // 先试 (0,0)，再试中心，再试若干随机
    const candidates: Array<[number, number]> = [
      [0, 0],
      [Math.floor(max / 2), Math.floor(max / 2)],
    ];
    for (let i = 0; i < 6; i++) {
      candidates.push([
        Math.floor(Math.random() * (max + 1)),
        Math.floor(Math.random() * (max + 1)),
      ]);
    }
    for (const [x, y] of candidates) {
      const data = await tryFetchTile(taskId, z, x, y);
      if (data && data.byteLength > 0) return data;
    }
  }
  return null;
}

export async function introspectMvtLayers(
  taskId: string,
  minZoom: number,
  maxZoom: number,
): Promise<MvtLayerSummary[]> {
  const data = await findSampleTile(taskId, minZoom, maxZoom);
  if (!data) return [];

  let tile: VectorTile;
  try {
    tile = new VectorTile(new Protobuf(new Uint8Array(data)));
  } catch (e) {
    console.warn("[useMvtStyleIntrospector] parse failed", e);
    return [];
  }

  const result: MvtLayerSummary[] = [];
  for (const id of Object.keys(tile.layers)) {
    const layer = tile.layers[id];
    let hasPolygon = false;
    let hasLine = false;
    let hasPoint = false;
    const sampleCount = Math.min(layer.length, 30);
    for (let i = 0; i < sampleCount; i++) {
      const feat = layer.feature(i);
      // type: 1=Point, 2=LineString, 3=Polygon
      if (feat.type === 1) hasPoint = true;
      else if (feat.type === 2) hasLine = true;
      else if (feat.type === 3) hasPolygon = true;
      if (hasPoint && hasLine && hasPolygon) break;
    }
    result.push({ id, hasPolygon, hasLine, hasPoint });
  }
  return result;
}
