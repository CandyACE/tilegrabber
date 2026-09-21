//! 区域文件解析：KML / KMZ / GeoJSON → bounds + polygons
//!
//! 返回所有多边形的外环坐标及其外包围矩形。
//! KMZ 为 ZIP 压缩的 KML，内部找到第一个 .kml 文件解析。

use anyhow::{Context, Result};

#[derive(Debug, serde::Serialize)]
pub struct ParsedArea {
    pub west: f64,
    pub east: f64,
    pub south: f64,
    pub north: f64,
    /// 所有多边形外环坐标（可能为 None，如点/线要素）
    pub polygons: Option<Vec<Vec<[f64; 2]>>>,
}

// ─── 公开入口 ────────────────────────────────────────────────────────────────

pub fn parse_area_file(path: &std::path::Path) -> Result<ParsedArea> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "kml" => {
            let content = std::fs::read_to_string(path).context("读取 KML 文件失败")?;
            parse_kml_str(&content)
        }
        "kmz" => parse_kmz(path),
        "json" | "geojson" => {
            let content = std::fs::read_to_string(path).context("读取 GeoJSON 文件失败")?;
            parse_geojson_str(&content)
        }
        other => anyhow::bail!("不支持的文件格式: .{}", other),
    }
}

// ─── KMZ（ZIP 内含 KML）────────────────────────────────────────────────────

fn parse_kmz(path: &std::path::Path) -> Result<ParsedArea> {
    let file = std::fs::File::open(path).context("打开 KMZ 文件失败")?;
    let mut archive = zip::ZipArchive::new(file).context("KMZ 解压失败")?;

    // 优先找根目录下的 .kml，其次找任意 .kml
    let kml_index = (0..archive.len())
        .find(|&i| {
            archive
                .by_index(i)
                .map(|f| f.name().to_lowercase().ends_with(".kml"))
                .unwrap_or(false)
        })
        .with_context(|| "KMZ 中未找到 .kml 文件")?;

    let mut kml_entry = archive.by_index(kml_index)?;
    use std::io::Read;
    let mut content = String::new();
    kml_entry
        .read_to_string(&mut content)
        .context("读取 KMZ 内 KML 文件失败")?;

    parse_kml_str(&content)
}

// ─── KML 解析 ────────────────────────────────────────────────────────────────

fn parse_kml_str(content: &str) -> Result<ParsedArea> {
    let doc = roxmltree::Document::parse(content).context("解析 KML XML 失败")?;

    let mut polygons: Vec<Vec<[f64; 2]>> = Vec::new();
    let mut first_fallback_ring: Option<Vec<[f64; 2]>> = None;
    let mut all_coords: Vec<[f64; 2]> = Vec::new();

    // 遍历所有 <coordinates> 标签
    for node in doc.descendants() {
        if node.is_element() && node.tag_name().name().eq_ignore_ascii_case("coordinates") {
            if let Some(text) = node.text() {
                let ring = parse_kml_coordinates(text);
                if ring.is_empty() {
                    continue;
                }

                // 只把 Polygon.outerBoundaryIs 作为下载面；innerBoundaryIs 是洞，不能另算一个面。
                if is_in_polygon_outer_ring(&node) {
                    if let Some(polygon) = normalize_ring(&ring) {
                        polygons.push(polygon);
                    }
                } else if first_fallback_ring.is_none() {
                    // 兼容结构不规范、仅包含 coordinates 的旧 KML。
                    first_fallback_ring = normalize_ring(&ring);
                }
                all_coords.extend_from_slice(&ring);
            }
        }
    }

    if all_coords.is_empty() {
        anyhow::bail!("KML 文件中未找到有效坐标");
    }

    let bounds = coords_to_bounds(&all_coords);
    if polygons.is_empty() {
        if let Some(ring) = first_fallback_ring {
            polygons.push(ring);
        }
    }
    tracing::info!(polygon_count = polygons.len(), "[area_file] KML 区域解析完成");
    Ok(ParsedArea {
        west: bounds[0],
        south: bounds[1],
        east: bounds[2],
        north: bounds[3],
        polygons: (!polygons.is_empty()).then_some(polygons),
    })
}

