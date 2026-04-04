//! TileGrabber — 瓦片计算 Tauri 命令

use crate::tile_math::{count_tiles, generate_tile_grid_geojson, TileCount, TileGrid};
use crate::types::{Bounds, CrsType};
use tauri::command;

/// 计算指定范围 + 层级范围内的瓦片总数量
///
/// 参数:
/// - `bounds`   — {west, east, south, north}
/// - `min_zoom` — 起始层级 (0-22)
/// - `max_zoom` — 结束层级 (0-22)
/// - `crs`      — 坐标系 "WEB_MERCATOR" | "WGS84" | 其他
#[command]
pub fn calculate_tile_count(
    bounds: Bounds,
    min_zoom: u8,
    max_zoom: u8,
    crs: Option<CrsType>,
) -> Result<TileCount, String> {
    if !bounds.is_valid() {
        return Err(format!(
            "无效的地理范围: west={}, east={}, south={}, north={}",
            bounds.west, bounds.east, bounds.south, bounds.north
        ));
    }
    if min_zoom > max_zoom {
        return Err(format!("min_zoom({}) > max_zoom({})", min_zoom, max_zoom));
    }
    if max_zoom > 22 {
        return Err(format!("max_zoom({}) 超出范围 [0, 22]", max_zoom));
    }

    let crs = crs.unwrap_or(CrsType::WebMercator);
    Ok(count_tiles(&bounds, min_zoom, max_zoom, &crs))
}

/// 生成指定层级的瓦片网格 GeoJSON（用于地图预览）
///
/// 当瓦片数 > 500 时，只返回范围框而不绘制每个格子（避免卡顿）
#[command]
pub fn generate_tile_grid(
    bounds: Bounds,
    zoom: u8,
    crs: Option<CrsType>,
) -> Result<TileGrid, String> {
    if !bounds.is_valid() {
        return Err(format!(
            "无效的地理范围: west={}, east={}, south={}, north={}",
            bounds.west, bounds.east, bounds.south, bounds.north
        ));
    }
    if zoom > 22 {
        return Err(format!("zoom({}) 超出范围 [0, 22]", zoom));
    }

    let crs = crs.unwrap_or(CrsType::WebMercator);
    let (geojson, tile_count) = generate_tile_grid_geojson(&bounds, zoom, &crs, 500);

    Ok(TileGrid { geojson, tile_count })
}
