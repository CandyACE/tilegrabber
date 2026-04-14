//! 瓦片图像地理范围裁剪
//!
//! 将超出任务范围的瓦片像素设为透明（alpha=0），范围内保留原始像素。
//! 边框相交的瓦片不再被整体排除，而是对其外部像素进行透明化处理。
//!
//! # 坐标系
//! - WebMercator: 经度线性分布；纬度通过 Mercator Y 投影与像素行线性对应
//! - WGS84: 经度和纬度均与像素线性对应
//!
//! # 性能优化
//! - **就地修改**：直接在解码后的原图 raw buffer 清零范围外像素（消除第二缓冲区分配）
//! - **批量清零**：`slice::fill(0)` 替代逐像素 put_pixel（触发 SIMD/memset 自动向量化）
//! - **快速 PNG 编码**：zlib `Fast` 压缩级别，速度约 2-4×，仅影响边界瓦片体积
//! - **扫描线多边形填充**：O(H × V) 替代 O(H × W × V) 的逐像素射线法
//! - **覆盖率 alpha 抗锯齿**：裁剪边界处的像素按子像素覆盖比例设半透明，消除锯齿

use std::f64::consts::PI;
use std::io::Cursor;

use anyhow::{Context, Result};
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ImageEncoder, RgbaImage};

use crate::types::{Bounds, CrsType};

// ── 内部辅助 ─────────────────────────────────────────────────────────────────

/// 使用 zlib Fast 压缩将 RGBA 图像编码为 PNG。
///
/// 相对于默认压缩（level 6），速度约快 2-4×，文件体积增大约 30-50%。
/// 仅用于边界瓦片，对整体存储体积影响有限。
#[inline]
fn encode_png_fast(img: &RgbaImage) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    PngEncoder::new_with_quality(
        Cursor::new(&mut buf),
        CompressionType::Fast,
        FilterType::Sub,
    )
    .write_image(
        img.as_raw(),
        img.width(),
        img.height(),
        image::ExtendedColorType::Rgba8,
    )
    .context("编码裁剪后瓦片为 PNG 失败")?;
    Ok(buf)
}

/// 对单行的指定列范围内每个像素的 alpha 通道乘以 `factor`（0.0–1.0）。
///
/// 用于在裁剪边界列实现覆盖率抗锯齿；内部的非边界像素不走此路径。
#[inline]
fn blend_alpha_col(raw: &mut [u8], row: usize, stride: usize, col: usize, factor: f64) {
    let off = row * stride + col * 4 + 3; // alpha 字节偏移
    if off < raw.len() {
        raw[off] = (raw[off] as f64 * factor).round() as u8;
    }
}


// ── 公开 API ──────────────────────────────────────────────────────────────────

/// 将单张瓦片图像裁剪至任务地理范围（WebMercator 专用，供导出模块使用）。
///
/// # 返回值
/// - `Ok(None)` — 瓦片与任务范围无交集，应跳过
/// - `Ok(Some(data))` — 处理后的图像字节：
///   - 若瓦片完全在范围内，返回原始 `data` 的副本（不重新编码）
///   - 若瓦片与范围边界相交，返回 RGBA PNG：范围外像素透明（alpha=0），
///     边界像素按覆盖率半透明（抗锯齿）
pub fn clip_tile_to_bounds(
    data: &[u8],
    x: u32,
    y: u32,
    zoom: u8,
    task_bounds: &Bounds,
) -> Result<Option<Vec<u8>>> {
    clip_tile_to_bounds_crs(data, x, y, zoom, task_bounds, &CrsType::WebMercator)
}

