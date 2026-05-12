//! 将瓦片包导出为 GeoTIFF 格式（分条带流式写入，低内存占用）
//!
//! 将指定层级的所有瓦片拼接并写入带地理参考的 TIFF 文件。
//! 采用逐条带（strip）写入：每次仅将一行瓦片（256 px 高度带）加载进内存，
//! 峰值内存开销为 `宽度（像素）× 256 × 4 字节`，与图像高度无关。
//!
//! # 注意
//! - 支持 WebMercator（EPSG:3857）与 WGS84（EPSG:4326）两种瓦片网格
//! - GeoTIFF 地理参考按源瓦片 CRS 写入正确的坐标范围与 EPSG

use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};

use anyhow::{bail, Context, Result};
use image::RgbaImage;
use rayon::prelude::*;
use rusqlite::{Connection, OpenFlags};
use tiff::encoder::colortype::RGBA8;
use tiff::encoder::compression::{Deflate, DeflateLevel, Lzw};
use tiff::encoder::TiffEncoder;
use tiff::tags::Tag;

use crate::export::utm as utm_proj;
use crate::types::CrsType;

// ─── Mercator Y 投影辅助 ────────────────────────────────────────────────────

/// WebMercator 正投影：纬度 → Mercator Y
#[inline(always)]
fn merc(lat: f64) -> f64 {
    use std::f64::consts::PI;
    (PI / 4.0 + lat.to_radians() / 2.0).tan().ln()
}

/// WebMercator 反投影：Mercator Y → 纬度
#[inline(always)]
fn merc_inv(y: f64) -> f64 {
    use std::f64::consts::PI;
    (2.0 * y.exp().atan() - PI / 2.0).to_degrees()
}

/// WGS84 经纬度 → WebMercator 米坐标
#[inline(always)]
fn lng_lat_to_web_mercator(lng: f64, lat: f64) -> (f64, f64) {
    const ORIGIN_SHIFT: f64 = 20_037_508.342_789_244;
    let lat = lat.clamp(-85.051_128_779_806_6, 85.051_128_779_806_6);
    let x = lng * ORIGIN_SHIFT / 180.0;
    let y = merc(lat) * ORIGIN_SHIFT / std::f64::consts::PI;
    (x, y)
}

// ─── 目标坐标系 ──────────────────────────────────────────────────────────────

/// 导出目标坐标系（内部使用）
#[derive(Debug, Clone)]
enum TargetCrs {
    /// 地理坐标系（经纬度），像素网格与 WGS84 相同，仅写入指定 EPSG 代号
    Geographic { epsg: u32 },
    /// EPSG:3857 Web Mercator
    WebMercator,
    /// UTM 投影（通用横轴墨卡托）
    Utm { zone: u8, north: bool, epsg: u32 },
}

impl TargetCrs {
    /// 根据 EPSG 代号构造 TargetCrs
    fn from_epsg(epsg: u32) -> Self {
        match epsg {
            3857 => TargetCrs::WebMercator,
            // WGS84 UTM 北半球 Zone 1-60
            e @ 32601..=32660 => TargetCrs::Utm { zone: (e - 32600) as u8, north: true, epsg: e },
            // WGS84 UTM 南半球 Zone 1-60
            e @ 32701..=32760 => TargetCrs::Utm { zone: (e - 32700) as u8, north: false, epsg: e },
            // 其他地理坐标系（4326/4490/4214/4610 等）均按 WGS84 像素网格处理
            e => TargetCrs::Geographic { epsg: e },
        }
    }

    /// 对应的像素网格 CrsType（用于行重映射判断）
    #[allow(dead_code)]
    fn pixel_crs(&self) -> &'static str {
        match self {
            TargetCrs::WebMercator => "merc",
            TargetCrs::Geographic { .. } | TargetCrs::Utm { .. } => "geo",
        }
    }
}

#[derive(Debug, Clone)]
struct GeoReference {
    model_west: f64,
    model_north: f64,
    pixel_scale_x: f64,
    pixel_scale_y: f64,
    geo_keys: Vec<u16>,
}

fn build_geo_reference(
    target: &TargetCrs,
    geo_west: f64,
    geo_north: f64,
    geo_east: f64,
    geo_south: f64,
    out_w: u32,
    out_h: u32,
) -> GeoReference {
    match target {
        TargetCrs::Geographic { epsg } => GeoReference {
            model_west: geo_west,
            model_north: geo_north,
            pixel_scale_x: (geo_east - geo_west) / out_w as f64,
            pixel_scale_y: (geo_north - geo_south) / out_h as f64,
            // GTModelTypeGeoKey=2(Geographic), GTRasterTypeGeoKey=1, GeographicTypeGeoKey=epsg
            geo_keys: vec![1, 1, 0, 3, 1024, 0, 1, 2, 1025, 0, 1, 1, 2048, 0, 1, *epsg as u16],
        },
        TargetCrs::WebMercator => {
            let (model_west, model_north) = lng_lat_to_web_mercator(geo_west, geo_north);
            let (model_east, model_south) = lng_lat_to_web_mercator(geo_east, geo_south);
            GeoReference {
                model_west,
                model_north,
                pixel_scale_x: (model_east - model_west) / out_w as f64,
                pixel_scale_y: (model_north - model_south) / out_h as f64,
                geo_keys: vec![1, 1, 0, 3, 1024, 0, 1, 1, 1025, 0, 1, 1, 3072, 0, 1, 3857],
            }
        }
        TargetCrs::Utm { zone, north, epsg } => {
            let (east_min, north_max, east_max, north_min) =
                utm_proj::utm_bbox_from_wgs84(geo_west, geo_south, geo_east, geo_north, *zone, *north);
            GeoReference {
                model_west: east_min,
                model_north: north_max,
                pixel_scale_x: (east_max - east_min) / out_w as f64,
                pixel_scale_y: (north_max - north_min) / out_h as f64,
                // GTModelTypeGeoKey=1(Projected), GTRasterTypeGeoKey=1, ProjectedCSTypeGeoKey=epsg
                geo_keys: vec![1, 1, 0, 3, 1024, 0, 1, 1, 1025, 0, 1, 1, 3072, 0, 1, *epsg as u16],
            }
        }
    }
}

