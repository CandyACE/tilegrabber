//! TileGrabber — 瓦片数学计算模块
//!
//! 提供 WebMercator(EPSG:3857) 和 WGS84(EPSG:4326) 两种坐标系下的：
//! - 经纬度 ↔ 瓦片坐标转换
//! - 范围内瓦片枚举
//! - 瓦片数量统计
//! - GeoJSON 网格线生成

use crate::types::{Bounds, CrsType};
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

// ─── 公共类型 ────────────────────────────────────────────────────────────────

/// 单个瓦片坐标
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TileCoord {
    pub z: u8,
    pub x: u32,
    pub y: u32,
}

/// 瓦片数量统计结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TileCount {
    /// 按层级分组的瓦片数量
    pub per_zoom: Vec<ZoomCount>,
    /// 总瓦片数量
    pub total: u64,
}

/// 单层级瓦片数量
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZoomCount {
    pub zoom: u8,
    pub count: u64,
    /// 列范围 (x_min, x_max)
    pub x_range: (u32, u32),
    /// 行范围 (y_min, y_max)
    pub y_range: (u32, u32),
}

/// 瓦片网格 GeoJSON (用于前端预览)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TileGrid {
    /// GeoJSON FeatureCollection 字符串
    pub geojson: String,
    /// 网格中的瓦片数量
    pub tile_count: u64,
}

// ─── WebMercator (EPSG:3857) 计算 ───────────────────────────────────────────

/// 经纬度转 WebMercator 瓦片坐标 (XYZ/TMS 标准)
///
/// - lng: 经度 [-180, 180]
/// - lat: 纬度 [-85.051129, 85.051129]
/// - zoom: 层级 [0, 22]
pub fn lng_lat_to_tile_xyz(lng: f64, lat: f64, zoom: u8) -> (u32, u32) {
    let n = 2u32.pow(zoom as u32) as f64;
    let x = ((lng + 180.0) / 360.0 * n).floor() as u32;
    let lat_rad = lat.to_radians();
    let y = ((1.0 - (lat_rad.tan() + 1.0 / lat_rad.cos()).ln() / PI) / 2.0 * n).floor() as u32;
    // 确保不超出范围
    let max = (n as u32).saturating_sub(1);
    (x.min(max), y.min(max))
}

/// WebMercator 瓦片坐标转经纬度 (瓦片左上角)
pub fn tile_xyz_to_lng_lat(x: u32, y: u32, zoom: u8) -> (f64, f64) {
    let n = 2u32.pow(zoom as u32) as f64;
    let lng = (x as f64) / n * 360.0 - 180.0;
    let lat_rad = ((1.0 - 2.0 * (y as f64) / n) * PI).sinh().atan();
    let lat = lat_rad.to_degrees();
    (lng, lat)
}

/// WebMercator 瓦片坐标转地理范围 (west, south, east, north)
pub fn tile_xyz_to_bounds(x: u32, y: u32, zoom: u8) -> Bounds {
    let (west, north) = tile_xyz_to_lng_lat(x, y, zoom);
    let (east, south) = tile_xyz_to_lng_lat(x + 1, y + 1, zoom);
    Bounds {
        west,
        east,
        south,
        north,
    }
}

/// 按坐标系将瓦片坐标转换为经纬度范围（用于闪烁效果）
pub fn tile_to_lonlat_bounds(x: u32, y: u32, zoom: u8, crs: &CrsType) -> Bounds {
    match crs {
        CrsType::Wgs84 => {
            let cols = 2u32.pow(zoom as u32) as f64;
            let rows = 2u32.pow((zoom as u32).saturating_sub(1)) as f64;
            let west = (x as f64) / cols * 360.0 - 180.0;
            let east = (x as f64 + 1.0) / cols * 360.0 - 180.0;
            let north = 90.0 - (y as f64) / rows * 180.0;
            let south = 90.0 - (y as f64 + 1.0) / rows * 180.0;
            Bounds {
                west,
                east,
                south,
                north,
            }
        }
        _ => tile_xyz_to_bounds(x, y, zoom),
    }
}