/// 检查 `<coordinates>` 节点是否位于 Polygon 的 outerBoundaryIs 结构中。
fn is_in_polygon_outer_ring(node: &roxmltree::Node) -> bool {
    let mut cur = *node;
    let mut found_outer_boundary = false;
    // 向上最多查 6 层祖先，遇到 innerBoundaryIs 时立即排除。
    for _ in 0..6 {
        if let Some(parent) = cur.parent() {
            let name = parent.tag_name().name().to_lowercase();
            if name == "innerboundaryis" {
                return false;
            }
            if name == "outerboundaryis" {
                found_outer_boundary = true;
            }
            if name == "polygon" {
                return found_outer_boundary;
            }
            cur = parent;
        } else {
            break;
        }
    }
    false
}

fn parse_kml_coordinates(text: &str) -> Vec<[f64; 2]> {
    text.split_whitespace()
        .filter_map(|token| {
            let parts: Vec<&str> = token.split(',').collect();
            if parts.len() >= 2 {
                let lng = parts[0].parse::<f64>().ok()?;
                let lat = parts[1].parse::<f64>().ok()?;
                Some([lng, lat])
            } else {
                None
            }
        })
        .collect()
}

/// 去掉重复闭合点并拒绝退化坐标环。
fn normalize_ring(ring: &[[f64; 2]]) -> Option<Vec<[f64; 2]>> {
    let mut normalized = ring.to_vec();
    if normalized.len() > 1 && normalized.first() == normalized.last() {
        normalized.pop();
    }
    (normalized.len() >= 3).then_some(normalized)
}

// ─── GeoJSON 解析 ─────────────────────────────────────────────────────────────

fn parse_geojson_str(content: &str) -> Result<ParsedArea> {
    let v: serde_json::Value = serde_json::from_str(content).context("解析 GeoJSON 失败")?;

    let mut polygons: Vec<Vec<[f64; 2]>> = Vec::new();
    let mut all_coords: Vec<[f64; 2]> = Vec::new();

    collect_geojson_coords(&v, &mut polygons, &mut all_coords);

    if all_coords.is_empty() {
        anyhow::bail!("GeoJSON 文件中未找到有效坐标");
    }

    let bounds = coords_to_bounds(&all_coords);
    tracing::info!(polygon_count = polygons.len(), "[area_file] GeoJSON 区域解析完成");
    Ok(ParsedArea {
        west: bounds[0],
        south: bounds[1],
        east: bounds[2],
        north: bounds[3],
        polygons: (!polygons.is_empty()).then_some(polygons),
    })
}

