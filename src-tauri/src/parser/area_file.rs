//! 区域文件解析：KML / KMZ / GeoJSON → bounds + polygon
//!
//! 返回第一个多边形的外环坐标及其外包围矩形。
//! KMZ 为 ZIP 压缩的 KML，内部找到第一个 .kml 文件解析。

use anyhow::{Context, Result};

#[derive(Debug, serde::Serialize)]
pub struct ParsedArea {
    pub west: f64,
    pub east: f64,
    pub south: f64,
    pub north: f64,
    /// 第一个多边形外环坐标 `[lng, lat]`（可能为 None，如点/线要素）
    pub polygon: Option<Vec<[f64; 2]>>,
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
            let content =
                std::fs::read_to_string(path).context("读取 KML 文件失败")?;
            parse_kml_str(&content)
        }
        "kmz" => parse_kmz(path),
        "json" | "geojson" => {
            let content =
                std::fs::read_to_string(path).context("读取 GeoJSON 文件失败")?;
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
    let doc = roxmltree::Document::parse(content)
        .context("解析 KML XML 失败")?;

    let mut first_polygon: Option<Vec<[f64; 2]>> = None;
    let mut all_coords: Vec<[f64; 2]> = Vec::new();

    // 遍历所有 <coordinates> 标签
    for node in doc.descendants() {
        if node.is_element() && node.tag_name().name().eq_ignore_ascii_case("coordinates") {
            if let Some(text) = node.text() {
                let ring = parse_kml_coordinates(text);
                if ring.is_empty() {
                    continue;
                }

                // 只保留第一个多边形外环（兄弟名称链路中有 LinearRing 且父级含 outerBoundaryIs）
                if first_polygon.is_none() {
                    // 检查是否在 LinearRing > outerBoundaryIs / Polygon 结构内
                    let in_polygon = is_in_polygon_outer_ring(&node);
                    if in_polygon {
                        first_polygon = Some(ring.clone());
                    } else if first_polygon.is_none() {
                        // 如果找不到明确 Polygon 结构，用第一个 <coordinates> 作为 fallback
                        first_polygon = Some(ring.clone());
                    }
                }
                all_coords.extend_from_slice(&ring);
            }
        }
    }

    if all_coords.is_empty() {
        anyhow::bail!("KML 文件中未找到有效坐标");
    }

    let bounds = coords_to_bounds(&all_coords);
    Ok(ParsedArea {
        west: bounds[0],
        south: bounds[1],
        east: bounds[2],
        north: bounds[3],
        polygon: first_polygon,
    })
}

/// 检查 `<coordinates>` 节点是否位于 Polygon 的 outerBoundaryIs 结构中
fn is_in_polygon_outer_ring(node: &roxmltree::Node) -> bool {
    let mut cur = *node;
    // 向上最多查 6 层祖先
    for _ in 0..6 {
        if let Some(parent) = cur.parent() {
            let name = parent.tag_name().name().to_lowercase();
            if name == "polygon" {
                return true;
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

// ─── GeoJSON 解析 ─────────────────────────────────────────────────────────────

fn parse_geojson_str(content: &str) -> Result<ParsedArea> {
    let v: serde_json::Value =
        serde_json::from_str(content).context("解析 GeoJSON 失败")?;

    let mut first_polygon: Option<Vec<[f64; 2]>> = None;
    let mut all_coords: Vec<[f64; 2]> = Vec::new();

    collect_geojson_coords(&v, &mut first_polygon, &mut all_coords);

    if all_coords.is_empty() {
        anyhow::bail!("GeoJSON 文件中未找到有效坐标");
    }

    let bounds = coords_to_bounds(&all_coords);
    Ok(ParsedArea {
        west: bounds[0],
        south: bounds[1],
        east: bounds[2],
        north: bounds[3],
        polygon: first_polygon,
    })
}

fn collect_geojson_coords(
    v: &serde_json::Value,
    first_poly: &mut Option<Vec<[f64; 2]>>,
    all: &mut Vec<[f64; 2]>,
) {
    let geom_type = v.get("type").and_then(|t| t.as_str()).unwrap_or("");

    match geom_type {
        "FeatureCollection" => {
            if let Some(features) = v.get("features").and_then(|f| f.as_array()) {
                for feature in features {
                    collect_geojson_coords(feature, first_poly, all);
                }
            }
        }
        "Feature" => {
            if let Some(geom) = v.get("geometry") {
                collect_geojson_coords(geom, first_poly, all);
            }
        }
        "Polygon" => {
            if let Some(coords) = v.get("coordinates").and_then(|c| c.as_array()) {
                // 外环（第一个 ring）
                if let Some(outer_ring) = coords.first().and_then(|r| r.as_array()) {
                    let ring = parse_coord_ring(outer_ring);
                    all.extend_from_slice(&ring);
                    // 去掉闭合的最后一个点（与第一个点相同）
                    let poly = if ring.len() > 1 && ring.first() == ring.last() {
                        ring[..ring.len() - 1].to_vec()
                    } else {
                        ring
                    };
                    if first_poly.is_none() {
                        *first_poly = Some(poly);
                    }
                }
            }
        }
        "MultiPolygon" => {
            if let Some(polygons) = v.get("coordinates").and_then(|c| c.as_array()) {
                for poly_coords in polygons {
                    if let Some(rings) = poly_coords.as_array() {
                        if let Some(outer_ring) = rings.first().and_then(|r| r.as_array()) {
                            let ring = parse_coord_ring(outer_ring);
                            all.extend_from_slice(&ring);
                            if first_poly.is_none() {
                                let poly = if ring.len() > 1 && ring.first() == ring.last() {
                                    ring[..ring.len() - 1].to_vec()
                                } else {
                                    ring
                                };
                                *first_poly = Some(poly);
                            }
                        }
                    }
                }
            }
        }
        "GeometryCollection" => {
            if let Some(geoms) = v.get("geometries").and_then(|g| g.as_array()) {
                for geom in geoms {
                    collect_geojson_coords(geom, first_poly, all);
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
        if *lng < west { west = *lng; }
        if *lng > east { east = *lng; }
        if *lat < south { south = *lat; }
        if *lat > north { north = *lat; }
    }
    [west, south, east, north]
}