/// 判断瓦片是否严格位于指定范围内（瓦片四至完全被范围包含，不含仅相交的边界瓦片）
///
/// 仅支持 WebMercator 坐标系；WGS84 使用近似线性映射。
pub fn tile_strictly_within_bounds(
    x: u32,
    y: u32,
    zoom: u8,
    bounds: &Bounds,
    crs: &CrsType,
) -> bool {
    let tb = match crs {
        CrsType::Wgs84 => {
            let cols = 2u32.pow(zoom as u32) as f64;
            let rows = 2u32.pow((zoom as u32).saturating_sub(1)) as f64;
            let west = (x as f64) / cols * 360.0 - 180.0;
            let east = (x as f64 + 1.0) / cols * 360.0 - 180.0;
            let north = 90.0 - (y as f64) / rows * 180.0;
            let south = 90.0 - (y as f64 + 1.0) / rows * 180.0;
            Bounds {
                west,
                east,
                south,
                north,
            }
        }
        _ => tile_xyz_to_bounds(x, y, zoom),
    };
    tb.west >= bounds.west
        && tb.east <= bounds.east
        && tb.south >= bounds.south
        && tb.north <= bounds.north
}

/// 计算 WebMercator 范围内指定层级的瓦片列范围
pub fn bounds_to_tile_range_xyz(bounds: &Bounds, zoom: u8) -> ((u32, u32), (u32, u32)) {
    // 裁剪到有效经纬度范围
    let west = bounds.west.max(-180.0).min(180.0);
    let east = bounds.east.max(-180.0).min(180.0);
    let south = bounds.south.max(-85.051129).min(85.051129);
    let north = bounds.north.max(-85.051129).min(85.051129);

    let (x_min, y_min) = lng_lat_to_tile_xyz(west, north, zoom); // north → y_min
    let (x_max, y_max) = lng_lat_to_tile_xyz(east, south, zoom); // south → y_max

    let n = 2u32.pow(zoom as u32).saturating_sub(1);
    ((x_min.min(n), x_max.min(n)), (y_min.min(n), y_max.min(n)))
}

// ─── WGS84 (EPSG:4326) 计算 ─────────────────────────────────────────────────
//
// 天地图经纬度瓦片服务 (TianDiTuLatLon) 使用此坐标系。
// 瓦片大小在经度方向：360 / 2^zoom，在纬度方向：180 / 2^(zoom-1)
// zoom 0 时有 2 列 × 1 行瓦片（覆盖全球）

/// 经纬度转 WGS84 瓦片坐标
pub fn lng_lat_to_tile_wgs84(lng: f64, lat: f64, zoom: u8) -> (u32, u32) {
    let cols = 2u32.pow(zoom as u32) as f64;
    let rows = 2u32.pow((zoom as u32).saturating_sub(1)) as f64;

    let x = ((lng + 180.0) / 360.0 * cols).floor() as u32;
    let y = ((90.0 - lat) / 180.0 * rows).floor() as u32;

    let max_x = (cols as u32).saturating_sub(1);
    let max_y = (rows as u32).saturating_sub(1);
    (x.min(max_x), y.min(max_y))
}

/// 计算 WGS84 范围内指定层级的瓦片列范围
pub fn bounds_to_tile_range_wgs84(bounds: &Bounds, zoom: u8) -> ((u32, u32), (u32, u32)) {
    let west = bounds.west.max(-180.0).min(180.0);
    let east = bounds.east.max(-180.0).min(180.0);
    let south = bounds.south.max(-90.0).min(90.0);
    let north = bounds.north.max(-90.0).min(90.0);

    let (x_min, y_min) = lng_lat_to_tile_wgs84(west, north, zoom);
    let (x_max, y_max) = lng_lat_to_tile_wgs84(east, south, zoom);

    let cols = 2u32.pow(zoom as u32).saturating_sub(1);
    let rows = 2u32.pow((zoom as u32).saturating_sub(1)).saturating_sub(1);

    (
        (x_min.min(cols), x_max.min(cols)),
        (y_min.min(rows), y_max.min(rows)),
    )
}

