//! TileGrabber — 导出模块
//!
//! 支持将已下载的瓦片包导出为：
//! - MBTiles（OGC 标准 SQLite 格式，供 MapTiler/QGis/离线地图使用）
//! - 目录（z/x/y.{ext} 文件树，供各种 Web 服务使用）
//! - GeoTIFF（地理参考栅格图像，供 QGIS/ArcGIS/遥感软件使用）

pub mod directory;
pub mod geotiff;
pub mod mbtiles;
pub mod mvt_scan;
pub mod pmtiles;
pub mod tile_clip;
pub mod utm;

// ─── 瓦片重编码工具 ──────────────────────────────────────────────────────────

/// 按需对瓦片进行重编码（更改 JPEG 品质或 PNG 压缩级别）。
///
/// - JPEG 瓦片（`\xff\xd8` 魔数）：当 `jpeg_quality` 为 `Some(q)` 时重编码（`q`: 1–100）
/// - PNG 瓦片（`\x89PNG` 魔数）：当 `png_level` 为 `Some(level)` 时重编码（`level`: 0–9）
/// - 其他格式（WebP 等）：原样返回
/// - 任何解码/编码失败时原样返回（不报错）
pub fn try_reencode_tile(
    data: &[u8],
    jpeg_quality: Option<u8>,
    png_level: Option<u8>,
) -> std::borrow::Cow<'_, [u8]> {
    use image::ImageFormat;
    use std::io::Cursor;

    if data.starts_with(&[0xFF, 0xD8]) {
        if let Some(q) = jpeg_quality {
            if let Ok(img) = image::load_from_memory_with_format(data, ImageFormat::Jpeg) {
                let mut buf = Vec::with_capacity(data.len());
                let q = q.clamp(1, 100);
                let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, q);
                if enc.encode_image(&img).is_ok() {
                    return std::borrow::Cow::Owned(buf);
                }
            }
        }
    } else if data.starts_with(b"\x89PNG") {
        if let Some(level) = png_level {
            if let Ok(img) = image::load_from_memory_with_format(data, ImageFormat::Png) {
                let mut buf = Vec::with_capacity(data.len());
                let compression = match level.clamp(0, 9) {
                    0 => image::codecs::png::CompressionType::Fast,
                    1..=3 => image::codecs::png::CompressionType::Fast,
                    4..=6 => image::codecs::png::CompressionType::Default,
                    _ => image::codecs::png::CompressionType::Best,
                };
                let enc = image::codecs::png::PngEncoder::new_with_quality(
                    Cursor::new(&mut buf),
                    compression,
                    image::codecs::png::FilterType::Adaptive,
                );
                if img.write_with_encoder(enc).is_ok() {
                    return std::borrow::Cow::Owned(buf);
                }
            }
        }
    }

    std::borrow::Cow::Borrowed(data)
}