// ─── 常量 ────────────────────────────────────────────────────────────────────

/// 单张瓦片像素尺寸
const TILE_SIZE: u32 = 256;

// ─── GeoTIFF Tag IDs ─────────────────────────────────────────────────────────

/// ModelPixelScaleTag — 像素地理尺寸 [sx, sy, sz]
const TAG_MODEL_PIXEL_SCALE: u16 = 33550;
/// ModelTiepointTag — 参考控制点 [i,j,k, x,y,z]
const TAG_MODEL_TIEPOINT: u16 = 33922;
/// GeoKeyDirectoryTag — GeoKey 字典
const TAG_GEO_KEY_DIRECTORY: u16 = 34735;

// ─── 公开接口 ────────────────────────────────────────────────────────────────

/// 从任务瓦片存储（`.tiles`）导出 GeoTIFF
///
/// # 参数
/// - `tile_store_path` — 源 `.tiles` SQLite 文件路径
/// - `dest_path`       — 目标 `.tif` 文件路径
/// - `bounds`          — `[west, south, east, north]` WGS84 任务范围
/// - `zoom`            — 导出层级（使用该层级的瓦片拼接）
/// - `clip_to_bounds`  — 若为 true，将输出图像裁剪至精确地理范围
/// - `polygon`         — 可选多边形顶点 `[经度, 纬度]`，为 Some 时在矩形裁剪基础上
///                       按多边形形状将范围外像素设为透明（奇偶填充规则）
/// - `crs`             — 瓦片坐标系（WebMercator 或 WGS84）
/// - `target_epsg`     — 输出 GeoTIFF 的 EPSG 代号；`None` 表示跟随源图层；
///                       支持 4326/4490 等地理坐标系、3857 Web Mercator、
///                       32601–32760 UTM 带（真正的像素重投影）
/// - `compression`     — 压缩方式："none"（无压缩）、"lzw"、"deflate"
/// - `progress_cb`     — 进度回调 `(done_tile_rows, total_tile_rows)`
pub fn export_geotiff<F: Fn(u64, u64)>(
    tile_store_path: &Path,
    dest_path: &Path,
    bounds: [f64; 4],
    zoom: u8,
    clip_to_bounds: bool,
    polygon: Option<Vec<[f64; 2]>>,
    crs: &CrsType,
    target_epsg: Option<u32>,
    compression: &str,
    cancel: &AtomicBool,
    progress_cb: F,
) -> Result<u64> {
    let [west, south, east, north] = bounds;

    // ── 打开源数据库 ─────────────────────────────────────────────────────────
    let src = Connection::open_with_flags(tile_store_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .context("打开源瓦片存储失败")?;

    // ── 查询目标层级的瓦片坐标范围 ──────────────────────────────────────────
    let (x_min, x_max, y_min, y_max): (u32, u32, u32, u32) = src
        .query_row(
            "SELECT MIN(tile_column), MAX(tile_column), MIN(tile_row), MAX(tile_row)
             FROM tiles WHERE zoom_level = ?1",
            rusqlite::params![zoom as i64],
            |row| {
                Ok((
                    row.get::<_, i64>(0)? as u32,
                    row.get::<_, i64>(1)? as u32,
                    row.get::<_, i64>(2)? as u32,
                    row.get::<_, i64>(3)? as u32,
                ))
            },
        )
        .context("该层级无瓦片数据")?;

    let cols = x_max - x_min + 1;
    let rows = y_max - y_min + 1;

    // ── 计算拼接画布的地理范围 ───────────────────────────────────────────────
    let top_left = crate::tile_math::tile_to_lonlat_bounds(x_min, y_min, zoom, crs);
    let bottom_right = crate::tile_math::tile_to_lonlat_bounds(x_max, y_max, zoom, crs);
    let img_west = top_left.west;
    let img_north = top_left.north;
    let img_east = bottom_right.east;
    let img_south = bottom_right.south;

    // ── 计算输出像素窗口（考虑 clip_to_bounds） ─────────────────────────────
    // out_x0/out_y0: 输出窗口左上角在完整画布中的像素坐标
    // merc_n_out / merc_per_out_px: polygon mask 需要用 Mercator Y 反投影算行纬度（WGS84 时为 0.0）
    let (
        out_x0,
        out_y0,
        out_w,
        out_h,
        geo_west,
        geo_north,
        geo_east,
        geo_south,
        merc_n_out,
        merc_per_out_px,
    ) = {
        let canvas_w = cols * TILE_SIZE;
        let canvas_h = rows * TILE_SIZE;
        if clip_to_bounds {
            let lon_per_px = (img_east - img_west) / canvas_w as f64;

            let (py0, py1, gn, gs) = match crs {
                CrsType::Wgs84 => {
                    // WGS84：纬度与像素行线性对应
                    let lat_per_px = (img_north - img_south) / canvas_h as f64;
                    let py0 = ((img_north - north.min(img_north)) / lat_per_px).ceil() as u32;
                    let py1 = ((img_north - south.max(img_south)) / lat_per_px).floor() as u32;
                    let gn = img_north - py0 as f64 * lat_per_px;
                    let gs = img_north - py1 as f64 * lat_per_px;
                    (py0, py1, gn, gs)
                }
                _ => {
                    // WebMercator：使用 Mercator Y 反投影求像素行，避免纬度线性近似误差
                    let mn = merc(img_north);
                    let ms = merc(img_south);
                    let mpp = (mn - ms) / canvas_h as f64;
                    let py0 = ((mn - merc(north.min(img_north))) / mpp).ceil() as u32;
                    let py1 = ((mn - merc(south.max(img_south))) / mpp).floor() as u32;
                    let gn = merc_inv(mn - py0 as f64 * mpp);
                    let gs = merc_inv(mn - py1 as f64 * mpp);
                    (py0, py1, gn, gs)
                }
            };

            let px0 = ((west.max(img_west) - img_west) / lon_per_px).ceil() as u32;
            let px1 = ((east.min(img_east) - img_west) / lon_per_px).floor() as u32;
            let w = px1.saturating_sub(px0).max(1);
            let h = py1.saturating_sub(py0).max(1);
            let gw = img_west + px0 as f64 * lon_per_px;
            let ge = img_west + px1 as f64 * lon_per_px;

            // 为 polygon mask 存储 Mercator 参数（WGS84 时置 0.0 占位）
            let (mn_out, mpp_out) = match crs {
                CrsType::Wgs84 => (0.0f64, 0.0f64),
                _ => {
                    let mn = merc(gn);
                    let ms = merc(gs);
                    (mn, (mn - ms) / h as f64)
                }
            };
            (px0, py0, w, h, gw, gn, ge, gs, mn_out, mpp_out)
        } else {
            // clip_to_bounds=false：输出覆盖全部瓦片范围
            let (mn_out, mpp_out) = match crs {
                CrsType::Wgs84 => (0.0f64, 0.0f64),
                _ => {
                    let mn = merc(img_north);
                    let ms = merc(img_south);
                    let h_f = (rows * TILE_SIZE) as f64;
                    (mn, (mn - ms) / h_f)
                }
            };
            (
                0, 0, canvas_w, canvas_h, img_west, img_north, img_east, img_south, mn_out, mpp_out,
            )
        }
    };

    // ── 创建 TIFF 编码器并写入地理参考标签 ──────────────────────────────────
    let file = std::fs::File::create(dest_path).context("创建 GeoTIFF 文件失败")?;
    let mut encoder = TiffEncoder::new_big(std::io::BufWriter::new(file))?;
    let lon_scale = (geo_east - geo_west) / out_w as f64;
    let lat_scale = (geo_north - geo_south) / out_h as f64;

    // 确定目标坐标系（TargetCrs）
    let target = if let Some(epsg) = target_epsg {
        TargetCrs::from_epsg(epsg)
    } else {
        // auto：与源图层相同
        match crs {
            CrsType::WebMercator => TargetCrs::WebMercator,
            _ => TargetCrs::Geographic { epsg: 4326 },
        }
    };
    let geo_ref = build_geo_reference(&target, geo_west, geo_north, geo_east, geo_south, out_w, out_h);

    // 确定重投影模式
    let src_is_merc = matches!(crs, CrsType::WebMercator);
    let reproject_mode: u8 = if matches!(crs, CrsType::Terrain | CrsType::Unknown) {
        0 // 无效源：不重投影
    } else {
        match &target {
            TargetCrs::Utm { .. } => 2, // 2D UTM 重投影
            TargetCrs::Geographic { .. } if src_is_merc => 1, // 1D 行重映射
            TargetCrs::WebMercator if !src_is_merc => 1,      // 1D 行重映射（反向）
            _ => 0, // 同格网，无需像素重投影
        }
    };

    if reproject_mode >= 1 {
        // ── 重投影路径：先将所有瓦片加载到内存画布，再逐行重映射 ────────────
        let (src_canvas, read_count) = load_source_canvas(
            &src,
            zoom,
            x_min,
            y_min,
            cols,
            rows,
            out_x0,
            out_y0,
            out_w,
            out_h,
            &progress_cb,
        )?;

        // UTM 参数（仅 reproject_mode==2 时使用）
        let utm_params: Option<(u8, bool, f64, f64, f64, f64)> = if reproject_mode == 2 {
            if let TargetCrs::Utm { zone, north, .. } = &target {
                let (east_min, north_max, east_max, north_min) =
                    utm_proj::utm_bbox_from_wgs84(geo_west, geo_south, geo_east, geo_north, *zone, *north);
                Some((*zone, *north, east_min, north_max, east_max, north_min))
            } else {
                None
            }
        } else {
            None
        };

        macro_rules! encode_reprojected {
            ($image_enc:expr) => {{
                let mut image_enc = $image_enc;
                image_enc.rows_per_strip(TILE_SIZE)?;
                image_enc.encoder().write_tag(
                    Tag::Unknown(TAG_MODEL_PIXEL_SCALE),
                    [geo_ref.pixel_scale_x, geo_ref.pixel_scale_y, 0.0_f64].as_slice(),
                )?;
                image_enc.encoder().write_tag(
                    Tag::Unknown(TAG_MODEL_TIEPOINT),
                    [0.0_f64, 0.0, 0.0, geo_ref.model_west, geo_ref.model_north, 0.0].as_slice(),
                )?;
                image_enc.encoder().write_tag(Tag::ExtraSamples, [2u16].as_slice())?;
                image_enc.encoder().write_tag(
                    Tag::Unknown(TAG_GEO_KEY_DIRECTORY),
                    geo_ref.geo_keys.as_slice(),
                )?;

                let mut abs_row = 0u32;
                while abs_row < out_h {
                    if cancel.load(AtomicOrdering::Relaxed) {
                        anyhow::bail!("__cancelled__");
                    }
                    let strip_rows = (out_h - abs_row).min(TILE_SIZE);
                    let strip_buf = if let Some((zone, north, east_min, north_max, east_max, north_min)) = utm_params {
                        render_reprojected_strip_utm(
                            &src_canvas,
                            abs_row,
                            strip_rows,
                            out_h,
                            out_w,
                            geo_west,
                            geo_north,
                            geo_south,
                            geo_east,
                            east_min,
                            north_max,
                            east_max,
                            north_min,
                            zone,
                            north,
                            crs,
                            polygon.as_deref(),
                        )
                    } else {
                        render_reprojected_strip(
                            &src_canvas,
                            abs_row,
                            strip_rows,
                            out_h,
                            out_w,
                            geo_west,
                            geo_north,
                            geo_south,
                            geo_east,
                            crs,
                            &target,
                            polygon.as_deref(),
                        )
                    };
                    image_enc.write_strip(&strip_buf)?;
                    abs_row += strip_rows;
                }
                image_enc.finish()?;
                read_count
            }};
        }

        let result = match compression {
            "lzw" => encode_reprojected!(encoder.new_image_with_compression::<RGBA8, _>(
                out_w,
                out_h,
                Lzw::default()
            )?),
            "deflate" => encode_reprojected!(encoder.new_image_with_compression::<RGBA8, _>(
                out_w,
                out_h,
                Deflate::with_level(DeflateLevel::Best)
            )?),
            _ => encode_reprojected!(encoder.new_image::<RGBA8>(out_w, out_h)?),
        };
        return Ok(result);
    }

    // ── 无需重投影路径：逐条带读取瓦片行并写入 ─────────────────────────────
    macro_rules! encode_with {
        ($image_enc:expr) => {{
            let mut image_enc = $image_enc;
            image_enc.rows_per_strip(TILE_SIZE)?;
            image_enc.encoder().write_tag(
                Tag::Unknown(TAG_MODEL_PIXEL_SCALE),
                [geo_ref.pixel_scale_x, geo_ref.pixel_scale_y, 0.0_f64].as_slice(),
            )?;
            image_enc.encoder().write_tag(
                Tag::Unknown(TAG_MODEL_TIEPOINT),
                [0.0_f64, 0.0, 0.0, geo_ref.model_west, geo_ref.model_north, 0.0].as_slice(),
            )?;
            image_enc.encoder().write_tag(Tag::ExtraSamples, [2u16].as_slice())?;
            image_enc.encoder().write_tag(
                Tag::Unknown(TAG_GEO_KEY_DIRECTORY),
                geo_ref.geo_keys.as_slice(),
            )?;
            let read_count = render_geotiff_strips(
                &src,
                zoom,
                x_min,
                y_min,
                cols,
                rows,
                out_x0,
                out_y0,
                out_w,
                out_h,
                polygon.as_deref(),
                merc_n_out,
                merc_per_out_px,
                geo_west,
                geo_north,
                lon_scale,
                lat_scale,
                cancel,
                progress_cb,
                |strip_buf| image_enc.write_strip(&strip_buf).map_err(Into::into),
            )?;
            image_enc.finish()?;
            read_count
        }};
    }

    let read_count = match compression {
        "lzw" => encode_with!(encoder.new_image_with_compression::<RGBA8, _>(
            out_w,
            out_h,
            Lzw::default()
        )?),
        "deflate" => encode_with!(encoder.new_image_with_compression::<RGBA8, _>(
            out_w,
            out_h,
            Deflate::with_level(DeflateLevel::Best)
        )?),
        _ => encode_with!(encoder.new_image::<RGBA8>(out_w, out_h)?),
    };

    Ok(read_count)
}

/// 计算 RgbaImage 中指定行的字节偏移（width × row × 4）
#[inline(always)]
fn tire_row_byte_offset(width: u32, row: u32) -> usize {
    row as usize * width as usize * 4
}

#[allow(clippy::too_many_arguments)]
fn render_geotiff_strips<FProgress, FConsume>(
    src: &Connection,
    zoom: u8,
    x_min: u32,
    y_min: u32,
    cols: u32,
    rows: u32,
    out_x0: u32,
    out_y0: u32,
    out_w: u32,
    out_h: u32,
    polygon: Option<&[[f64; 2]]>,
    merc_n_out: f64,
    merc_per_out_px: f64,
    geo_west: f64,
    geo_north: f64,
    lon_scale: f64,
    lat_scale: f64,
    cancel: &AtomicBool,
    progress_cb: FProgress,
    mut consume_strip: FConsume,
) -> Result<u64>
where
    FProgress: Fn(u64, u64),
    FConsume: FnMut(Vec<u8>) -> Result<()>,
{
    let mut stmt =
        src.prepare("SELECT tile_column, tile_data FROM tiles WHERE zoom_level = ?1 AND tile_row = ?2")?;
    let mut abs_out_row: u32 = 0;
    let mut cached_tile_y: Option<u32> = None;
    let mut tile_row_cache: HashMap<u32, RgbaImage> = HashMap::new();
    let mut read_count: u64 = 0;
    let total_ty_rows = rows as u64;

    while abs_out_row < out_h {
        if cancel.load(AtomicOrdering::Relaxed) {
            anyhow::bail!("__cancelled__");
        }
        let strip_rows = (out_h - abs_out_row).min(TILE_SIZE);
        let mut strip_buf = vec![0u8; strip_rows as usize * out_w as usize * 4];

        for row_in_strip in 0..strip_rows {
            let global_px_row = out_y0 + abs_out_row + row_in_strip;
            let tile_row_idx = global_px_row / TILE_SIZE;
            let tile_inner_y = global_px_row % TILE_SIZE;
            let actual_tile_y = y_min + tile_row_idx;

            if cached_tile_y != Some(actual_tile_y) {
                tile_row_cache.clear();
                let loaded: Vec<(u32, Vec<u8>)> = stmt
                    .query_map(rusqlite::params![zoom as i64, actual_tile_y as i64], |r| {
                        Ok((r.get::<_, i64>(0)? as u32, r.get::<_, Vec<u8>>(1)?))
                    })?
                    .filter_map(|r| r.ok())
                    .collect();
                let decoded: Vec<(u32, Option<RgbaImage>)> = loaded
                    .into_par_iter()
                    .map(|(tx, data)| {
                        let img = image::load_from_memory(&data).map(|i| i.to_rgba8()).ok();
                        (tx, img)
                    })
                    .collect();
                let mut count = 0u64;
                for (tx, img_opt) in decoded {
                    if let Some(img) = img_opt {
                        tile_row_cache.insert(tx, img);
                        count += 1;
                    }
                }
                read_count += count;
                cached_tile_y = Some(actual_tile_y);
                let done_rows = (actual_tile_y - y_min + 1) as u64;
                progress_cb(done_rows, total_ty_rows);
            }

            let row_offset = row_in_strip as usize * out_w as usize * 4;
            for col_tile_idx in 0..cols {
                let actual_tx = x_min + col_tile_idx;
                let tile_abs_col_start = col_tile_idx * TILE_SIZE;
                let tile_abs_col_end = tile_abs_col_start + TILE_SIZE;
                let region_col_left = tile_abs_col_start.max(out_x0);
                let region_col_right = tile_abs_col_end.min(out_x0 + out_w);
                if region_col_left >= region_col_right {
                    continue;
                }
                let tile_src_x = region_col_left - tile_abs_col_start;
                let dst_col = region_col_left - out_x0;
                let copy_w = (region_col_right - region_col_left) as usize;
                if let Some(tile_img) = tile_row_cache.get(&actual_tx) {
                    if tile_inner_y < tile_img.height() && tile_src_x + copy_w as u32 <= tile_img.width()
                    {
                        let raw = tile_img.as_raw();
                        let row_start = tire_row_byte_offset(tile_img.width(), tile_inner_y);
                        let src_start = row_start + tile_src_x as usize * 4;
                        let dst_start = row_offset + dst_col as usize * 4;
                        strip_buf[dst_start..dst_start + copy_w * 4]
                            .copy_from_slice(&raw[src_start..src_start + copy_w * 4]);
                    }
                }
            }

            if let Some(poly) = polygon {
                let row_abs = abs_out_row + row_in_strip;
                let row_lat = if merc_per_out_px > 0.0 {
                    merc_inv(merc_n_out - (row_abs as f64 + 0.5) * merc_per_out_px)
                } else {
                    geo_north - (row_abs as f64 + 0.5) * lat_scale
                };
                apply_polygon_mask_to_row(
                    &mut strip_buf,
                    out_w,
                    row_offset,
                    geo_west,
                    lon_scale,
                    row_lat,
                    poly,
                );
            }
        }

        abs_out_row += strip_rows;
        consume_strip(strip_buf)?;
    }

    if read_count == 0 {
        bail!("层级 {} 无可用瓦片数据", zoom);
    }

    Ok(read_count)
}

/// 对条带缓冲区中某像素行应用多边形掩膜（奇偶填充规则）
///
/// 对每个输出列，计算其中心经度并判断是否在多边形内；
/// 若在外部，则将该像素 RGBA 全部清零（透明）。
fn apply_polygon_mask_to_row(
    strip_buf: &mut [u8],
    out_w: u32,
    row_offset: usize,
    geo_west: f64,
    lon_per_px: f64,
    row_lat: f64,
    polygon: &[[f64; 2]],
) {
    // 计算当前扫描线与多边形各边的经度交点
    let mut crossings: Vec<f64> = Vec::new();
    let n = polygon.len();
    for i in 0..n {
        let [x0, y0] = polygon[i];
        let [x1, y1] = polygon[(i + 1) % n];
        // 严格奇偶：仅保留端点 y0 < lat <= y1 或 y1 < lat <= y0 的边
        if (y0 < row_lat) != (y1 < row_lat) {
            let t = (row_lat - y0) / (y1 - y0);
            crossings.push(x0 + t * (x1 - x0));
        }
    }
    crossings.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));

    // 逐列扫描：维护 inside 状态，在每个交叉点切换
    let mut crossing_idx = 0usize;
    let mut inside = false;
    for col in 0..out_w as usize {
        let lng = geo_west + (col as f64 + 0.5) * lon_per_px;
        while crossing_idx < crossings.len() && crossings[crossing_idx] <= lng {
            inside = !inside;
            crossing_idx += 1;
        }
        if !inside {
            let base = row_offset + col * 4;
            strip_buf[base] = 0;
            strip_buf[base + 1] = 0;
            strip_buf[base + 2] = 0;
            strip_buf[base + 3] = 0;
        }
    }
}

