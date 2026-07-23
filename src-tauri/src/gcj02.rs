//! GCJ02（"火星坐标"）转换与瓦片像素偏移
//!
//! 提供 WGS84→GCJ02 坐标转换，以及 XYZ 瓦片纠偏合成所需的像素偏移量计算。
//! 算法来自公开的 EvilTransform。

use std::f64::consts::PI;

const A: f64 = 6_378_245.0;
const EE: f64 = 0.006_693_421_622_965_943;

/// 判断经纬度是否在中国大陆范围外（范围外无需 GCJ02 纠偏）
pub fn out_of_china(lng: f64, lat: f64) -> bool {
    lng < 72.004 || lng > 137.8347 || lat < 0.8293 || lat > 55.8271
}

fn transform_lat(x: f64, y: f64) -> f64 {
    let mut ret = -100.0 + 2.0 * x + 3.0 * y + 0.2 * y * y + 0.1 * x * y + 0.2 * x.abs().sqrt();
    ret += (20.0 * (6.0 * x * PI).sin() + 20.0 * (2.0 * x * PI).sin()) * (2.0 / 3.0);
    ret += (20.0 * (y * PI).sin() + 40.0 * (y / 3.0 * PI).sin()) * (2.0 / 3.0);
    ret += (160.0 * (y / 12.0 * PI).sin() + 320.0 * (y * PI / 30.0).sin()) * (2.0 / 3.0);
    ret
}

fn transform_lng(x: f64, y: f64) -> f64 {
    let mut ret = 300.0 + x + 2.0 * y + 0.1 * x * x + 0.1 * x * y + 0.1 * x.abs().sqrt();
    ret += (20.0 * (6.0 * x * PI).sin() + 20.0 * (2.0 * x * PI).sin()) * (2.0 / 3.0);
    ret += (20.0 * (x * PI).sin() + 40.0 * (x / 3.0 * PI).sin()) * (2.0 / 3.0);
    ret += (150.0 * (x / 12.0 * PI).sin() + 300.0 * (x / 30.0 * PI).sin()) * (2.0 / 3.0);
    ret
}

/// WGS84 → GCJ02 坐标转换（公开的 EvilTransform 算法）
///
/// 返回 `(gcj02_lng, gcj02_lat)`。对中国境外坐标直接返回原值。
pub fn wgs84_to_gcj02(lng: f64, lat: f64) -> (f64, f64) {
    if out_of_china(lng, lat) {
        return (lng, lat);
    }

    let mut d_lat = transform_lat(lng - 105.0, lat - 35.0);
    let mut d_lng = transform_lng(lng - 105.0, lat - 35.0);

    let rad_lat = lat / 180.0 * PI;
    let magic = 1.0 - EE * rad_lat.sin().powi(2);
    let sqrt_magic = magic.sqrt();

    d_lat = d_lat * 180.0 / (A * (1.0 - EE) / (magic * sqrt_magic) * PI);
    d_lng = d_lng * 180.0 / (A / sqrt_magic * rad_lat.cos() * PI);

    (lng + d_lng, lat + d_lat)
}

/// WebMercator 对数纬度（Mercator Y），单位：弧度
#[inline]
fn merc_y(lat_deg: f64) -> f64 {
    let r = lat_deg.to_radians();
    (PI / 4.0 + r / 2.0).tan().ln()
}

/// 计算 XYZ 瓦片中心点的 WGS84 经纬度 `(lng, lat)`
fn tile_center_wgs84(z: u8, x: u32, y: u32) -> (f64, f64) {
    let n = (1u64 << z) as f64;
    let lng = (x as f64 + 0.5) / n * 360.0 - 180.0;
    let lat_rad = (PI * (1.0 - 2.0 * (y as f64 + 0.5) / n)).sinh().atan();
    (lng, lat_rad.to_degrees())
}

/// 计算 XYZ 瓦片 (z, x, y) 的 GCJ02 纠偏像素偏移 `(dx, dy)`。
///
/// - `dx` ≥ 0：Gaode 内容偏东（屏幕右方）
/// - `dy` ≤ 0：Gaode 内容偏北（屏幕上方，WebMercator y 减小方向）
///
/// 合成策略：目标瓦片 (z, x, y) 的像素内容来自
/// `(z, x + dx.div_euclid(256), y + dy.div_euclid(256))` 起始的 2×2 源瓦片。
/// 仅适用于 256×256 像素的 XYZ（north_to_south=true）WebMercator 瓦片。
pub fn gcj02_pixel_delta(z: u8, x: u32, y: u32) -> (i32, i32) {
    let (lng, lat) = tile_center_wgs84(z, x, y);
    let (gcj_lng, gcj_lat) = wgs84_to_gcj02(lng, lat);

    let total_px = 256.0 * (1u64 << z) as f64;

    // 经度偏移→像素（向东为正）
    let dx = ((gcj_lng - lng) / 360.0 * total_px).round() as i32;

    // 纬度偏移→像素（向北 = WebMercator y 减小 = dy 为负）
    let dy = ((merc_y(lat) - merc_y(gcj_lat)) / (2.0 * PI) * total_px).round() as i32;

    (dx, dy)
}

// ─── 单元测试 ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// 中国境外坐标应原样返回
    #[test]
    fn out_of_china_passes_through() {
        let (lng, lat) = (139.6917, 35.6895); // 东京
        assert_eq!(wgs84_to_gcj02(lng, lat), (lng, lat));
    }

    /// 境内坐标应有非零偏移（北京天安门公开测试点）
    #[test]
    fn beijing_has_offset() {
        let wgs = (116.3974, 39.9093);
        let (gcj_lng, gcj_lat) = wgs84_to_gcj02(wgs.0, wgs.1);
        let dlng = gcj_lng - wgs.0;
        let dlat = gcj_lat - wgs.1;
        assert!(dlng.abs() > 1e-5, "经度偏移应显著: {dlng}");
        assert!(dlat.abs() > 1e-5, "纬度偏移应显著: {dlat}");
    }

    /// 偏移量在不同缩放层级下不应异常巨大
    #[test]
    fn pixel_delta_order_of_magnitude() {
        let (dx, dy) = gcj02_pixel_delta(10, 857, 418);
        assert!(dx.abs() < 500, "z10 经度像素偏移不应超过 500: {dx}");
        assert!(dy.abs() < 500, "z10 纬度像素偏移不应超过 500: {dy}");
    }

    /// pixel_delta 在境内应非零，境外应接近零
    #[test]
    fn pixel_delta_zero_outside_china() {
        // 东京附近瓦片
        let (dx, dy) = gcj02_pixel_delta(8, 232, 101);
        assert_eq!(dx, 0, "境外 dx 应为 0");
        assert_eq!(dy, 0, "境外 dy 应为 0");
    }
}
