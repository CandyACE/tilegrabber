import type { Bounds, CoordType } from "~/types/tile-source";

/** 御图默认的中国视点，用于全球范围的 GCJ02 图源首次定位。 */
export const CHINA_PREVIEW_CENTER: [number, number] = [104.1954, 35.8617];

/**
 * 判断图源范围是否只是解析器提供的默认全球范围。
 *
 * 这类范围不能直接用于业务定位，否则最小层级大于 0 的图源会被缩放到
 * 世界视图并停止请求瓦片。
 */
export function isNearlyGlobalBounds(bounds: Bounds): boolean {
  const { west, east, south, north } = bounds;
  return (
    east - west > 300 ||
    (west < -150 && east > 150 && north - south > 140)
  );
}

/**
 * 将图源最小层级换算为 MapLibre 相机的最小显示层级。
 *
 * MapLibre 的基准瓦片尺寸为 512；256 像素图源在相机层级 N 时会请求
 * 图源层级 N+1，因此需要保留这一层换算关系。
 */
export function getMinimumMapZoom(
  sourceMinZoom: number,
  tileSize: number,
): number {
  const normalizedTileSize =
    Number.isFinite(tileSize) && tileSize > 0 ? tileSize : 256;
  const normalizedMinZoom = Number.isFinite(sourceMinZoom)
    ? sourceMinZoom
    : 0;
  return Math.max(
    0,
    normalizedMinZoom + Math.log2(normalizedTileSize / 512),
  );
}

/** 判断当前视点是否位于中国大陆图源通常覆盖的经纬度范围内。 */
export function isChinaPreviewCenter(
  lng: number,
  lat: number,
  coordType: CoordType,
): boolean {
  if (coordType !== "GCJ02") return true;
  return lng >= 72 && lng <= 138 && lat >= 0 && lat <= 56;
}