/// 将单张瓦片图像裁剪至任务地理范围（支持 WebMercator 和 WGS84）。
///
/// 边界像素按子像素覆盖率设 alpha（覆盖率抗锯齿），消除裁剪边缘锯齿。
///
/// # 返回值
/// - `Ok(None)` — 瓦片与任务范围无交集，应跳过
/// - `Ok(Some(data))` — 处理后的图像字节
pub fn clip_tile_to_bounds_crs(
    data: &[u8],
    x: u32,
    y: u32,
    zoom: u8,
    task_bounds: &Bounds,
    crs: &CrsType,
) -> Result<Option<Vec<u8>>> {
    let tb = crate::tile_math::tile_to_lonlat_bounds(x, y, zoom, crs);

    // ── 无交集：瓦片完全在范围外 ─────────────────────────────────────────
    if tb.east <= task_bounds.west
        || tb.west >= task_bounds.east
        || tb.north <= task_bounds.south
        || tb.south >= task_bounds.north
    {
        return Ok(None);
    }

    // ── 完全包含：无需修改，直接返回原始字节 ─────────────────────────────
    if tb.west >= task_bounds.west
        && tb.east <= task_bounds.east
        && tb.south >= task_bounds.south
        && tb.north <= task_bounds.north
    {
        return Ok(Some(data.to_vec()));
    }

    // ── 部分重叠：就地处理 ──────────────────────────────────────────────
    let mut img = image::load_from_memory(data)
        .context("解码瓦片图像失败")?
        .into_rgba8();

    let iw = img.width() as usize;
    let ih = img.height() as usize;
    let iw_f = iw as f64;
    let ih_f = ih as f64;

    // 计算裁剪矩形的精确浮点像素坐标（无取整）
    let (col_left_f, col_right_f, row_top_f, row_bot_f) = match crs {
        CrsType::Wgs84 => {
            let lng_span = tb.east - tb.west;
            let lat_span = tb.north - tb.south;
            let clip_w = task_bounds.west.max(tb.west);
            let clip_e = task_bounds.east.min(tb.east);
            let clip_n = task_bounds.north.min(tb.north);
            let clip_s = task_bounds.south.max(tb.south);
            let col_l = (clip_w - tb.west) / lng_span * iw_f;
            let col_r = (clip_e - tb.west) / lng_span * iw_f;
            let row_t = (tb.north - clip_n) / lat_span * ih_f;
            let row_b = (tb.north - clip_s) / lat_span * ih_f;
            (col_l, col_r, row_t, row_b)
        }
        _ => {
            let lng_span = tb.east - tb.west;
            let clip_w = task_bounds.west.max(tb.west);
            let clip_e = task_bounds.east.min(tb.east);
            let col_l = (clip_w - tb.west) / lng_span * iw_f;
            let col_r = (clip_e - tb.west) / lng_span * iw_f;

            let merc = |lat: f64| -> f64 {
                (PI / 4.0 + lat.to_radians() / 2.0).tan().ln()
            };
            let merc_n    = merc(tb.north);
            let merc_span = merc_n - merc(tb.south);
            let clip_n = task_bounds.north.min(tb.north);
            let clip_s = task_bounds.south.max(tb.south);
            let row_t = (merc_n - merc(clip_n)) / merc_span * ih_f;
            let row_b = (merc_n - merc(clip_s)) / merc_span * ih_f;
            (col_l, col_r, row_t, row_b)
        }
    };

    // 整数边界（完全清零区域）
    let col_left   = col_left_f.ceil()  as usize;
    let col_right  = col_right_f.floor() as usize;
    let row_top    = row_top_f.ceil()   as usize;
    let row_bottom = row_bot_f.floor()  as usize;

    // 边界列/行的抗锯齿覆盖率
    let aa_col_left   = col_left_f.ceil()  - col_left_f;  // 左边界列覆盖率
    let aa_col_right  = col_right_f        - col_right_f.floor(); // 右边界列覆盖率
    let aa_row_top    = row_top_f.ceil()   - row_top_f;   // 上边界行覆盖率
    let aa_row_bottom = row_bot_f          - row_bot_f.floor(); // 下边界行覆盖率

    // 越界保护
    let col_left   = col_left.min(iw);
    let col_right  = col_right.min(iw);
    let row_top    = row_top.min(ih);
    let row_bottom = row_bottom.min(ih);
    let aa_col_l_idx = (col_left_f.floor() as usize).min(iw.saturating_sub(1));
    let aa_col_r_idx = (col_right_f.floor() as usize).min(iw.saturating_sub(1));
    let aa_row_t_idx = (row_top_f.floor() as usize).min(ih.saturating_sub(1));
    let aa_row_b_idx = (row_bot_f.floor() as usize).min(ih.saturating_sub(1));

    let stride = iw * 4;
    {
        let raw = img.as_mut();

        // 1. 顶部整块行：批量清零
        if row_top > 0 {
            let clear_end = if aa_row_top > 0.0 && aa_row_t_idx < row_top {
                aa_row_t_idx * stride
            } else {
                row_top * stride
            };
            raw[..clear_end.min(row_top * stride)].fill(0);
        }

        // 1a. 上边界抗锯齿行
        if aa_row_top > 0.0 && aa_row_top < 1.0 && aa_row_t_idx < ih {
            // 先清零整行，再对保留列应用覆盖率
            let base = aa_row_t_idx * stride;
            raw[base..base + stride].fill(0);
            for col in col_left..col_right {
                let off = base + col * 4 + 3;
                if off < raw.len() {
                    raw[off] = (255.0 * aa_row_top).round() as u8;
                }
            }
        }

        // 2. 中间行：清零左右边距
        let left_bytes  = col_left  * 4;
        let right_start = col_right * 4;
        for row in row_top..row_bottom {
            let base = row * stride;
            // 左边整块清零
            if left_bytes > 0 {
                raw[base..base + left_bytes].fill(0);
            }
            // 左边界抗锯齿列
            if aa_col_left > 0.0 && aa_col_left < 1.0 && aa_col_l_idx < col_left {
                blend_alpha_col(raw, row, stride, aa_col_l_idx, aa_col_left);
            }
            // 右边整块清零
            if right_start < stride {
                raw[base + right_start..base + stride].fill(0);
            }
            // 右边界抗锯齿列
            if aa_col_right > 0.0 && aa_col_right < 1.0 && aa_col_r_idx >= col_right {
                blend_alpha_col(raw, row, stride, aa_col_r_idx, aa_col_right);
            }
        }

        // 2a. 下边界抗锯齿行
        if aa_row_bottom > 0.0 && aa_row_bottom < 1.0 && aa_row_b_idx >= row_bottom && aa_row_b_idx < ih {
            let base = aa_row_b_idx * stride;
            raw[base..base + stride].fill(0);
            for col in col_left..col_right {
                let off = base + col * 4 + 3;
                if off < raw.len() {
                    raw[off] = (255.0 * aa_row_bottom).round() as u8;
                }
            }
        }

        // 3. 底部整块行：批量清零
        let clear_start = if aa_row_bottom > 0.0 && aa_row_b_idx >= row_bottom {
            (aa_row_b_idx + 1) * stride
        } else {
            row_bottom * stride
        };
        if clear_start < raw.len() {
            raw[clear_start..].fill(0);
        }
    }

    encode_png_fast(&img).map(Some)
}

