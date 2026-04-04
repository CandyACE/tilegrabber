// TileSource 类型定义（与 Rust types.rs 保持镜像）

export type CrsType = "WEB_MERCATOR" | "WGS84" | "TERRAIN" | "UNKNOWN";

export interface Bounds {
  west: number;
  east: number;
  south: number;
  north: number;
}

export interface TileSource {
  kind: "lrc" | "lra" | "ovmap" | "wmts" | "tms" | "webcapture" | "threedtiles";
  name: string;
  url_template: string;
  url_param_order: string[];
  subdomains: string[];
  crs: CrsType;
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
