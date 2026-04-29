// TileSource 类型定义（与 Rust types.rs 保持镜像）

export type CrsType = "WEB_MERCATOR" | "WGS84" | "TERRAIN" | "UNKNOWN";

/**
 * 瓦片内容的坐标空间类型（与 Rust CoordType 镜像）。
 * - WGS84：标准坐标，无偏移
 * - GCJ02：国测局"火星坐标"（高德、腾讯地图）
 * - BD09：百度坐标
 */
export type CoordType = "WGS84" | "GCJ02" | "BD09";

export interface Bounds {
  west: number;
  east: number;
  south: number;
  north: number;
}

export interface TileSource {
  kind: "lrc" | "lra" | "ovmap" | "wmts" | "tms" | "webcapture" | "threedtiles" | "mbtilefile";
  name: string;
  url_template: string;
  url_param_order: string[];
  subdomains: string[];
  crs: CrsType;
  /** 瓦片内容的坐标空间（默认 WGS84，GCJ02 表示高德/腾讯等国内偏移坐标） */
  coord_type: CoordType;
  tile_size: number;
  north_to_south: boolean;
  bounds: Bounds;
  min_zoom: number;
  max_zoom: number;
  headers: Record<string, string>;
  /** 预计算的 URL 参数，key 对应 URL 模板中的 {key} 占位符，由前端对 param_scripts 求值得到 */
  extra_params: Record<string, string>;
  /** JS 表达式脚本，用于键入时存储、下载前求值成 extra_params */
  param_scripts: Record<string, string>;
  attribution: string | null;
  format: string;
}

// 瓦片计数结果（与 Rust tile_math::TileCount 镜像）
export interface ZoomCount {
  zoom: number;
  count: number;
  x_range: [number, number];
  y_range: [number, number];
}

export interface TileCount {
  per_zoom: ZoomCount[];
  total: number;
}