/// 将单张瓦片图像按多边形进行像素级裁剪（多边形范围外的像素设为透明）。
///
/// `polygon` 为经纬度坐标 `[[lng, lat], ...]`，首尾不需要重复闭合。
/// 返回 RGBA PNG 图像；若瓦片与多边形无交集则返回 `Ok(None)`。
///
/// # 算法
/// 预先将多边形顶点投影到像素坐标，然后对每行使用扫描线填充（even-odd rule），
/// 避免原来对每个像素做地理坐标反投影 + 射线法判定的 O(H×W×V) 开销，
/// 降至 O(H×V)（V = 多边形顶点数）。
///
/// # 抗锯齿
/// 每行进入/退出多边形的第一个和最后一个像素按子像素 x 覆盖率设 alpha，
/// 消除多边形裁剪边缘的锯齿感。
pub fn clip_tile_to_polygon_crs(
    data: &[u8],
    x: u32,
    y: u32,
    zoom: u8,
    polygon: &[[f64; 2]],
    crs: &CrsType,
) -> Result<Option<Vec<u8>>> {
    use crate::tile_math::{tile_intersects_polygon, tile_to_lonlat_bounds};

    if !tile_intersects_polygon(x, y, zoom, polygon, crs) {
        return Ok(None);
    }

    let mut img = image::load_from_memory(data)
        .context("解码瓦片图像失败")?
        .into_rgba8();

    let iw = img.width() as usize;
    let ih = img.height() as usize;
    let iw_f = iw as f64;
    let ih_f = ih as f64;

    let tb = tile_to_lonlat_bounds(x, y, zoom, crs);
    let lng_span = tb.east - tb.west;
    let lat_span = tb.north - tb.south;

    let is_wgs84 = matches!(crs, CrsType::Wgs84);

    // WebMercator 预计算 Mercator 参数（避免在顶点投影循环中重复计算）
    let merc = |lat: f64| -> f64 { (PI / 4.0 + lat.to_radians() / 2.0).tan().ln() };
    let (merc_n, merc_span) = if !is_wgs84 {
        let mn = merc(tb.north);
        (mn, mn - merc(tb.south))
    } else {
        (0.0, 0.0)
    };

    // 一次性将多边形顶点投影到像素坐标（消除热循环中的地理坐标变换）
    let n = polygon.len();
    let px_verts: Vec<(f64, f64)> = polygon
        .iter()
        .map(|v| {
            let col = (v[0] - tb.west) / lng_span * iw_f;
            let row = if is_wgs84 {
                (tb.north - v[1]) / lat_span * ih_f
            } else {
                (merc_n - merc(v[1])) / merc_span * ih_f
            };
            (col, row)
        })
        .collect();

    // 扫描线多边形填充（even-odd rule）+ 覆盖率 alpha 抗锯齿
    let raw = img.as_mut();
    let stride = iw * 4;
    let mut xs: Vec<f64> = Vec::with_capacity(n); // 跨行复用，避免重复分配

    for row in 0..ih {
        let y_scan = row as f64 + 0.5; // 像素中心

        // 收集所有与扫描线严格跨越的边的 x 交点
        xs.clear();
        let mut j = n - 1;
        for i in 0..n {
            let (xi, yi) = px_verts[i];
            let (xj, yj) = px_verts[j];
            if (yi > y_scan) != (yj > y_scan) {
                let x_inter = (xj - xi) * (y_scan - yi) / (yj - yi) + xi;
                xs.push(x_inter);
            }
            j = i;
        }

        let row_start = row * stride;
        let row_buf   = &mut raw[row_start..row_start + stride];

        if xs.is_empty() {
            row_buf.fill(0); // 整行在多边形外
            continue;
        }

        xs.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        // 按奇偶规则清零外部区间，并对进入/退出边界像素应用覆盖率 alpha
        // 外部区间：[0, xs[0])、[xs[1], xs[2])、…、[xs[last], iw)
        // 内部区间：[xs[0], xs[1])、[xs[2], xs[3])、…（跳过，不操作）
        let mut prev = 0usize;
        let mut k = 0;
        while k < xs.len() {
            // [prev, xs[k]) 是外部 → 清零
            let x_enter_f = xs[k].max(0.0).min(iw_f);
            let x_enter_i = x_enter_f.floor() as usize; // 进入的第一个完整像素
            let x_enter_clear = x_enter_i.min(iw); // 完全清零截止到 x_enter_i

            if x_enter_clear > prev {
                row_buf[prev * 4..x_enter_clear * 4].fill(0);
            }

            // 进入边界像素：按子像素覆盖率 (1 - frac) 混合 alpha
            let enter_frac = x_enter_f - x_enter_i as f64; // 已被清零的小数部分
            if enter_frac > 0.0 && x_enter_i < iw {
                let off = x_enter_i * 4 + 3;
                if off < row_buf.len() {
                    row_buf[off] = (255.0 * (1.0 - enter_frac)).round() as u8;
                }
            }

            if k + 1 < xs.len() {
                // [xs[k], xs[k+1]) 是内部 → 保留，处理退出边界像素
                let x_exit_f = xs[k + 1].max(0.0).min(iw_f);
                let x_exit_i = x_exit_f.floor() as usize;
                let exit_frac = x_exit_f - x_exit_i as f64; // 退出后仍在像素内的小数部分

                // 退出边界像素：按覆盖率 frac 混合 alpha
                if exit_frac > 0.0 && x_exit_i < iw {
                    let off = x_exit_i * 4 + 3;
                    if off < row_buf.len() {
                        row_buf[off] = (255.0 * exit_frac).round() as u8;
                    }
                }

                prev = (x_exit_i + if exit_frac > 0.0 { 1 } else { 0 }).min(iw);
                k += 2;
            } else {
                // 奇数交点（顶点恰好在扫描线上的退化情况）→ 视为内部，不清零尾部
                prev = iw;
                k += 1;
            }
        }
        // 最后一个内部区间结束后的尾部 → 外部，清零
        if prev < iw {
            row_buf[prev * 4..].fill(0);
        }
    }

    encode_png_fast(&img).map(Some)
}