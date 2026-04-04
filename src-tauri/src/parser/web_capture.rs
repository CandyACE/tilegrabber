//! 瓦片 URL 工具函数
//!
//! 提供将具体瓦片 URL（如 .../15/12345/6789.png）转换为
//! {z}/{x}/{y} 模板的能力，供网页抓取捕获会话使用。

use crate::types::{CrsType, SourceKind, TileSource};

/// 将一条具体的瓦片 URL 转换为 `{z}/{x}/{y}` 模板
///
/// 例：`https://tile.example.com/15/12345/6789.png`
///   → `https://tile.example.com/{z}/{x}/{y}.png`
///
/// 支持路径中有额外前缀，如 `.../maps/layer1/15/12345/6789.png`
/// 同时支持查询参数形式：`?x=849&y=419&l=10` → `?x={x}&y={y}&l={z}`
pub fn tile_url_to_template(raw: &str) -> Option<String> {
    // 分离查询字符串
    let (base, query) = match raw.find('?') {
        Some(i) => (&raw[..i], raw[i..].to_string()),
        None => (raw, String::new()),
    };

    // 分离 scheme://host 和路径
    let scheme_end = base.find("://")?;
    let after_scheme = scheme_end + 3;
    let host_end = base[after_scheme..]
        .find('/')
        .map(|i| after_scheme + i)
        .unwrap_or(base.len());

    let host_part = &base[..host_end];       // "https://tile.example.com"
    let path = &base[host_end..];            // "/15/12345/6789.png"

    let path_segs: Vec<&str> = path.split('/').collect();

    // 从前向后找连续三段可解析为 (z, x, y) 的路径节
    for i in 0..path_segs.len().saturating_sub(2) {
        let z_s = path_segs[i];
        let x_s = path_segs[i + 1];
        let y_s_raw = path_segs[i + 2];

        // y 节可能带文件扩展名
        let (y_s, ext) = match y_s_raw.rfind('.') {
            Some(d) => (&y_s_raw[..d], y_s_raw[d..].to_string()),
            None => (y_s_raw, String::new()),
        };

        let Ok(z) = z_s.parse::<u64>() else { continue };
        let Ok(_x) = x_s.parse::<u64>() else { continue };
        let Ok(_y) = y_s.parse::<u64>() else { continue };

        // z > 24 通常不是有效缩放级别
        if z > 24 {
            continue;
        }

        let pre_path = path_segs[..i].join("/"); // 路径前缀
        let template_path = format!("{}/{{z}}/{{x}}/{{y}}{}", pre_path, ext);
        return Some(format!("{}{}{}", host_part, template_path, query));
    }

    // 路径中未找到，尝试查询参数形式（如天地图 DataServer?x=&y=&l=）
    tile_url_to_template_query(raw)
}

/// 将查询参数形式的瓦片 URL 转换为 `{z}/{x}/{y}` 模板
///
/// 例：`https://t6.tianditu.gov.cn/DataServer?T=img_w&x=849&y=419&l=10&tk=abc`
///   → `https://t6.tianditu.gov.cn/DataServer?T=img_w&x={x}&y={y}&l={z}&tk=abc`
fn tile_url_to_template_query(raw: &str) -> Option<String> {
    let q_pos = raw.find('?')?;
    let base = &raw[..q_pos];
    let query_str = &raw[q_pos + 1..];

    // 解析查询参数列表
    let params: Vec<(&str, &str)> = query_str
        .split('&')
        .filter_map(|kv| {
            let mut it = kv.splitn(2, '=');
            let k = it.next()?.trim();
            let v = it.next().unwrap_or("").trim();
            if k.is_empty() { None } else { Some((k, v)) }
        })
        .collect();

    // 找到对应 z/x/y 的参数名
    let (z_key, x_key, y_key) = find_zxy_params(&params)?;

    // 重建查询字符串，替换坐标值为占位符
    let new_query = params
        .iter()
        .map(|(k, v)| {
            if *k == z_key {
                format!("{}={{z}}", k)
            } else if *k == x_key {
                format!("{}={{x}}", k)
            } else if *k == y_key {
                format!("{}={{y}}", k)
            } else {
                format!("{}={}", k, v)
            }
        })
        .collect::<Vec<_>>()
        .join("&");

    Some(format!("{}?{}", base, new_query))
}