// ─── 统一接口 ────────────────────────────────────────────────────────────────

/// 统计范围内所有层级的瓦片数量
pub fn count_tiles(bounds: &Bounds, min_zoom: u8, max_zoom: u8, crs: &CrsType) -> TileCount {
    let mut per_zoom = Vec::new();
    let mut total: u64 = 0;

    for zoom in min_zoom..=max_zoom {
        let ((x_min, x_max), (y_min, y_max)) = match crs {
            CrsType::Wgs84 => bounds_to_tile_range_wgs84(bounds, zoom),
            _ => bounds_to_tile_range_xyz(bounds, zoom),
        };

        let cols = (x_max as u64).saturating_sub(x_min as u64) + 1;
        let rows = (y_max as u64).saturating_sub(y_min as u64) + 1;
        let count = cols * rows;

        per_zoom.push(ZoomCount {
            zoom,
            count,
            x_range: (x_min, x_max),
            y_range: (y_min, y_max),
        });
        total += count;
    }

    TileCount { per_zoom, total }
}

/// 枚举范围内所有瓦片坐标（上限 50 万以避免内存爆炸）
pub fn enumerate_tiles(
    bounds: &Bounds,
    min_zoom: u8,
    max_zoom: u8,
    crs: &CrsType,
    limit: Option<u64>,
) -> Vec<TileCoord> {
    let max_tiles = limit.unwrap_or(500_000);
    let mut tiles = Vec::new();

    'outer: for zoom in min_zoom..=max_zoom {
        let ((x_min, x_max), (y_min, y_max)) = match crs {
            CrsType::Wgs84 => bounds_to_tile_range_wgs84(bounds, zoom),
            _ => bounds_to_tile_range_xyz(bounds, zoom),
        };

        for y in y_min..=y_max {
            for x in x_min..=x_max {
                if tiles.len() as u64 >= max_tiles {
                    break 'outer;
                }
                tiles.push(TileCoord { z: zoom, x, y });
            }
        }
    }

    tiles
}

/// 生成指定层级的瓦片网格 GeoJSON（用于地图可视化预览）
///
/// 返回 GeoJSON FeatureCollection，包含:
/// - 所有瓦片的 polygon 边界线（type: "Feature", geometry: Polygon）
///
/// 瓦片数量超过 `max_tiles` 时降级：只返回范围框而不绘制网格
pub fn generate_tile_grid_geojson(
    bounds: &Bounds,
    zoom: u8,
    crs: &CrsType,
    max_tiles: u32,
) -> (String, u64) {
    let ((x_min, x_max), (y_min, y_max)) = match crs {
        CrsType::Wgs84 => bounds_to_tile_range_wgs84(bounds, zoom),
        _ => bounds_to_tile_range_xyz(bounds, zoom),
    };

    let cols = (x_max as u64).saturating_sub(x_min as u64) + 1;
    let rows = (y_max as u64).saturating_sub(y_min as u64) + 1;
    let count = cols * rows;

    if count > max_tiles as u64 {
        // 瓦片太多，只返回范围框
        let geojson = bounds_to_geojson_feature_collection(bounds);
        return (geojson, count);
    }

    let mut features = Vec::new();

    for y in y_min..=y_max {
        for x in x_min..=x_max {
            let tile_b = tile_xyz_to_bounds(x, y, zoom);
            let feature = format!(
                r#"{{"type":"Feature","properties":{{"z":{},"x":{},"y":{}}},"geometry":{{"type":"Polygon","coordinates":[[{},{},{},{},{}]]}}}}"#,
                zoom,
                x,
                y,
                format_coord(tile_b.west, tile_b.north),
                format_coord(tile_b.east, tile_b.north),
                format_coord(tile_b.east, tile_b.south),
                format_coord(tile_b.west, tile_b.south),
                format_coord(tile_b.west, tile_b.north),
            );
            features.push(feature);
        }
    }

    let geojson = format!(
        r#"{{"type":"FeatureCollection","features":[{}]}}"#,
        features.join(",")
    );

    (geojson, count)
}