/// 将所有瓦片加载到单张内存 RGBA 画布，裁剪至输出像素窗口。
///
/// 返回 `(canvas, tiles_read_count)`。canvas 的尺寸为 `out_w × out_h`。
#[allow(clippy::too_many_arguments)]
fn load_source_canvas<F: Fn(u64, u64)>(
    src: &Connection,
    zoom: u8,
    x_min: u32,
    y_min: u32,
    _cols: u32,
    rows: u32,
    out_x0: u32,
    out_y0: u32,
    out_w: u32,
    out_h: u32,
    progress_cb: &F,
) -> Result<(RgbaImage, u64)> {
    let mut canvas = RgbaImage::new(out_w, out_h);
    let mut stmt =
        src.prepare("SELECT tile_column, tile_data FROM tiles WHERE zoom_level = ?1 AND tile_row = ?2")?;
    let mut read_count: u64 = 0;
    let total_ty_rows = rows as u64;

    for ty_idx in 0..rows {
        let actual_ty = y_min + ty_idx;
        let loaded: Vec<(u32, Vec<u8>)> = stmt
            .query_map(rusqlite::params![zoom as i64, actual_ty as i64], |r| {
                Ok((r.get::<_, i64>(0)? as u32, r.get::<_, Vec<u8>>(1)?))
            })?
            .filter_map(|r| r.ok())
            .collect();
        let decoded: Vec<(u32, Option<RgbaImage>)> = loaded
            .into_par_iter()
            .map(|(tx, data)| {
                let img = image::load_from_memory(&data).map(|i| i.to_rgba8()).ok();
                (tx, img)
            })
            .collect();

        for (tx, img_opt) in decoded {
            let Some(tile_img) = img_opt else { continue };
            read_count += 1;

            let tile_abs_col_start = (tx - x_min) * TILE_SIZE;
            let tile_abs_row_start = ty_idx * TILE_SIZE;
            let tile_abs_col_end = tile_abs_col_start + TILE_SIZE;
            let tile_abs_row_end = tile_abs_row_start + TILE_SIZE;

            let region_col_left = tile_abs_col_start.max(out_x0);
            let region_col_right = tile_abs_col_end.min(out_x0 + out_w);
            let region_row_top = tile_abs_row_start.max(out_y0);
            let region_row_bot = tile_abs_row_end.min(out_y0 + out_h);

            if region_col_left >= region_col_right || region_row_top >= region_row_bot {
                continue;
            }
            let tile_src_x = region_col_left - tile_abs_col_start;
            let tile_src_y = region_row_top - tile_abs_row_start;
            let dst_col = region_col_left - out_x0;
            let dst_row = region_row_top - out_y0;
            let copy_w = (region_col_right - region_col_left) as usize;
            let copy_h = region_row_bot - region_row_top;

            let raw = tile_img.as_raw();
            for r in 0..copy_h {
                let src_y = tile_src_y + r;
                let src_start = tire_row_byte_offset(tile_img.width(), src_y) + tile_src_x as usize * 4;
                let dst_y = dst_row + r;
                let dst_start = tire_row_byte_offset(out_w, dst_y) + dst_col as usize * 4;
                canvas.as_mut().get_mut(dst_start..dst_start + copy_w * 4)
                    .map(|s| s.copy_from_slice(&raw[src_start..src_start + copy_w * 4]));
            }
        }
        progress_cb(ty_idx as u64 + 1, total_ty_rows);
    }

    if read_count == 0 {
        bail!("层级 {} 无可用瓦片数据", zoom);
    }
    Ok((canvas, read_count))
}

