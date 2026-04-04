//! WMTS / TMS / XYZ URL 解析器
//!
//! 支持两种模式：
//! 1. WMTS GetCapabilities XML — 从 URL 获取并解析服务元数据
//! 2. TMS/XYZ URL 模板 — 直接从模板字符串构建 TileSource
//!    例如: `https://tile.openstreetmap.org/{z}/{x}/{y}.png`

use anyhow::{anyhow, Result};
use quick_xml::events::Event;
use quick_xml::Reader;
use std::collections::HashMap;

use crate::types::{Bounds, CrsType, SourceKind, TileSource};

// ─── TMS/XYZ 解析 ────────────────────────────────────────────────────────────

/// 从 TMS/XYZ URL 模板构建 TileSource（无网络请求）
/// 支持格式：
/// - `https://tile.osm.org/{z}/{x}/{y}.png`
/// - `https://t{s}.tianditu.gov.cn/img_w/wmts?...TILEMATRIX={z}&TILEROW={y}&TILECOL={x}`
pub fn parse_tms_url(url: &str, name: Option<&str>) -> Result<TileSource> {
    let url = url.trim().to_string();

    if url.is_empty() {
        return Err(anyhow!("URL 不能为空"));
    }

    // 检测 URL 中是否有标准占位符
    let has_xyz = url.contains("{z}") && url.contains("{x}") && url.contains("{y}");
    let has_d = url.contains("%d");

    if !has_xyz && !has_d {
        // 可能是 WMTS 服务 URL，尝试自动识别
        if url.to_lowercase().contains("wmts")
            || url.to_lowercase().contains("service=wmts")
        {
            return Err(anyhow!(
                "看起来是 WMTS 服务 URL，请使用 WMTS 解析器或手动添加 {{z}}/{{x}}/{{y}} 占位符"
            ));
        }
    }

    // 检测图像格式
    let format = detect_url_format(&url).to_string();

    // 提取子域名模板（如 {s} 或硬编码的 t0-t7）
    let subdomains = extract_subdomains(&url);

    // 推断 CRS（天地图经纬度服务有特殊标识）
    let crs = if url.contains("_c/") || url.contains("lat_lon") || url.contains("lnglat") {
        CrsType::Wgs84
    } else if url.contains("terrain") || url.contains("dem") {
        CrsType::Terrain
    } else {
        CrsType::WebMercator
    };

    let display_name = name
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| extract_name_from_url(&url))
        .to_string();

    Ok(TileSource {
        kind: SourceKind::Tms,
        name: display_name,
        url_template: url,
        subdomains,
        crs,
        format,
        ..Default::default()
    })
}

// ─── WMTS GetCapabilities 解析 ───────────────────────────────────────────────

/// WMTS 图层描述（从 GetCapabilities 解析）
#[derive(Debug, Clone)]
pub struct WmtsLayer {
    pub identifier: String,
    pub title: String,
    pub formats: Vec<String>,
    pub tile_matrix_sets: Vec<String>,
    pub resource_url: Option<String>,
    pub bounds: Option<Bounds>,
}

/// 从 GetCapabilities XML 字符串解析所有图层
pub fn parse_wmts_capabilities(xml: &str) -> Result<Vec<WmtsLayer>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut layers: Vec<WmtsLayer> = Vec::new();
    let mut current_layer: Option<WmtsLayer> = None;
    let mut path: Vec<String> = Vec::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let tag = String::from_utf8_lossy(e.name().local_name().as_ref()).to_string();

                match tag.as_str() {
                    "Layer" => {
                        let depth = path.iter().filter(|t| t.as_str() == "Layer").count();
                        if depth == 0 {
                            // 顶层 Layer 元素
                            current_layer = Some(WmtsLayer {
                                identifier: String::new(),
                                title: String::new(),
                                formats: Vec::new(),
                                tile_matrix_sets: Vec::new(),
                                resource_url: None,
                                bounds: None,
                            });
                        }
                    }
                    "ResourceURL" => {
                        if let Some(layer) = &mut current_layer {
                            let attrs = parse_wmts_attrs(&e);
                            // 只取 tile 类型的 ResourceURL
                            if attrs.get("resourceType").map(|s| s == "tile").unwrap_or(false) {
                                if let Some(tpl) = attrs.get("template") {
                                    // 转换 {TileMatrix}/{TileRow}/{TileCol} → {z}/{y}/{x}
                                    let url = normalize_wmts_url(tpl);
                                    layer.resource_url = Some(url);
                                }
                            }
                        }
                    }
                    "WGS84BoundingBox" => {
                        // 解析在下面的 Text 事件中
                    }
                    _ => {}
                }
                path.push(tag);
            }
            Ok(Event::End(e)) => {
                let tag = String::from_utf8_lossy(e.name().local_name().as_ref()).to_string();
                if tag == "Layer" {
                    if let Some(layer) = current_layer.take() {
                        if !layer.identifier.is_empty() {
                            layers.push(layer);
                        }
                    }
                }
                path.pop();
            }
            Ok(Event::Text(e)) => {
                let text = e.unescape().unwrap_or_default().trim().to_string();
                if text.is_empty() {
                    buf.clear();
                    continue;
                }

                let current_tag = path.last().cloned().unwrap_or_default();

                if let Some(layer) = &mut current_layer {
                    match current_tag.as_str() {
                        "Identifier" | "ows:Identifier" => {
                            if path.iter().rev().nth(1).map(|s| s.as_str()) == Some("Layer") {
                                layer.identifier = text;
                            }
                        }
                        "Title" | "ows:Title" => {
                            if layer.title.is_empty() {
                                layer.title = text;
                            }
                        }
                        "Format" => {
                            layer.formats.push(text);
                        }
                        "TileMatrixSet" => {
                            // 跳过 TileMatrixSetLink 内的 TileMatrixSet
                            let in_link = path
                                .iter()
                                .rev()
                                .skip(1)
                                .next()
                                .map(|s| s.contains("Link"))
                                .unwrap_or(false);
                            if in_link || path.contains(&"TileMatrixSetLink".to_string()) {
                                layer.tile_matrix_sets.push(text);
                            }
                        }
                        _ => {}
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(anyhow!("WMTS XML 解析错误: {}", e)),
            _ => {}
        }
        buf.clear();
    }

    Ok(layers)
}

