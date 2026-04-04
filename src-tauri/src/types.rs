//! TileGrabber — 核心数据类型
//!
//! 定义统一的瓦片数据源结构体，所有解析器（lrc/lra/wmts/tms/web）
//! 最终都转换为此结构体，前端通过 Tauri 命令获取。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─── 投影/坐标系 ────────────────────────────────────────────────────────────

/// 瓦片地图支持的坐标系类型
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CrsType {
    /// EPSG:3857 — Web 墨卡托 (XYZ/WMTS 标准)
    WebMercator,
    /// EPSG:4326 — WGS84 经纬度 (天地图经纬度服务)
    Wgs84,
    /// 高程地形瓦片 (GeoJSON + 自定义格式)
    Terrain,
    /// 未知投影
    Unknown,
}

impl Default for CrsType {
    fn default() -> Self {
        Self::WebMercator
    }
}
// ─── 坐标空间类型 ────────────────────────────────────────────────────────────

/// 地图内容的坐标空间类型（MapSpaceType）
///
/// 投影（CrsType）决定瓦片网格划分方式，坐标空间类型决定地物坐标的偏移方案。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CoordType {
    /// WGS84 标准坐标，无偏移（默认）
    #[default]
    Wgs84,
    /// GCJ02 国测局坐标（"火星坐标"），腾讯地图、高德地图使用
    Gcj02,
    /// BD09 百度坐标，百度地图使用
    Bd09,
}
// ─── 来源类型 ────────────────────────────────────────────────────────────────

/// 瓦片数据来源类型
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    /// 来自 .lrc 文件
    Lrc,
    /// 来自 .lra 文件 (gzip 压缩)
    Lra,
    /// WMTS 标准服务
    Wmts,
    /// TMS / XYZ URL 模板
    Tms,
    /// 网页抓取
    WebCapture,
    /// 3D Tiles
    ThreeDTiles,
    /// 来自 .ovmap 文件 (OviO Map 二进制格式)
    Ovmap,
}

// ─── 统一 TileSource ─────────────────────────────────────────────────────────

/// 统一的瓦片数据源描述，无论何种来源都转换为此结构体
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TileSource {
    /// 来源类型
    pub kind: SourceKind,

    /// 显示名称（文件名 / 服务标题 / 用户输入）
    pub name: String,

    /// URL 模板，支持以下占位符：
    /// - `{z}` / `%d` — 缩放级别
    /// - `{x}` / `%d` — 列  
    /// - `{y}` / `%d` — 行
    /// - `{s}` — 服务器编号（负载均衡）
    /// - `{tk}` — Token（如有）
    pub url_template: String,

    /// 参数顺序（仅 lrc `%d` 格式需要）
    /// 例如 `["X","Y","Z"]`
    pub url_param_order: Vec<String>,

    /// 负载均衡服务器列表（如 ["t0","t1","t2"]）
    pub subdomains: Vec<String>,

    /// 坐标系
    pub crs: CrsType,

    /// 坐标空间类型（地图内容的坐标偏移方案，如 GCJ02 火星坐标）
    /// 使用 serde(default) 确保旧版 JSON 中缺少此字段时也能正常反序列化
    #[serde(default)]
    pub coord_type: CoordType,

    /// 瓦片尺寸（像素）
    pub tile_size: u32,

    /// 行方向（true = Y 轴从北往南）
    pub north_to_south: bool,

    /// 地理范围（EPSG:4326 经纬度）
    pub bounds: Bounds,

    /// 最小缩放级别
    pub min_zoom: u8,

    /// 最大缩放级别
    pub max_zoom: u8,

    /// HTTP 请求头
    pub headers: HashMap<String, String>,

    /// 预计算的额外 URL 参数（key → 值）。
    /// 在 URL 模板和请求头字段内以 `{key}` 占位符替换。
    /// 由前端对 `param_scripts` 求值后写入。
    #[serde(default)]
    pub extra_params: HashMap<String, String>,

    /// 参数脚本（key → JS 表达式字符串）。
    /// 仅在前端保存，不参与下载逻辑。
    #[serde(default)]
    pub param_scripts: HashMap<String, String>,

    /// 数据版权/归属说明
    pub attribution: Option<String>,

    /// 图像格式：png / jpg / webp / terrain
    pub format: String,
}

/// 地理范围（WGS84 经纬度）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Bounds {
    pub west: f64,
    pub east: f64,
    pub south: f64,
    pub north: f64,
}

impl Bounds {
    pub fn new(west: f64, east: f64, south: f64, north: f64) -> Self {
        Self { west, east, south, north }
    }

    /// 转换为 GeoJSON bbox 数组 [west, south, east, north]
    pub fn to_geojson_bbox(&self) -> [f64; 4] {
        [self.west, self.south, self.east, self.north]
    }

    /// 检查范围是否合法
    pub fn is_valid(&self) -> bool {
        self.west < self.east
            && self.south < self.north
            && self.west >= -180.0
            && self.east <= 180.0
            && self.south >= -90.0
            && self.north <= 90.0
    }
}

impl Default for TileSource {
    fn default() -> Self {
        Self {
            kind: SourceKind::Tms,
            name: String::new(),
            url_template: String::new(),
            url_param_order: vec!["X".into(), "Y".into(), "Z".into()],
            subdomains: vec![],
            crs: CrsType::WebMercator,
            coord_type: CoordType::Wgs84,
            tile_size: 256,
            north_to_south: true,
            bounds: Bounds::new(-180.0, 180.0, -85.051129, 85.051129),
            min_zoom: 0,
            max_zoom: 18,
            headers: HashMap::new(),
            extra_params: HashMap::new(),
            param_scripts: HashMap::new(),
            attribution: None,
            format: "png".into(),
        }
    }
}

impl TileSource {
    /// 将 lrc `%d` 格式的 URL 转换为标准 `{z}/{x}/{y}` 格式
    /// 同时将 `{$serverpart}` 替换为 `{s}`
    pub fn normalize_url(url: &str, param_order: &[String]) -> String {
        let mut result = url.replace("{$serverpart}", "{s}");

        // %d 按 param_order 顺序替换
        let placeholders: Vec<&str> = param_order
            .iter()
            .map(|p| match p.to_uppercase().as_str() {
                "X" => "{x}",
                "Y" => "{y}",
                "Z" => "{z}",
                _ => "{x}",
            })
            .collect();

        for ph in placeholders {
            if let Some(pos) = result.find("%d") {
                result.replace_range(pos..pos + 2, ph);
            }
        }

        result
    }
}