fn format_coord(lng: f64, lat: f64) -> String {
    format!("[{:.6},{:.6}]", lng, lat)
}

fn bounds_to_geojson_feature_collection(bounds: &Bounds) -> String {
    format!(
        r#"{{"type":"FeatureCollection","features":[{{"type":"Feature","properties":{{}},"geometry":{{"type":"Polygon","coordinates":[[{},{},{},{},{}]]}}}}]}}"#,
        format_coord(bounds.west, bounds.north),
        format_coord(bounds.east, bounds.north),
        format_coord(bounds.east, bounds.south),
        format_coord(bounds.west, bounds.south),
        format_coord(bounds.west, bounds.north),
    )
}

// ─── 多边形交叉检测 ─────────────────────────────────────────────────────────

/// 向量叉积辅助
#[inline]
fn cross2d(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

#[inline]
fn sub2d(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

/// 判断线段 (p1,p2) 与 (p3,p4) 是否严格相交
fn segments_intersect(p1: [f64; 2], p2: [f64; 2], p3: [f64; 2], p4: [f64; 2]) -> bool {
    let d1 = cross2d(sub2d(p2, p1), sub2d(p3, p1));
    let d2 = cross2d(sub2d(p2, p1), sub2d(p4, p1));
    let d3 = cross2d(sub2d(p4, p3), sub2d(p1, p3));
    let d4 = cross2d(sub2d(p4, p3), sub2d(p2, p3));
    ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
}

/// 射线投射法：判断点 (lng, lat) 是否在多边形内部
///
/// `polygon` 中坐标格式为 `[lng, lat]`；首尾不需要重复闭合。
pub fn point_in_polygon(lng: f64, lat: f64, polygon: &[[f64; 2]]) -> bool {
    let n = polygon.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let xi = polygon[i][0];
        let yi = polygon[i][1];
        let xj = polygon[j][0];
        let yj = polygon[j][1];
        if ((yi > lat) != (yj > lat)) && (lng < (xj - xi) * (lat - yi) / (yj - yi) + xi) {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// 判断点是否位于矩形内部或边界上。
///
/// 这里使用很小的浮点容差，避免瓦片边界与多边形边界恰好重合时被误判为不相交。
#[inline]
fn point_in_or_on_bounds(point: [f64; 2], bounds: &Bounds) -> bool {
    const EPSILON: f64 = 1e-10;
    point[0] >= bounds.west - EPSILON
        && point[0] <= bounds.east + EPSILON
        && point[1] >= bounds.south - EPSILON
        && point[1] <= bounds.north + EPSILON
}

/// 判断三点的方向关系。
#[inline]
fn orientation(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    cross2d(sub2d(b, a), sub2d(c, a))
}

/// 判断点是否在线段上（含端点）。
#[inline]
fn point_on_segment(point: [f64; 2], start: [f64; 2], end: [f64; 2]) -> bool {
    const EPSILON: f64 = 1e-10;
    orientation(start, end, point).abs() <= EPSILON
        && point[0] >= start[0].min(end[0]) - EPSILON
        && point[0] <= start[0].max(end[0]) + EPSILON
        && point[1] >= start[1].min(end[1]) - EPSILON
        && point[1] <= start[1].max(end[1]) + EPSILON
}

/// 判断两条线段是否相交或接触。
///
/// `segments_intersect` 只识别严格相交；严格裁剪的快速路径还需要识别边界接触、
/// 共线重叠等情况，否则凹多边形的缺口可能让瓦片被错误地当成完整内部瓦片。
fn segments_intersect_or_touch(p1: [f64; 2], p2: [f64; 2], p3: [f64; 2], p4: [f64; 2]) -> bool {
    if segments_intersect(p1, p2, p3, p4) {
        return true;
    }

    const EPSILON: f64 = 1e-10;
    let d1 = orientation(p1, p2, p3);
    let d2 = orientation(p1, p2, p4);
    let d3 = orientation(p3, p4, p1);
    let d4 = orientation(p3, p4, p2);

    (d1.abs() <= EPSILON && point_on_segment(p3, p1, p2))
        || (d2.abs() <= EPSILON && point_on_segment(p4, p1, p2))
        || (d3.abs() <= EPSILON && point_on_segment(p1, p3, p4))
        || (d4.abs() <= EPSILON && point_on_segment(p2, p3, p4))
}

/// 判断多边形边界是否进入、穿过或接触瓦片范围。
///
/// 只有多边形边界完全位于瓦片外部时，且瓦片四角均在同一多边形内，才可跳过
/// 像素级裁剪。仅检查四角会漏掉凹多边形伸入瓦片内部的缺口。
fn polygon_boundary_touches_bounds(polygon: &[[f64; 2]], bounds: &Bounds) -> bool {
    if polygon.len() < 3 {
        return true;
    }

    let tile_edges = [
        ([bounds.west, bounds.north], [bounds.east, bounds.north]),
        ([bounds.east, bounds.north], [bounds.east, bounds.south]),
        ([bounds.east, bounds.south], [bounds.west, bounds.south]),
        ([bounds.west, bounds.south], [bounds.west, bounds.north]),
    ];

    for index in 0..polygon.len() {
        let start = polygon[index];
        let end = polygon[(index + 1) % polygon.len()];
        if point_in_or_on_bounds(start, bounds)
            || point_in_or_on_bounds(end, bounds)
            || tile_edges.iter().any(|&(edge_start, edge_end)| {
                segments_intersect_or_touch(start, end, edge_start, edge_end)
            })
        {
            return true;
        }
    }
    false
}

/// 判断点是否位于多面集合中的任意一个面内。
///
/// 多面区域按各外环的并集处理；KML 内环在解析阶段已被排除，避免把洞误当成下载面。
pub fn point_in_polygons(lng: f64, lat: f64, polygons: &[Vec<[f64; 2]>]) -> bool {
    polygons
        .iter()
        .any(|polygon| point_in_polygon(lng, lat, polygon))
}

/// 判断瓦片是否完全位于多面集合中的同一个面内。
///
/// 不能仅判断四个角点是否分别落在多面并集内：当瓦片跨越两个
/// 不相连的面或位于两个面之间的空白区域时，四个角点可能都命中，
/// 但瓦片内部仍需要像素级裁剪。
pub fn tile_fully_within_polygons(
    x: u32,
    y: u32,
    zoom: u8,
    polygons: &[Vec<[f64; 2]>],
    crs: &CrsType,
) -> bool {
    let tb = tile_to_lonlat_bounds(x, y, zoom, crs);
    let corners = [
        [tb.west, tb.north],
        [tb.east, tb.north],
        [tb.east, tb.south],
        [tb.west, tb.south],
    ];
    polygons.iter().any(|polygon| {
        corners
            .iter()
            .all(|corner| point_in_polygon(corner[0], corner[1], polygon))
            && !polygon_boundary_touches_bounds(polygon, &tb)
    })
}

/// 解析任务中保存的多边形 JSON。
///
/// 新格式为 `[[[lng, lat], ...], ...]`；旧版本单面格式
/// `[[lng, lat], ...]` 也会被包装成只有一个面的集合。
pub fn parse_polygon_geometry(value: &str) -> Option<Vec<Vec<[f64; 2]>>> {
    if let Ok(polygons) = serde_json::from_str::<Vec<Vec<[f64; 2]>>>(value) {
        if !polygons.is_empty() {
            return Some(polygons);
        }
    }
    serde_json::from_str::<Vec<[f64; 2]>>(value)
        .ok()
        .filter(|polygon| !polygon.is_empty())
        .map(|polygon| vec![polygon])
}

/// 判断瓦片是否与多边形相交（用于下载阶段过滤不必要的瓦片）
///
/// 存在以下三种相交情形之一时返回 true：
/// 1. 多边形任意顶点落在瓦片范围内
/// 2. 瓦片任意角点落在多边形内部
/// 3. 多边形任意边与瓦片任意边相交
pub fn tile_intersects_polygon(
    x: u32,
    y: u32,
    zoom: u8,
    polygon: &[[f64; 2]],
    crs: &CrsType,
) -> bool {
    let n = polygon.len();
    if n < 3 {
        return true; // 无效多边形，不过滤
    }
    let tb = tile_to_lonlat_bounds(x, y, zoom, crs);

    // 快速排除：多边形 bbox 与瓦片 bbox 无交集
    let poly_min_lng = polygon.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
    let poly_max_lng = polygon
        .iter()
        .map(|p| p[0])
        .fold(f64::NEG_INFINITY, f64::max);
    let poly_min_lat = polygon.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
    let poly_max_lat = polygon
        .iter()
        .map(|p| p[1])
        .fold(f64::NEG_INFINITY, f64::max);

    if tb.east <= poly_min_lng
        || tb.west >= poly_max_lng
        || tb.north <= poly_min_lat
        || tb.south >= poly_max_lat
    {
        return false;
    }

    // 情形1：多边形顶点落在瓦片内
    for v in polygon {
        if v[0] >= tb.west && v[0] <= tb.east && v[1] >= tb.south && v[1] <= tb.north {
            return true;
        }
    }

    // 情形2：瓦片角点落在多边形内
    let corners = [
        [tb.west, tb.north],
        [tb.east, tb.north],
        [tb.east, tb.south],
        [tb.west, tb.south],
    ];
    for corner in &corners {
        if point_in_polygon(corner[0], corner[1], polygon) {
            return true;
        }
    }

    // 情形3：多边形边与瓦片边相交
    let tile_edges: [([f64; 2], [f64; 2]); 4] = [
        ([tb.west, tb.north], [tb.east, tb.north]),
        ([tb.east, tb.north], [tb.east, tb.south]),
        ([tb.east, tb.south], [tb.west, tb.south]),
        ([tb.west, tb.south], [tb.west, tb.north]),
    ];
    for i in 0..n {
        let p1 = polygon[i];
        let p2 = polygon[(i + 1) % n];
        for &(e1, e2) in &tile_edges {
            if segments_intersect(p1, p2, e1, e2) {
                return true;
            }
        }
    }

    false
}

/// 判断瓦片是否与多面集合中的任意一个面相交。
pub fn tile_intersects_polygons(
    x: u32,
    y: u32,
    zoom: u8,
    polygons: &[Vec<[f64; 2]>],
    crs: &CrsType,
) -> bool {
    polygons
        .iter()
        .any(|polygon| tile_intersects_polygon(x, y, zoom, polygon, crs))
}

/// 按多边形范围枚举瓦片（仅返回与多边形相交的瓦片，跳过外围矩形中不相交的部分）
pub fn enumerate_tiles_with_polygon(
    bounds: &Bounds,
    min_zoom: u8,
    max_zoom: u8,
    crs: &CrsType,
    polygon: &[[f64; 2]],
    limit: Option<u64>,
) -> Vec<TileCoord> {
    let max_tiles = limit.unwrap_or(500_000);
    let mut tiles = Vec::new();

    'outer: for zoom in min_zoom..=max_zoom {
        let ((x_min, x_max), (y_min, y_max)) = match crs {
            CrsType::Wgs84 => bounds_to_tile_range_wgs84(bounds, zoom),
            _ => bounds_to_tile_range_xyz(bounds, zoom),
        };

        for y in y_min..=y_max {
            for x in x_min..=x_max {
                if tiles.len() as u64 >= max_tiles {
                    break 'outer;
                }
                if tile_intersects_polygon(x, y, zoom, polygon, crs) {
                    tiles.push(TileCoord { z: zoom, x, y });
                }
            }
        }
    }

    tiles
}

/// 按多面范围枚举瓦片，只保留与任意一个多边形相交的瓦片。
pub fn enumerate_tiles_with_polygons(
    bounds: &Bounds,
    min_zoom: u8,
    max_zoom: u8,
    crs: &CrsType,
    polygons: &[Vec<[f64; 2]>],
    limit: Option<u64>,
) -> Vec<TileCoord> {
    let max_tiles = limit.unwrap_or(500_000);
    let mut tiles = Vec::new();

    'outer: for zoom in min_zoom..=max_zoom {
        let ((x_min, x_max), (y_min, y_max)) = match crs {
            CrsType::Wgs84 => bounds_to_tile_range_wgs84(bounds, zoom),
            _ => bounds_to_tile_range_xyz(bounds, zoom),
        };
        for y in y_min..=y_max {
            for x in x_min..=x_max {
                if tiles.len() as u64 >= max_tiles {
                    break 'outer;
                }
                if tile_intersects_polygons(x, y, zoom, polygons, crs) {
                    tiles.push(TileCoord { z: zoom, x, y });
                }
            }
        }
    }
    tiles
}

// ─── 单元测试 ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multi_polygon_geometry_keeps_disconnected_regions() {
        let west = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]];
        let east = vec![[10.0, 10.0], [11.0, 10.0], [11.0, 11.0]];
        let polygons = vec![west, east];
        assert!(point_in_polygons(0.5, 0.2, &polygons));
        assert!(point_in_polygons(10.5, 10.2, &polygons));
        assert!(!point_in_polygons(5.0, 5.0, &polygons));
    }

    #[test]
    fn multi_polygon_enumeration_includes_each_disconnected_region() {
        let polygons = vec![
            vec![[-101.0, 39.0], [-99.0, 39.0], [-99.0, 41.0], [-101.0, 41.0]],
            vec![[99.0, 39.0], [101.0, 39.0], [101.0, 41.0], [99.0, 41.0]],
        ];
        let bounds = Bounds {
            west: -101.0,
            east: 101.0,
            south: 39.0,
            north: 41.0,
        };
        let tiles =
            enumerate_tiles_with_polygons(&bounds, 6, 6, &CrsType::WebMercator, &polygons, None);
        let west_tile = lng_lat_to_tile_xyz(-100.0, 40.0, 6);
        let east_tile = lng_lat_to_tile_xyz(100.0, 40.0, 6);
        assert!(tiles.iter().any(|tile| (tile.x, tile.y) == west_tile));
        assert!(tiles.iter().any(|tile| (tile.x, tile.y) == east_tile));
    }

    #[test]
    fn multi_polygon_tile_is_not_marked_inside_when_corners_hit_different_faces() {
        let tile = TileCoord { z: 1, x: 0, y: 0 };
        let tile_bounds = tile_to_lonlat_bounds(tile.x, tile.y, tile.z, &CrsType::WebMercator);
        let split_lng = (tile_bounds.west + tile_bounds.east) / 2.0;
        let polygons = vec![
            vec![
                [tile_bounds.west - 1.0, tile_bounds.south - 1.0],
                [split_lng, tile_bounds.south - 1.0],
                [split_lng, tile_bounds.north + 1.0],
                [tile_bounds.west - 1.0, tile_bounds.north + 1.0],
            ],
            vec![
                [split_lng, tile_bounds.south - 1.0],
                [tile_bounds.east + 1.0, tile_bounds.south - 1.0],
                [tile_bounds.east + 1.0, tile_bounds.north + 1.0],
                [split_lng, tile_bounds.north + 1.0],
            ],
        ];
        assert!(!tile_fully_within_polygons(
            tile.x,
            tile.y,
            tile.z,
            &polygons,
            &CrsType::WebMercator
        ));
    }

    #[test]
    fn concave_polygon_tile_with_an_internal_notch_requires_clipping() {
        let tile = TileCoord { z: 1, x: 0, y: 0 };
        let bounds = tile_to_lonlat_bounds(tile.x, tile.y, tile.z, &CrsType::WebMercator);
        let middle_lng = (bounds.west + bounds.east) / 2.0;
        let notch_bottom = (bounds.south + bounds.north) / 2.0;
        let polygon = vec![
            [bounds.west - 1.0, bounds.south - 1.0],
            [bounds.east + 1.0, bounds.south - 1.0],
            [bounds.east + 1.0, bounds.north + 1.0],
            [middle_lng + 10.0, bounds.north + 1.0],
            [middle_lng + 10.0, notch_bottom],
            [middle_lng - 10.0, notch_bottom],
            [middle_lng - 10.0, bounds.north + 1.0],
            [bounds.west - 1.0, bounds.north + 1.0],
        ];
        let polygons = vec![polygon];

        assert!(
            !tile_fully_within_polygons(tile.x, tile.y, tile.z, &polygons, &CrsType::WebMercator),
            "凹多边形缺口进入瓦片时必须进入像素裁剪流程"
        );
    }

    #[test]
    fn polygon_geometry_parser_supports_old_and_new_storage_formats() {
        let old = parse_polygon_geometry("[[0,0],[1,0],[0,1]]").expect("旧格式应兼容");
        assert_eq!(old.len(), 1);
        let new = parse_polygon_geometry("[[[0,0],[1,0],[0,1]],[[10,10],[11,10],[10,11]]]")
            .expect("新格式应解析");
        assert_eq!(new.len(), 2);
    }

    #[test]
    fn test_lng_lat_to_tile_xyz() {
        // 上海大约在 z=10: x=857, y=418
        let (x, y) = lng_lat_to_tile_xyz(121.4737, 31.2304, 10);
        assert_eq!(x, 857);
        assert_eq!(y, 418);
    }

    #[test]
    fn test_tile_xyz_to_bounds() {
        let b = tile_xyz_to_bounds(857, 418, 10);
        assert!((b.west - 121.2890625).abs() < 0.01);
        assert!((b.north - 31.353638).abs() < 0.01);
    }

    #[test]
    fn test_count_tiles_xyz() {
        let bounds = Bounds {
            west: 121.0,
            east: 122.0,
            south: 31.0,
            north: 32.0,
        };
        let result = count_tiles(&bounds, 8, 10, &CrsType::WebMercator);
        assert!(result.total > 0);
        assert_eq!(result.per_zoom.len(), 3);
        // z10 应该比 z8 的瓦片多得多
        let z8_count = result.per_zoom[0].count;
        let z10_count = result.per_zoom[2].count;
        assert!(z10_count > z8_count);
    }

    #[test]
    fn test_count_tiles_wgs84() {
        let bounds = Bounds {
            west: 121.0,
            east: 122.0,
            south: 31.0,
            north: 32.0,
        };
        let result = count_tiles(&bounds, 8, 10, &CrsType::Wgs84);
        assert!(result.total > 0);
    }

    #[test]
    fn test_enumerate_tiles_limit() {
        let bounds = Bounds {
            west: 73.0,
            east: 135.0,
            south: 18.0,
            north: 53.0,
        };
        // z18 全中国瓦片非常多，应被截断到 limit
        let tiles = enumerate_tiles(&bounds, 18, 18, &CrsType::WebMercator, Some(1000));
        assert_eq!(tiles.len(), 1000);
    }

    #[test]
    fn test_generate_grid_geojson() {
        let bounds = Bounds {
            west: 121.0,
            east: 122.0,
            south: 31.0,
            north: 32.0,
        };
        let (geojson, count) = generate_tile_grid_geojson(&bounds, 8, &CrsType::WebMercator, 100);
        assert!(!geojson.is_empty());
        assert!(count > 0);
        assert!(geojson.contains("FeatureCollection"));
    }
}