fn collect_geojson_coords(
    v: &serde_json::Value,
    polygons: &mut Vec<Vec<[f64; 2]>>,
    all: &mut Vec<[f64; 2]>,
) {
    let geom_type = v.get("type").and_then(|t| t.as_str()).unwrap_or("");

    match geom_type {
        "FeatureCollection" => {
            if let Some(features) = v.get("features").and_then(|f| f.as_array()) {
                for feature in features {
                    collect_geojson_coords(feature, polygons, all);
                }
            }
        }
        "Feature" => {
            if let Some(geom) = v.get("geometry") {
                collect_geojson_coords(geom, polygons, all);
            }
        }
        "Polygon" => {
            if let Some(coords) = v.get("coordinates").and_then(|c| c.as_array()) {
                // 外环（第一个 ring）
                if let Some(outer_ring) = coords.first().and_then(|r| r.as_array()) {
                    let ring = parse_coord_ring(outer_ring);
                    all.extend_from_slice(&ring);
                    if let Some(polygon) = normalize_ring(&ring) {
                        polygons.push(polygon);
                    }
                }
            }
        }
        "MultiPolygon" => {
            if let Some(polygon_values) = v.get("coordinates").and_then(|c| c.as_array()) {
                for poly_coords in polygon_values {
                    if let Some(rings) = poly_coords.as_array() {
                        if let Some(outer_ring) = rings.first().and_then(|r| r.as_array()) {
                            let ring = parse_coord_ring(outer_ring);
                            all.extend_from_slice(&ring);
                            if let Some(polygon) = normalize_ring(&ring) {
                                polygons.push(polygon);
                            }
                        }
                    }
                }
            }
        }
        "GeometryCollection" => {
            if let Some(geoms) = v.get("geometries").and_then(|g| g.as_array()) {
                for geom in geoms {
                    collect_geojson_coords(geom, polygons, all);
                }
            }
        }
        "Point" => {
            if let Some(coord) = v.get("coordinates") {
                if let Some(p) = parse_geojson_point(coord) {
                    all.push(p);
                }
            }
        }
        "LineString" | "MultiPoint" => {
            if let Some(coords) = v.get("coordinates").and_then(|c| c.as_array()) {
                for c in coords {
                    if let Some(p) = parse_geojson_point(c) {
                        all.push(p);
                    }
                }
            }
        }
        "MultiLineString" => {
            if let Some(lines) = v.get("coordinates").and_then(|c| c.as_array()) {
                for line in lines {
                    if let Some(pts) = line.as_array() {
                        for c in pts {
                            if let Some(p) = parse_geojson_point(c) {
                                all.push(p);
                            }
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

fn parse_coord_ring(arr: &[serde_json::Value]) -> Vec<[f64; 2]> {
    arr.iter().filter_map(|c| parse_geojson_point(c)).collect()
}

fn parse_geojson_point(v: &serde_json::Value) -> Option<[f64; 2]> {
    let arr = v.as_array()?;
    let lng = arr.first()?.as_f64()?;
    let lat = arr.get(1)?.as_f64()?;
    Some([lng, lat])
}

// ─── 通用工具 ────────────────────────────────────────────────────────────────

/// 从坐标列表计算外包矩形：`[west, south, east, north]`
fn coords_to_bounds(coords: &[[f64; 2]]) -> [f64; 4] {
    let mut west = f64::INFINITY;
    let mut east = f64::NEG_INFINITY;
    let mut south = f64::INFINITY;
    let mut north = f64::NEG_INFINITY;
    for [lng, lat] in coords {
        if *lng < west {
            west = *lng;
        }
        if *lng > east {
            east = *lng;
        }
        if *lat < south {
            south = *lat;
        }
        if *lat > north {
            north = *lat;
        }
    }
    [west, south, east, north]
}

#[cfg(test)]
mod tests {
    use super::parse_kml_str;

    #[test]
    fn parses_all_kml_polygon_outer_rings() {
        let kml = r#"
            <kml xmlns="http://www.opengis.net/kml/2.2"><Document>
              <Placemark><Polygon><outerBoundaryIs><LinearRing><coordinates>
                10,10,0 11,10,0 11,11,0 10,10,0
              </coordinates></LinearRing></outerBoundaryIs></Polygon></Placemark>
              <Placemark><Polygon><outerBoundaryIs><LinearRing><coordinates>
                20,20,0 21,20,0 21,21,0 20,20,0
              </coordinates></LinearRing></outerBoundaryIs>
              <innerBoundaryIs><LinearRing><coordinates>
                20.2,20.2,0 20.3,20.2,0 20.3,20.3,0 20.2,20.2,0
              </coordinates></LinearRing></innerBoundaryIs></Polygon></Placemark>
            </Document></kml>
        "#;
        let parsed = parse_kml_str(kml).expect("KML 应能解析");
        let polygons = parsed.polygons.expect("应返回多面");
        assert_eq!(polygons.len(), 2);
        assert_eq!(polygons[0].len(), 3);
        assert_eq!(polygons[1].len(), 3);
        assert_eq!([parsed.west, parsed.south, parsed.east, parsed.north], [10.0, 10.0, 21.0, 21.0]);
    }
}
