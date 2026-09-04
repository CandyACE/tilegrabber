/**
 * GCJ02（"火星坐标"）纠偏工具
 *
 * 高德地图、腾讯地图等国内服务使用 GCJ02 坐标系，
 * 其瓦片内容相对 WGS84 存在约 100–700 m 的偏移。
 * 本模块提供 WGS84→GCJ02 转换及瓦片纠偏所需的像素偏移量计算。
 */

const PI = Math.PI;
// WGS84 椭球参数
const a = 6378245.0;
const ee = 0.00669342162296594323;

/** 判断是否在中国大陆范围外（范围外无需纠偏） */
export function outOfChina(lng: number, lat: number): boolean {
  return lng < 72.004 || lng > 137.8347 || lat < 0.8293 || lat > 55.8271;
}

function transformLat(x: number, y: number): number {
  let ret = -100 + 2 * x + 3 * y + 0.2 * y * y + 0.1 * x * y + 0.2 * Math.sqrt(Math.abs(x));
  ret += (20 * Math.sin(6 * x * PI) + 20 * Math.sin(2 * x * PI)) * (2 / 3);
  ret += (20 * Math.sin(y * PI) + 40 * Math.sin((y / 3) * PI)) * (2 / 3);
  ret += (160 * Math.sin((y / 12) * PI) + 320 * Math.sin((y * PI) / 30)) * (2 / 3);
  return ret;
}

function transformLng(x: number, y: number): number {
  let ret = 300 + x + 2 * y + 0.1 * x * x + 0.1 * x * y + 0.1 * Math.sqrt(Math.abs(x));
  ret += (20 * Math.sin(6 * x * PI) + 20 * Math.sin(2 * x * PI)) * (2 / 3);
  ret += (20 * Math.sin(x * PI) + 40 * Math.sin((x / 3) * PI)) * (2 / 3);
  ret += (150 * Math.sin((x / 12) * PI) + 300 * Math.sin((x / 30) * PI)) * (2 / 3);
  return ret;
}

/** WGS84 → GCJ02 坐标转换（公开算法） */
export function wgs84ToGcj02(lng: number, lat: number): { lng: number; lat: number } {
  if (outOfChina(lng, lat)) return { lng, lat };

  let dLat = transformLat(lng - 105, lat - 35);
  let dLng = transformLng(lng - 105, lat - 35);

  const radLat = (lat / 180) * PI;
  let magic = Math.sin(radLat);
  magic = 1 - ee * magic * magic;
  const sqrtMagic = Math.sqrt(magic);

  dLat = (dLat * 180) / (((a * (1 - ee)) / (magic * sqrtMagic)) * PI);
  dLng = (dLng * 180) / ((a / sqrtMagic) * Math.cos(radLat) * PI);

  return { lng: lng + dLng, lat: lat + dLat };
}

/** WebMercator 对数纬度（Mercator Y） */
function mercY(lat: number): number {
  const r = (lat * PI) / 180;
  return Math.log(Math.tan(PI / 4 + r / 2));
}

/** 计算 XYZ 瓦片 (z, x, y) 中心点的 WGS84 经纬度 */
export function tileCenterWgs84(z: number, x: number, y: number): { lng: number; lat: number } {
  const n = Math.pow(2, z);
  return {
    lng: ((x + 0.5) / n) * 360 - 180,
    lat: (Math.atan(Math.sinh(PI * (1 - (2 * (y + 0.5)) / n))) * 180) / PI,
  };
}

/**
 * 计算 GCJ02 纠偏所需的全局像素偏移 (dx, dy)（缩放级别 z 下）。
 *
 * 语义：Gaode 等 GCJ02 瓦片中地物，在 WGS84 地图上显示时，
 * 相对于正确位置偏移了 (dx, dy) 像素：
 *   dx > 0 = 偏东（屏幕右方）
 *   dy < 0 = 偏北（屏幕上方，WebMercator y 减小方向）
 *
 * 纠偏策略：为输出瓦片 (z, x, y)，从 GCJ02 来源获取从
 * (x + floor(dx/tileSize), y + floor(dy/tileSize)) 开始的 2×2 块瓦片并合成，
 * 使地物内容对齐到 WGS84 坐标系。
 *
 * 注意：仅适用于 XYZ（north_to_south=true）方案的来源。
 */
export function gcj02PixelDelta(
  z: number,
  x: number,
  y: number,
  tileSize = 256,
): { dx: number; dy: number } {
  const { lng, lat } = tileCenterWgs84(z, x, y);
  const gcj = wgs84ToGcj02(lng, lat);

  const normalizedTileSize =
    Number.isFinite(tileSize) && tileSize > 0 ? tileSize : 256;
  if (normalizedTileSize !== tileSize) {
    console.warn("[gcj02] 收到无效瓦片尺寸，已回退到 256", { tileSize });
  }
  const totalPx = normalizedTileSize * Math.pow(2, z);

  // 经度偏差→屏幕 x 像素（东正）
  const dx = Math.round(((gcj.lng - lng) / 360) * totalPx);

  // 纬度偏差→屏幕 y 像素（WebMercator y 向南增大，故 lat↑ 对应 dy < 0）
  const dy = Math.round(((mercY(lat) - mercY(gcj.lat)) / (2 * PI)) * totalPx);

  return { dx, dy };
}