/// 在查询参数列表中识别 z/x/y 坐标参数，返回 (z_key, x_key, y_key)
fn find_zxy_params<'a>(
    params: &[(&'a str, &'a str)],
) -> Option<(&'a str, &'a str, &'a str)> {
    // 各坐标常见参数名（大小写均支持）
    const Z_NAMES: &[&str] = &["z", "l", "level", "zoom", "tilematrix", "lev", "zoomlevel"];
    const X_NAMES: &[&str] = &["x", "col", "tilecol", "tilecolumn", "column"];
    const Y_NAMES: &[&str] = &["y", "row", "tilerow"];

    // 在参数列表中按名称查找，要求值为整数
    let find_int = |names: &[&str]| -> Option<(&'a str, u64)> {
        for name in names {
            if let Some(&(k, v)) = params
                .iter()
                .find(|(k, _)| k.to_lowercase() == *name)
            {
                if let Ok(n) = v.parse::<u64>() {
                    return Some((k, n));
                }
            }
        }
        None
    };

    let (z_key, z_val) = find_int(Z_NAMES)?;
    let (x_key, _)     = find_int(X_NAMES)?;
    let (y_key, _)     = find_int(Y_NAMES)?;

    // z 值需在合理范围内
    if z_val > 24 { return None; }

    // 三个参数不能相同
    if z_key == x_key || z_key == y_key || x_key == y_key {
        return None;
    }

    Some((z_key, x_key, y_key))
}

/// 从原始瓦片 URL 构建 TileSource（含模板转换）
pub fn tile_url_to_source(raw: &str) -> Option<TileSource> {
    let template = tile_url_to_template(raw)?;
    Some(TileSource {
        kind: SourceKind::WebCapture,
        name: guess_name(&template),
        url_template: template,
        crs: guess_crs(raw),
        tile_size: 256,
        north_to_south: true,
        ..Default::default()
    })
}

/// 从 URL 猜测图层名称（取二级域名）
pub fn guess_name(url: &str) -> String {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);

    let domain = rest.split('/').next().unwrap_or("");
    let domain = domain.split(':').next().unwrap_or(domain);
    let domain = domain.strip_prefix("www.").unwrap_or(domain);

    let parts: Vec<&str> = domain.split('.').collect();
    let base = if parts.len() >= 2 {
        parts[parts.len() - 2]
    } else {
        domain
    };

    format!("{} 网页抓取", base)
}

/// 从 URL 特征猜测坐标系
pub fn guess_crs(url: &str) -> CrsType {
    let lower = url.to_lowercase();
    // 天地图 _c 后缀 = WGS84 经纬度（如 T=img_c、T=vec_c）
    if lower.contains("_c&")
        || lower.contains("_c=")
        || lower.ends_with("_c")
        || lower.contains("%3d")     // URL 编码的 =
            && lower.contains("_c")
    {
        return CrsType::Wgs84;
    }
    if lower.contains("4326")
        || lower.contains("wgs84")
        || lower.contains("crs84")
        || lower.contains("epsg:4326")
    {
        CrsType::Wgs84
    } else {
        CrsType::WebMercator
    }
}

// ─── 测试 ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_xyz_path() {
        let tmpl = tile_url_to_template("https://tile.example.com/15/12345/6789.png").unwrap();
        assert_eq!(tmpl, "https://tile.example.com/{z}/{x}/{y}.png");
    }

    #[test]
    fn with_path_prefix() {
        let tmpl =
            tile_url_to_template("https://t0.example.com/maps/layer/14/8192/4096.png").unwrap();
        assert_eq!(tmpl, "https://t0.example.com/maps/layer/{z}/{x}/{y}.png");
    }

    #[test]
    fn no_extension() {
        let tmpl = tile_url_to_template("https://tile.example.com/10/512/256").unwrap();
        assert_eq!(tmpl, "https://tile.example.com/{z}/{x}/{y}");
    }

    #[test]
    fn z_too_large_skipped() {
        // z=100, 不是有效缩放级别
        assert!(tile_url_to_template("https://example.com/100/200/300.png").is_none());
    }

    #[test]
    fn query_param_tianditu() {
        let url = "https://t6.tianditu.gov.cn/DataServer?T=img_w&x=849&y=419&l=10&tk=abc123";
        let tmpl = tile_url_to_template(url).unwrap();
        assert_eq!(
            tmpl,
            "https://t6.tianditu.gov.cn/DataServer?T=img_w&x={x}&y={y}&l={z}&tk=abc123"
        );
    }

    #[test]
    fn query_param_xyz() {
        let url = "https://tile.example.com/service?z=8&x=120&y=60";
        let tmpl = tile_url_to_template(url).unwrap();
        assert_eq!(tmpl, "https://tile.example.com/service?z={z}&x={x}&y={y}");
    }
}