/// 渲染重投影条带：对目标 CRS 中每一行像素，从源 CRS 画布双线性采样。
///
/// WebMercator ↔ WGS84 仅 Y 方向（行）不同，X 方向（列）直接复制。
#[allow(clippy::too_many_arguments)]
fn render_reprojected_strip(
    src_canvas: &RgbaImage,
    strip_start_row: u32,
    strip_rows: u32,
    out_h: u32,
    out_w: u32,
    geo_west: f64,
    geo_north: f64,
    geo_south: f64,
    geo_east: f64,
    src_crs: &CrsType,
    dst_crs: &TargetCrs,
    polygon: Option<&[[f64; 2]]>,
) -> Vec<u8> {
    let src_h = src_canvas.height() as f64;
    let src_raw = src_canvas.as_raw();

    // 计算源 CRS 的 Y 范围（用于行映射）
    let (src_y_north, src_y_south) = match src_crs {
        CrsType::Wgs84 => (geo_north, geo_south),
        _ => (merc(geo_north), merc(geo_south)),
    };
    let src_y_range = src_y_north - src_y_south;

    // 计算目标 CRS 的像素高度到行 Y 映射
    let (dst_y_north, dst_y_south) = match dst_crs {
        TargetCrs::Geographic { .. } => (geo_north, geo_south),
        _ => (merc(geo_north), merc(geo_south)),
    };
    let dst_y_per_px = (dst_y_north - dst_y_south) / out_h as f64;

    let lon_per_px = (geo_east - geo_west) / out_w as f64;

    let mut buf = vec![0u8; strip_rows as usize * out_w as usize * 4];

    for row_in_strip in 0..strip_rows {
        let abs_row = strip_start_row + row_in_strip;
        let dst_y = dst_y_north - (abs_row as f64 + 0.5) * dst_y_per_px;
        let lat = match dst_crs {
            TargetCrs::Geographic { .. } => dst_y,
            _ => merc_inv(dst_y),
        };
        let src_y_coord = match src_crs {
            CrsType::Wgs84 => lat,
            _ => merc(lat),
        };
        let src_row_f = (src_y_north - src_y_coord) * src_h / src_y_range - 0.5;
        let row0 = src_row_f.floor() as i64;
        let row1 = row0 + 1;
        let t = src_row_f - row0 as f64;

        let row_offset = row_in_strip as usize * out_w as usize * 4;

        for col in 0..out_w as usize {
            let src_col = col;
            let get_px = |row: i64| -> [u8; 4] {
                if row < 0 || row >= src_h as i64 {
                    return [0, 0, 0, 0];
                }
                let src_start = tire_row_byte_offset(out_w, row as u32) + src_col * 4;
                if src_start + 4 > src_raw.len() {
                    return [0, 0, 0, 0];
                }
                [
                    src_raw[src_start],
                    src_raw[src_start + 1],
                    src_raw[src_start + 2],
                    src_raw[src_start + 3],
                ]
            };
            let p0 = get_px(row0);
            let p1 = get_px(row1);
            let blend = |a: u8, b: u8| -> u8 {
                (a as f64 * (1.0 - t) + b as f64 * t).round() as u8
            };
            let dst_start = row_offset + col * 4;
            buf[dst_start] = blend(p0[0], p1[0]);
            buf[dst_start + 1] = blend(p0[1], p1[1]);
            buf[dst_start + 2] = blend(p0[2], p1[2]);
            buf[dst_start + 3] = blend(p0[3], p1[3]);
        }

        if let Some(poly) = polygon {
            apply_polygon_mask_to_row(&mut buf, out_w, row_offset, geo_west, lon_per_px, lat, poly);
        }
    }

    buf
}

