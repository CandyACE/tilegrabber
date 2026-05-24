//! 将瓦片包导出为 PMTiles v3 单文件格式
//!
//! PMTiles（Protomaps 主导，BSD-3）是现代 Web 友好的单文件瓦片包：
//! - 单文件、HTTP Range 直读、CDN/S3 静态托管
//! - Hilbert 曲线编码 tile_id 提升空间局部性
//! - 自动去重 + run-length 编码
//!
//! 当前实现仅支持 WebMercator (EPSG:3857) 坐标系；WGS84 源会直接报错。

use anyhow::{anyhow, bail, Context, Result};
use pmtiles::{Compression, PmTilesWriter, TileCoord, TileId, TileType};
use rusqlite::{params, Connection};
use std::fs::File;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::types::{Bounds, CrsType};

/// 从任务瓦片存储（`.tiles`）导出 PMTiles v3 单文件。
///
/// # 参数
/// 与 [`crate::export::mbtiles::export_mbtiles`] 一致。
pub fn export_pmtiles<F>(
    tile_store_path: &Path,
    dest_path: &Path,
    task_name: &str,
    bounds: [f64; 4],
    min_zoom: u8,
    max_zoom: u8,
    format: &str,
    clip_to_bounds: bool,
    polygon: Option<&[[f64; 2]]>,
    crs: &CrsType,
    jpeg_quality: Option<u8>,
    png_level: Option<u8>,
    cancel: &AtomicBool,
    mut progress_cb: F,
) -> Result<u64>
where
    F: FnMut(u64, u64),
{
    if !matches!(crs, CrsType::WebMercator) {
        bail!("PMTiles 仅支持 WebMercator (EPSG:3857) 坐标系；当前任务为 WGS84，请改用 MBTiles 或目录导出");
    }

    let format_lc = format.to_ascii_lowercase();
    let is_vector = matches!(format_lc.as_str(), "pbf" | "mvt");

    // ── 打开源数据库（只读）──────────────────────────────────────────────────
    let src = Connection::open_with_flags(
        tile_store_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .context("打开源瓦片存储失败")?;

    let total: u64 = src
        .query_row("SELECT COUNT(*) FROM tiles", [], |r| r.get::<_, i64>(0))
        .unwrap_or(0) as u64;
    if total == 0 {
        return Ok(0);
    }

    let already_clipped: bool = src
        .query_row(
            "SELECT value FROM metadata WHERE name='tiles.clipped'",
            [],
            |row| row.get::<_, String>(0),
        )
        .map(|v| v == "1")
        .unwrap_or(false);
    let need_clip = clip_to_bounds && !already_clipped && !is_vector;

    // ── 几何 + 元数据 ────────────────────────────────────────────────────────
    let [west, south, east, north] = bounds;
    let center_lon = (west + east) / 2.0;
    let center_lat = (south + north) / 2.0;
    let center_zoom = (min_zoom + max_zoom) / 2;

    let tile_type = match format_lc.as_str() {
        "png" => TileType::Png,
        "jpg" | "jpeg" => TileType::Jpeg,
        "webp" => TileType::Webp,
        "pbf" | "mvt" => TileType::Mvt,
        _ => TileType::Unknown,
    };

    // 矢量瓦片：扫描首批样本提取 layer 名 -> vector_layers JSON 元数据
    let vector_layers_json: Option<serde_json::Value> = if is_vector {
        let mut layer_names = std::collections::BTreeSet::<String>::new();
        let mut sample_stmt = src.prepare(
            "SELECT tile_data FROM tiles ORDER BY zoom_level DESC LIMIT 16",
        )?;
        let rows = sample_stmt.query_map([], |r| r.get::<_, Vec<u8>>(0))?;
        for row in rows.flatten() {
            let decoded = crate::export::mvt_scan::maybe_gunzip(&row);
            for n in crate::export::mvt_scan::extract_layer_names(decoded.as_ref()) {
                layer_names.insert(n);
            }
            if layer_names.len() > 64 {
                break;
            }
        }
        let layers: Vec<serde_json::Value> = if layer_names.is_empty() {
            vec![serde_json::json!({
                "id": "default",
                "description": "Vector layer (auto-generated placeholder; original style spec unavailable)",
                "minzoom": min_zoom,
                "maxzoom": max_zoom,
                "fields": {}
            })]
        } else {
            layer_names
                .into_iter()
                .map(|id| {
                    serde_json::json!({
                        "id": id,
                        "description": "",
                        "minzoom": min_zoom,
                        "maxzoom": max_zoom,
                        "fields": {}
                    })
                })
                .collect()
        };
        Some(serde_json::Value::Array(layers))
    } else {
        None
    };

    let mut metadata_obj = serde_json::json!({
        "name": task_name,
        "description": format!("Exported by TileGrabber from {task_name}"),
        "version": "1.0.0",
        "format": if is_vector { "pbf" } else { format },
        "type": if is_vector { "overlay" } else { "baselayer" },
        "attribution": "TileGrabber",
    });
    if let Some(vl) = vector_layers_json {
        if let Some(obj) = metadata_obj.as_object_mut() {
            obj.insert("vector_layers".into(), vl);
        }
    }
    let metadata_json = metadata_obj.to_string();

    // ── 创建/覆盖目标 .pmtiles ───────────────────────────────────────────────
    if dest_path.exists() {
        std::fs::remove_file(dest_path).context("无法删除已存在的目标文件")?;
    }
    let out_file = File::create(dest_path).context("创建 PMTiles 文件失败")?;

    // 栅格瓦片本身已压缩（PNG/JPEG/WebP），tile_compression 必须设为 None；
    // 矢量瓦片（MVT/PBF）按 PMTiles 规范必须 gzip — 由 writer 自动包装
    let tile_compression = if is_vector {
        Compression::Gzip
    } else {
        Compression::None
    };

    let mut writer = PmTilesWriter::new(tile_type)
        .min_zoom(min_zoom)
        .max_zoom(max_zoom)
        .bounds(west, south, east, north)
        .center(center_lon, center_lat)
        .center_zoom(center_zoom)
        .tile_compression(tile_compression)
        .metadata(&metadata_json)
        .create(out_file)
        .map_err(|e| anyhow!("初始化 PMTiles writer 失败: {e}"))?;

    let task_bounds = Bounds {
        west,
        east,
        south,
        north,
    };

    // ── 按 zoom 升序处理；同 z 内按 tile_id (Hilbert 曲线) 升序写入 ──────────
    //
    //   PmTilesWriter 要求 tile_id 单调递增才能启用去重 + run-length 优化；
    //   不同 zoom 之间 tile_id 自然递增（高 z 对应更大 tile_id），无需担心；
    //   同 z 内 (x,y) 字典序与 Hilbert 序不同，必须显式排序。
    let mut written: u64 = 0;

    for z in min_zoom..=max_zoom {
        if cancel.load(Ordering::Relaxed) {
            bail!("__cancelled__");
        }

        // 阶段 1：拉取本层所有 (x, y) 并算 tile_id 排序
        let mut entries: Vec<(u32, u32, u64)> = Vec::new();
        {
            let mut stmt = src.prepare(
                "SELECT tile_column, tile_row FROM tiles WHERE zoom_level = ?1",
            )?;
            let rows = stmt.query_map(params![z as i64], |row| {
                Ok((row.get::<_, i64>(0)? as u32, row.get::<_, i64>(1)? as u32))
            })?;
            for r in rows {
                if let Ok((x, y)) = r {
                    if let Ok(coord) = TileCoord::new(z, x, y) {
                        let tid: TileId = coord.into();
                        entries.push((x, y, tid.value()));
                    }
                }
            }
        }
        entries.sort_by_key(|&(_, _, tid)| tid);

        if entries.is_empty() {
            continue;
        }

        // 阶段 2：按 tile_id 升序，逐瓦片读取 BLOB → 裁剪/重编码 → 写入
        let mut sel = src.prepare(
            "SELECT tile_data FROM tiles
             WHERE zoom_level = ?1 AND tile_column = ?2 AND tile_row = ?3",
        )?;

        for (x, y, _tid) in entries {
            if cancel.load(Ordering::Relaxed) {
                bail!("__cancelled__");
            }

            let data: Vec<u8> = match sel.query_row(
                params![z as i64, x as i64, y as i64],
                |r| r.get::<_, Vec<u8>>(0),
            ) {
                Ok(d) => d,
                Err(_) => continue,
            };

            // 矢量瓦片：跳过 clip/reencode，确保数据是 raw（未 gzip）protobuf
            // —— PMTiles writer 配置了 Compression::Gzip 会自动包装；若 blob 已 gzip 需先解开
            if is_vector {
                let raw = crate::export::mvt_scan::maybe_gunzip(&data);
                let coord = match TileCoord::new(z, x, y) {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                writer
                    .add_tile(coord, raw.as_ref())
                    .map_err(|e| anyhow!("写入矢量瓦片失败 (z={z}, x={x}, y={y}): {e}"))?;
                written += 1;
                if written % 64 == 0 {
                    progress_cb(written, total);
                }
                continue;
            }

            // 裁剪：多边形优先于矩形
            let write_data: std::borrow::Cow<[u8]> = if need_clip {
                let clipped_opt = if let Some(poly) = polygon {
                    crate::export::tile_clip::clip_tile_to_polygon_crs(
                        &data, x, y, z, poly, crs,
                    )?
                } else {
                    crate::export::tile_clip::clip_tile_to_bounds_crs(
                        &data,
                        x,
                        y,
                        z,
                        &task_bounds,
                        crs,
                    )?
                };
                match clipped_opt {
                    None => continue,
                    Some(c) => std::borrow::Cow::Owned(c),
                }
            } else {
                std::borrow::Cow::Borrowed(data.as_slice())
            };

            // 重编码（可选）
            let final_data: std::borrow::Cow<[u8]> =
                if jpeg_quality.is_some() || png_level.is_some() {
                    crate::export::try_reencode_tile(
                        write_data.as_ref(),
                        jpeg_quality,
                        png_level,
                    )
                    .into_owned()
                    .into()
                } else {
                    write_data
                };

            let coord = match TileCoord::new(z, x, y) {
                Ok(c) => c,
                Err(_) => continue,
            };
            writer
                .add_tile(coord, final_data.as_ref())
                .map_err(|e| anyhow!("写入瓦片失败 (z={z}, x={x}, y={y}): {e}"))?;

            written += 1;
            if written % 64 == 0 {
                progress_cb(written, total);
            }
        }
    }

    writer
        .finalize()
        .map_err(|e| anyhow!("PMTiles 文件 finalize 失败: {e}"))?;
    progress_cb(written, total);
    Ok(written)
}