/// 将 WMTS 图层 + TileMatrixSet 构建为 TileSource
pub fn wmts_layer_to_source(
    layer: &WmtsLayer,
    capabilities_url: &str,
) -> Option<TileSource> {
    let url = if let Some(resource) = &layer.resource_url {
        resource.clone()
    } else {
        // 构建 KVP 风格 URL
        build_wmts_kvp_url(capabilities_url, &layer.identifier, layer)
    };

    let format = layer
        .formats
        .first()
        .map(|f| detect_url_format(f).to_string())
        .unwrap_or_else(|| "png".to_string());

    let crs = layer
        .tile_matrix_sets
        .iter()
        .find_map(|tms| {
            if tms.contains("GoogleMapsCompatible") || tms.contains("EPSG:3857") || tms == "w" {
                Some(CrsType::WebMercator)
            } else if tms.contains("EPSG:4326") || tms == "c" {
                Some(CrsType::Wgs84)
            } else {
                None
            }
        })
        .unwrap_or(CrsType::WebMercator);

    Some(TileSource {
        kind: SourceKind::Wmts,
        name: if layer.title.is_empty() {
            layer.identifier.clone()
        } else {
            layer.title.clone()
        },
        url_template: url,
        crs,
        format,
        bounds: layer.bounds.clone().unwrap_or_default(),
        ..Default::default()
    })
}

// ─── 辅助函数 ────────────────────────────────────────────────────────────────

fn parse_wmts_attrs(
    e: &quick_xml::events::BytesStart,
) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for attr in e.attributes().flatten() {
        let key = String::from_utf8_lossy(attr.key.local_name().as_ref()).to_string();
        let val = attr.unescape_value().unwrap_or_default().to_string();
        map.insert(key, val);
    }
    map
}

/// 将 WMTS ResourceURL 模板转化为标准占位符格式
fn normalize_wmts_url(url: &str) -> String {
    url.replace("{TileMatrix}", "{z}")
        .replace("{TileRow}", "{y}")
        .replace("{TileCol}", "{x}")
        .replace("{Style}", "default")
}

/// 构建 KVP 风格的 WMTS URL（无 RESTful ResourceURL 时）
fn build_wmts_kvp_url(base_url: &str, layer_id: &str, layer: &WmtsLayer) -> String {
    // 去掉原有 GetCapabilities 参数
    let base = if let Some(idx) = base_url.find('?') {
        &base_url[..idx]
    } else {
        base_url
    };

    let tile_matrix_set = layer
        .tile_matrix_sets
        .first()
        .cloned()
        .unwrap_or_else(|| "GoogleMapsCompatible".to_string());

    let format = layer.formats.first().cloned().unwrap_or_else(|| "image/png".to_string());

    format!(
        "{}?SERVICE=WMTS&REQUEST=GetTile&VERSION=1.0.0\
         &LAYER={layer_id}&STYLE=default&TILEMATRIXSET={tile_matrix_set}\
         &FORMAT={format}&TILEMATRIX={{z}}&TILEROW={{y}}&TILECOL={{x}}",
        base,
        layer_id = layer_id,
        tile_matrix_set = tile_matrix_set,
        format = format,
    )
}

fn detect_url_format(s: &str) -> &str {
    let lower = s.to_lowercase();
    if lower.contains("jpg") || lower.contains("jpeg") {
        "jpg"
    } else if lower.contains("webp") {
        "webp"
    } else if lower.contains("terrain") {
        "terrain"
    } else {
        "png"
    }
}

fn extract_subdomains(url: &str) -> Vec<String> {
    // 检测 {s} 或 abc 样式子域名
    if url.contains("{s}") {
        return vec!["a".into(), "b".into(), "c".into()];
    }
    Vec::new()
}

fn extract_name_from_url(url: &str) -> &str {
    // 从 URL 提取有意义的名称（domain 部分）
    if let Some(start) = url.find("://") {
        let rest = &url[start + 3..];
        let end = rest.find('/').unwrap_or(rest.len());
        &rest[..end]
    } else {
        "自定义图层"
    }
}

// ─── 测试 ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_tms_url() {
        let src =
            parse_tms_url("https://tile.openstreetmap.org/{z}/{x}/{y}.png", Some("OSM")).unwrap();
        assert_eq!(src.name, "OSM");
        assert_eq!(src.format, "png");
    }

    #[test]
    fn test_normalize_wmts_url() {
        let url = "https://example.com/wmts/{Layer}/{TileMatrix}/{TileRow}/{TileCol}";
        let normalized = normalize_wmts_url(url);
        assert!(normalized.contains("{z}"));
        assert!(normalized.contains("{y}"));
        assert!(normalized.contains("{x}"));
    }
}