/// UTM 2D 重投影条带渲染：对每个输出像素做 UTM→WGS84→源画布 双线性采样。
#[allow(clippy::too_many_arguments)]
fn render_reprojected_strip_utm(
    src_canvas: &RgbaImage,
    strip_start_row: u32,
    strip_rows: u32,
    out_h: u32,
    out_w: u32,
    geo_west: f64,
    geo_north: f64,
    geo_south: f64,
    geo_east: f64,
    utm_east_min: f64,
    utm_north_max: f64,
    utm_east_max: f64,
    utm_north_min: f64,
    zone: u8,
    is_north: bool,
    src_crs: &CrsType,
    polygon: Option<&[[f64; 2]]>,
) -> Vec<u8> {
    let src_w = src_canvas.width() as f64;
    let src_h = src_canvas.height() as f64;
    let src_raw = src_canvas.as_raw();

    let scale_x = (utm_east_max - utm_east_min) / out_w as f64;
    let scale_y = (utm_north_max - utm_north_min) / out_h as f64;

    // 源 CRS 范围（行列映射用）
    let (src_merc_north, src_merc_south) = (merc(geo_north), merc(geo_south));

    let _lon_per_px = (geo_east - geo_west) / out_w as f64;

    let mut buf = vec![0u8; strip_rows as usize * out_w as usize * 4];

    for row_in_strip in 0..strip_rows {
        let abs_row = strip_start_row + row_in_strip;
        let row_offset = row_in_strip as usize * out_w as usize * 4;

        for col in 0..out_w as usize {
            let utm_e = utm_east_min + (col as f64 + 0.5) * scale_x;
            let utm_n = utm_north_max - (abs_row as f64 + 0.5) * scale_y;
            let (lat, lon) = utm_proj::utm_to_wgs84(utm_e, utm_n, zone, is_north);

            // 超出源范围的像素保持透明
            if lat < geo_south || lat > geo_north || lon < geo_west || lon > geo_east {
                continue;
            }

            // 多边形掩膜逐像素检查
            if let Some(poly) = polygon {
                if !point_in_polygon(lon, lat, poly) {
                    continue;
                }
            }

            // 源画布列
            let src_col_f = (lon - geo_west) * src_w / (geo_east - geo_west) - 0.5;
            // 源画布行
            let src_row_f = match src_crs {
                CrsType::Wgs84 => (geo_north - lat) * src_h / (geo_north - geo_south) - 0.5,
                _ => (src_merc_north - merc(lat)) * src_h / (src_merc_north - src_merc_south) - 0.5,
            };

            let col0 = src_col_f.floor() as i64;
            let col1 = col0 + 1;
            let tc = src_col_f - col0 as f64;
            let row0 = src_row_f.floor() as i64;
            let row1 = row0 + 1;
            let tr = src_row_f - row0 as f64;

            let get_px = |r: i64, c: i64| -> [u8; 4] {
                if r < 0 || r >= src_h as i64 || c < 0 || c >= src_w as i64 {
                    return [0, 0, 0, 0];
                }
                let start = tire_row_byte_offset(src_canvas.width(), r as u32) + c as usize * 4;
                if start + 4 > src_raw.len() {
                    return [0, 0, 0, 0];
                }
                [src_raw[start], src_raw[start + 1], src_raw[start + 2], src_raw[start + 3]]
            };

            let p00 = get_px(row0, col0);
            let p01 = get_px(row0, col1);
            let p10 = get_px(row1, col0);
            let p11 = get_px(row1, col1);

            let blend = |a: u8, b: u8, c: u8, d: u8| -> u8 {
                let top = a as f64 * (1.0 - tc) + b as f64 * tc;
                let bot = c as f64 * (1.0 - tc) + d as f64 * tc;
                (top * (1.0 - tr) + bot * tr).round() as u8
            };

            let dst = row_offset + col * 4;
            buf[dst]     = blend(p00[0], p01[0], p10[0], p11[0]);
            buf[dst + 1] = blend(p00[1], p01[1], p10[1], p11[1]);
            buf[dst + 2] = blend(p00[2], p01[2], p10[2], p11[2]);
            buf[dst + 3] = blend(p00[3], p01[3], p10[3], p11[3]);
        }
    }

    buf
}

