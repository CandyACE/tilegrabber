//! 将瓦片包导出为 z/x/y.{ext} 文件目录
//!
//! 目录结构示例：
//! ```
//! output/
//!   14/
//!     10882/
//!       6556.png
//!       6557.png
//!   15/
//!     ...
//! ```
//!
//! 文件名采用 XYZ 编号（与 URL 模板一致），不做 TMS 翻转。

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OpenFlags};
use std::path::Path;

use crate::types::CrsType;

/// 从任务瓦片存储（`.tiles`）导出目录格式
///
/// # 参数
/// - `tile_store_path` — 源 `.tiles` SQLite 文件路径  
/// - `dest_dir`        — 目标根目录（将在其中创建 z/x/y.ext 文件树）
/// - `format`          — 图像格式：`"png"` / `"jpg"` / `"webp"`（决定扩展名）
/// - `clip_to_bounds`  — 是否仅导出完全位于范围内的瓦片（过滤边缘相交瓦片）
/// - `bounds`          — `[west, south, east, north]`（clip_to_bounds=true 时有效）
/// - `polygon`         — 多边形范围 WGS84 坐标（优先于矩形范围做像素级裁剪）
/// - `crs`             — 瓦片坐标系（WebMercator / WGS84）
/// - `progress_cb`     — 进度回调：`(已完成, 总数)`
pub fn export_directory<F>(
    tile_store_path: &Path,
    dest_dir: &Path,
    format: &str,
    clip_to_bounds: bool,
    bounds: [f64; 4],
    polygon: Option<&[[f64; 2]]>,
    crs: &CrsType,
    mut progress_cb: F,
) -> Result<u64>
where
    F: FnMut(u64, u64),
{
    // ── 打开源数据库（只读）──────────────────────────────────────────────────
    let src = Connection::open_with_flags(tile_store_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .context("打开源瓦片存储失败")?;

    // ── 统计总瓦片数 ─────────────────────────────────────────────────────────
    let total: u64 = src
        .query_row("SELECT COUNT(*) FROM tiles", [], |r| r.get::<_, i64>(0))
        .unwrap_or(0) as u64;

    if total == 0 {
        return Ok(0);
    }

    // 确保目标目录存在
    std::fs::create_dir_all(dest_dir).context("无法创建目标目录")?;

    // ── 检查瓦片是否已被裁剪 ────────────────────────────────────────────────
    let already_clipped: bool = src
        .query_row(
            "SELECT value FROM metadata WHERE name='tiles.clipped'",
            [],
            |row| row.get::<_, String>(0),
        )
        .map(|v| v == "1")
        .unwrap_or(false);
    let need_clip = clip_to_bounds && !already_clipped;

    // 文件扩展名（jpg 统一映射为 jpg）
    let ext = match format.to_lowercase().as_str() {
        "jpg" | "jpeg" => "jpg",
        "webp" => "webp",
        _ => "png",
    };

    // ── 分批读取并写入文件 ───────────────────────────────────────────────────
    const BATCH: i64 = 500;
    let mut offset: i64 = 0;
    let mut written: u64 = 0;

    loop {
        let batch: Vec<(i64, i64, i64, Vec<u8>)> = {
            let mut stmt = src.prepare(
                "SELECT zoom_level, tile_column, tile_row, tile_data
                 FROM tiles
                 ORDER BY zoom_level, tile_column, tile_row
                 LIMIT ?1 OFFSET ?2",
            )?;
            let rows = stmt.query_map(params![BATCH, offset], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Vec<u8>>(3)?,
                ))
            })?;
            rows.collect::<std::result::Result<_, _>>()?
        };

        if batch.is_empty() {
            break;
        }

        use rayon::prelude::*;
        let errors: Vec<anyhow::Error> = batch.par_iter().filter_map(|(z, x, y, data)| {
            // 范围裁剪：完全在范围外的跳过；边缘相交的保留区域内像素，外部透明
            let write_data = if need_clip {
                if let Some(poly) = polygon {
                    match crate::export::tile_clip::clip_tile_to_polygon_crs(
                        data, *x as u32, *y as u32, *z as u8, poly, crs,
                    ) {
                        Ok(None) => return None, // 无交集
                        Ok(Some(clipped)) => std::borrow::Cow::Owned(clipped),
                        Err(e) => return Some(e),
                    }
                } else {
                    let [west, south, east, north] = bounds;
                    let task_bounds = crate::types::Bounds { west, east, south, north };
                    match crate::export::tile_clip::clip_tile_to_bounds_crs(
                        data, *x as u32, *y as u32, *z as u8, &task_bounds, crs,
                    ) {
                        Ok(None) => return None,
                        Ok(Some(clipped)) => std::borrow::Cow::Owned(clipped),
                        Err(e) => return Some(e),
                    }
                }
            } else {
                std::borrow::Cow::Borrowed(data.as_slice())
            };
            let actual_ext = if need_clip && write_data.as_ref().starts_with(b"\x89PNG") {
                "png"
            } else {
                ext
            };
            let tile_dir = dest_dir.join(z.to_string()).join(x.to_string());
            if let Err(e) = std::fs::create_dir_all(&tile_dir) {
                return Some(anyhow::anyhow!("无法创建目录 {}: {e}", tile_dir.display()));
            }
            let file_path = tile_dir.join(format!("{y}.{actual_ext}"));
            if let Err(e) = std::fs::write(&file_path, write_data.as_ref()) {
                return Some(anyhow::anyhow!("写入瓦片文件失败: {}: {e}", file_path.display()));
            }
            None
        }).collect();
        if !errors.is_empty() {
            return Err(anyhow::anyhow!("批量导出时发生错误: {:?}", errors));
        }

        offset += batch.len() as i64;
        written += batch.len() as u64;
        progress_cb(written, total);
    }

    Ok(written)
}
