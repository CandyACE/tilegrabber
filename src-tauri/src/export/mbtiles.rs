//! 将瓦片包导出为标准 MBTiles（SQLite）格式
//!
//! MBTiles 规范要求：
//! - tile_row 采用 TMS 规范（Y 轴翻转）：tile_row = 2^zoom - 1 - y
//! - metadata 表包含 name / description / version / bounds /
//!   center / minzoom / maxzoom / format 等必填字段

use anyhow::{Context, Result};
use rayon::prelude::*;
use rusqlite::{params, Connection};
use std::path::Path;

use crate::types::CrsType;

/// 从任务瓦片存储（`.tiles`）导出标准 MBTiles
///
/// # 参数
/// - `tile_store_path` — 源 `.tiles` SQLite 文件路径  
/// - `dest_path`       — 目标 `.mbtiles` 文件路径（由用户通过文件对话框指定）
/// - `task_name`       — 任务名称，写入 metadata.name  
/// - `bounds`          — `[west, south, east, north]` WGS84  
/// - `min_zoom`        — 最小缩放级别  
/// - `max_zoom`        — 最大缩放级别  
/// - `format`          — 图像格式：`"png"` / `"jpg"` / `"webp"`
/// - `clip_to_bounds`  — 是否仅导出完全位于范围内的瓦片（过滤边缘相交瓦片）
/// - `polygon`         — 多边形范围 WGS84 坐标（优先于矩形范围做像素级裁剪）
/// - `crs`             — 瓦片坐标系（WebMercator / WGS84）
/// - `progress_cb`     — 进度回调：`(已完成, 总数)`
pub fn export_mbtiles<F>(
    tile_store_path: &Path,
    dest_path: &Path,
    task_name: &str,
    bounds: [f64; 4], // [west, south, east, north]
    min_zoom: u8,
    max_zoom: u8,
    format: &str,
    clip_to_bounds: bool,
    polygon: Option<&[[f64; 2]]>,
    crs: &CrsType,
    mut progress_cb: F,
) -> Result<u64>
where
    F: FnMut(u64, u64),
{
    // ── 打开源数据库（只读）──────────────────────────────────────────────────
    let src = Connection::open_with_flags(
        tile_store_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .context("打开源瓦片存储失败")?;

    // ── 创建/覆盖目标 MBTiles ────────────────────────────────────────────────
    if dest_path.exists() {
        std::fs::remove_file(dest_path).context("无法删除已存在的目标文件")?;
    }
    let dst = Connection::open(dest_path).context("创建 MBTiles 文件失败")?;

    // 开启 WAL + page_size 优化，大批量写入前禁用 journaling
    dst.execute_batch(
        "PRAGMA journal_mode = OFF;
         PRAGMA synchronous   = OFF;
         PRAGMA page_size     = 4096;",
    )?;

    // ── 创建 MBTiles 表结构 ──────────────────────────────────────────────────
    dst.execute_batch(
        "CREATE TABLE IF NOT EXISTS metadata (
             name  TEXT NOT NULL,
             value TEXT NOT NULL
         );
         CREATE UNIQUE INDEX IF NOT EXISTS idx_metadata_name ON metadata(name);

         CREATE TABLE IF NOT EXISTS tiles (
             zoom_level  INTEGER NOT NULL,
             tile_column INTEGER NOT NULL,
             tile_row    INTEGER NOT NULL,
             tile_data   BLOB    NOT NULL
         );
         CREATE UNIQUE INDEX IF NOT EXISTS idx_tiles
             ON tiles(zoom_level, tile_column, tile_row);",
    )?;

    // ── 写入 metadata ────────────────────────────────────────────────────────
    let [west, south, east, north] = bounds;
    let center_lon = (west + east) / 2.0;
    let center_lat = (south + north) / 2.0;
    let center_zoom = (min_zoom + max_zoom) / 2;

    let meta_entries: &[(&str, String)] = &[
        ("name", task_name.to_string()),
        ("description", format!("Exported by TileGrabber from {task_name}")),
        ("version", "1.1".into()),
        (
            "bounds",
            format!("{west:.6},{south:.6},{east:.6},{north:.6}"),
        ),
        (
            "center",
            format!("{center_lon:.6},{center_lat:.6},{center_zoom}"),
        ),
        ("minzoom", min_zoom.to_string()),
        ("maxzoom", max_zoom.to_string()),
        ("format", format.to_string()),
        ("type", "baselayer".into()),
    ];

    {
        let tx = dst.unchecked_transaction()?;
        for (k, v) in meta_entries {
            tx.execute(
                "INSERT OR REPLACE INTO metadata(name, value) VALUES(?1, ?2)",
                params![k, v],
            )?;
        }
        tx.commit()?;
    }

    // ── 统计总瓦片数（用于进度回调）────────────────────────────────────────
    let total: u64 = src
        .query_row("SELECT COUNT(*) FROM tiles", [], |r| r.get::<_, i64>(0))
        .unwrap_or(0) as u64;

    if total == 0 {
        return Ok(0);
    }

    // ── 检查瓦片是否已被裁剪 ────────────────────────────────────────────────
    // 下载完成后已执行过精确裁剪的瓦片无需再次裁剪
    let already_clipped: bool = src
        .query_row(
            "SELECT value FROM metadata WHERE name='tiles.clipped'",
            [],
            |row| row.get::<_, String>(0),
        )
        .map(|v| v == "1")
        .unwrap_or(false);
    let need_clip = clip_to_bounds && !already_clipped;

    // ── 分批复制瓦片，进行 TMS y 轴翻转 ─────────────────────────────────────
    //   MBTiles 规定： tile_row = 2^zoom - 1 - y
    //   我们的 .tiles 存储的 tile_row 与 XYZ 一致（北往南递增），需转换
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

        let tx = dst.unchecked_transaction()?;
        // 并行裁剪（CPU 密集），再串行写入 SQLite
        let processed: Vec<Result<Option<(i64, i64, i64, std::borrow::Cow<[u8]>)>>> = batch
            .par_iter()
            .map(|(z, x, y, data)| {
                let write_data = if need_clip {
                    if let Some(poly) = polygon {
                        match crate::export::tile_clip::clip_tile_to_polygon_crs(
                            data, *x as u32, *y as u32, *z as u8, poly, crs,
                        )? {
                            None => return Ok(None),
                            Some(clipped) => std::borrow::Cow::Owned(clipped),
                        }
                    } else {
                        let [west, south, east, north] = bounds;
                        let task_bounds = crate::types::Bounds { west, east, south, north };
                        match crate::export::tile_clip::clip_tile_to_bounds_crs(
                            data, *x as u32, *y as u32, *z as u8, &task_bounds, crs,
                        )? {
                            None => return Ok(None),
                            Some(clipped) => std::borrow::Cow::Owned(clipped),
                        }
                    }
                } else {
                    std::borrow::Cow::Borrowed(data.as_slice())
                };
                let tms_y = (1i64 << z) - 1 - y;
                Ok(Some((*z, *x, tms_y, write_data)))
            })
            .collect();

        for item in processed {
            if let Some((z, x, tms_y, write_data)) = item? {
                tx.execute(
                    "INSERT OR REPLACE INTO tiles
                     (zoom_level, tile_column, tile_row, tile_data)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![z, x, tms_y, write_data.as_ref()],
                )?;
            }
        }
        tx.commit()?;

        offset += batch.len() as i64;
        written += batch.len() as u64;
        progress_cb(written, total);
    }

    Ok(written)
}