/// 点在多边形内检测（奇偶规则）
fn point_in_polygon(lon: f64, lat: f64, polygon: &[[f64; 2]]) -> bool {
    let n = polygon.len();
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let [xi, yi] = polygon[i];
        let [xj, yj] = polygon[j];
        if ((yi > lat) != (yj > lat)) && (lon < (xj - xi) * (lat - yi) / (yj - yi) + xi) {
            inside = !inside;
        }
        j = i;
    }
    inside
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufReader;
    use std::time::{SystemTime, UNIX_EPOCH};

    use image::{DynamicImage, ImageFormat};
    use rusqlite::params;
    use tiff::decoder::{Decoder, DecodingResult};

    #[test]
    fn writes_wgs84_georeference_in_degrees() {
        let geo = build_geo_reference(&TargetCrs::Geographic { epsg: 4326 }, 120.0, 31.0, 121.0, 30.0, 256, 256);
        assert_eq!(geo.model_west, 120.0);
        assert_eq!(geo.model_north, 31.0);
        assert!((geo.pixel_scale_x - (1.0 / 256.0)).abs() < 1e-12);
        assert!((geo.pixel_scale_y - (1.0 / 256.0)).abs() < 1e-12);
        assert_eq!(
            geo.geo_keys,
            vec![1, 1, 0, 3, 1024, 0, 1, 2, 1025, 0, 1, 1, 2048, 0, 1, 4326,]
        );
    }

    #[test]
    fn writes_web_mercator_georeference_in_meters() {
        let geo = build_geo_reference(
            &TargetCrs::WebMercator,
            -180.0,
            85.051_128_779_806_6,
            180.0,
            -85.051_128_779_806_6,
            256,
            256,
        );
        assert!((geo.model_west + 20_037_508.342_789_244).abs() < 1e-6);
        assert!((geo.model_north - 20_037_508.342_789_244).abs() < 1e-6);
        assert!((geo.pixel_scale_x - 156_543.033_928_040_97).abs() < 1e-6);
        assert!((geo.pixel_scale_y - 156_543.033_928_040_97).abs() < 1e-6);
        assert_eq!(
            geo.geo_keys,
            vec![1, 1, 0, 3, 1024, 0, 1, 1, 1025, 0, 1, 1, 3072, 0, 1, 3857,]
        );
    }

    #[test]
    fn exports_readable_bigtiff() -> Result<()> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let temp_dir = std::env::temp_dir();
        let tile_store_path = temp_dir.join(format!("tilegrabber-geotiff-{nonce}.tiles"));
        let tif_path = temp_dir.join(format!("tilegrabber-geotiff-{nonce}.tif"));

        let result = (|| -> Result<()> {
            let conn = Connection::open(&tile_store_path)?;
            conn.execute_batch(
                "CREATE TABLE tiles (
                    zoom_level INTEGER NOT NULL,
                    tile_column INTEGER NOT NULL,
                    tile_row INTEGER NOT NULL,
                    tile_data BLOB NOT NULL
                );",
            )?;

            let tile = RgbaImage::from_pixel(TILE_SIZE, TILE_SIZE, image::Rgba([255, 0, 0, 255]));
            let mut encoded = Vec::new();
            DynamicImage::ImageRgba8(tile).write_to(
                &mut std::io::Cursor::new(&mut encoded),
                ImageFormat::Png,
            )?;

            conn.execute(
                "INSERT INTO tiles (zoom_level, tile_column, tile_row, tile_data)
                 VALUES (?1, ?2, ?3, ?4)",
                params![0_i64, 0_i64, 0_i64, encoded],
            )?;

            export_geotiff(
                &tile_store_path,
                &tif_path,
                [-180.0, -85.051_128_779_806_6, 180.0, 85.051_128_779_806_6],
                0,
                false,
                None,
                &CrsType::WebMercator,
                None,
                "none",
                |_, _| {},
            )?;

            let file = std::fs::File::open(&tif_path)?;
            let mut decoder = Decoder::new(BufReader::new(file))?;
            assert_eq!(decoder.dimensions()?, (256, 256));
            assert_eq!(decoder.get_tag_unsigned::<u16>(Tag::ExtraSamples)?, 2);
            match decoder.read_image()? {
                DecodingResult::U8(buf) => assert_eq!(buf.len(), 256 * 256 * 4),
                other => panic!("unexpected decoding result: {other:?}"),
            }

            Ok(())
        })();

        let _ = std::fs::remove_file(&tile_store_path);
        let _ = std::fs::remove_file(&tif_path);
        result
    }
}
