//! .lrc 文件解析器
//!
//! .lrc (Layer Resource Configuration) 是 GB18030 编码的 XML 文件，
//! 包含瓦片服务 URL 模板、投影、范围、HTTP 请求头等元数据。
//!
//! 示例结构：
//! ```xml
//! <DataDefine>
//!   <NetPath>{$serverpart}%d/%d/%d.png</NetPath>
//!   <GeoGridType>WebMercatorWGS84</GeoGridType>
//!   <UrlParamOrder>X,Y,Z</UrlParamOrder>
//!   <ServerParts>0 1 2 3 4 5 6 7</ServerParts>
//!   <Range West="-180" East="180" South="-90" North="90"
//!           LevelBegin="1" LevelEnd="18"/>
//!   <HttpHeaders><Header Name="Referer" Value="..." /></HttpHeaders>
//! </DataDefine>
//! ```

use std::collections::HashMap;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use encoding_rs::GB18030;
use quick_xml::events::Event;
use quick_xml::Reader;

use crate::types::{Bounds, CoordType, CrsType, SourceKind, TileSource};

/// 从文件路径解析 .lrc 文件
pub fn parse_lrc_file(path: &Path) -> Result<TileSource> {
    let raw = std::fs::read(path)
        .with_context(|| format!("无法读取 lrc 文件: {}", path.display()))?;

    parse_lrc_bytes(&raw, path.file_stem().and_then(|s| s.to_str()).unwrap_or("未知图层"))
}

/// 从字节切片解析 .lrc 内容（支持 GB18030 / UTF-8）
pub fn parse_lrc_bytes(raw: &[u8], default_name: &str) -> Result<TileSource> {
    // GB18030 解码（encoding_rs 自动处理 BOM）
    let (text, _, had_errors) = GB18030.decode(raw);
    if had_errors {
        // 尝试 UTF-8 fallback
        let fallback = std::str::from_utf8(raw)
            .with_context(|| "lrc 文件编码既非 GB18030 也非 UTF-8")?;
        parse_lrc_xml(fallback, default_name)
    } else {
        parse_lrc_xml(&text, default_name)
    }
}

// ─── 内部 XML 解析 ───────────────────────────────────────────────────────────

fn parse_lrc_xml(xml: &str, default_name: &str) -> Result<TileSource> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut source = TileSource {
        kind: SourceKind::Lrc,
        name: default_name.to_string(),
        ..Default::default()
    };

    let mut current_tag = String::new();
    let mut buf = Vec::new();
    let mut in_http_headers = false;
    let mut in_range = false;
    let mut url_script = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                current_tag = tag.clone();

                match tag.as_str() {
                    "Range" => {
                        in_range = true;
                        // 兼容属性格式：<Range West="-180" East="180" .../>
                        let attrs = parse_attrs(&e);
                        if let Some(v) = attrs.get("West").and_then(|s| s.parse().ok()) {
                            source.bounds.west = v;
                        }
                        if let Some(v) = attrs.get("East").and_then(|s| s.parse().ok()) {
                            source.bounds.east = v;
                        }
                        if let Some(v) = attrs.get("South").and_then(|s| s.parse().ok()) {
                            source.bounds.south = v;
                        }
                        if let Some(v) = attrs.get("North").and_then(|s| s.parse().ok()) {
                            source.bounds.north = v;
                        }
                        if let Some(v) = attrs.get("LevelBegin").and_then(|s| s.parse().ok()) {
                            source.min_zoom = v;
                        }
                        if let Some(v) = attrs.get("LevelEnd").and_then(|s| s.parse().ok()) {
                            source.max_zoom = v;
                        }
                    }
                    "Header" => {
                        if in_http_headers {
                            // 兼容属性格式：<Header Name="X" Value="Y" />
                            let attrs = parse_attrs(&e);
                            if let (Some(name), Some(value)) =
                                (attrs.get("Name"), attrs.get("Value"))
                            {
                                source.headers.insert(name.clone(), value.clone());
                            }
                        }
                    }
                    "HttpHeaders" => {
                        in_http_headers = true;
                    }
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                match tag.as_str() {
                    "HttpHeaders" => in_http_headers = false,
                    "Range" => in_range = false,
                    _ => {}
                }
                current_tag.clear();
            }
            Ok(Event::Text(e)) => {
                // unescape() 在遇到裸 & 时会失败（URL 中常见），
                // 失败时回退到原始字节，保留字面量 &
                let text = match e.unescape() {
                    Ok(t) => t.trim().to_string(),
                    Err(_) => String::from_utf8_lossy(e.as_ref()).trim().to_string(),
                };

                if text.is_empty() {
                    buf.clear();
                    continue;
                }

                // 在 <HttpHeaders> 内：标签名即请求头名，文本即值
                // 例如 <Referer>https://...</Referer>
                if in_http_headers && !current_tag.is_empty()
                    && current_tag != "HttpHeaders"
                    && current_tag != "Header"
                {
                    source.headers.insert(current_tag.clone(), text.clone());
                    buf.clear();
                    continue;
                }

                // 在 <Range> 内：子元素格式 <West>-180</West>
                if in_range {
                    match current_tag.as_str() {
                        "West" => { source.bounds.west = text.parse().unwrap_or(source.bounds.west); }
                        "East" => { source.bounds.east = text.parse().unwrap_or(source.bounds.east); }
                        "South" => { source.bounds.south = text.parse().unwrap_or(source.bounds.south); }
                        "North" => { source.bounds.north = text.parse().unwrap_or(source.bounds.north); }
                        "LevelBegin" => { source.min_zoom = text.parse().unwrap_or(source.min_zoom); }
                        "LevelEnd" => { source.max_zoom = text.parse().unwrap_or(source.max_zoom); }
                        _ => {}
                    }
                    buf.clear();
                    continue;
                }

                match current_tag.as_str() {
                    "Name" | "LayerName" => {
                        if source.name == default_name {
                            source.name = text;
                        }
                    }
                    "NetPath" => {
                        source.url_template = text;
                    }
                    "UrlScript" => {
                        url_script.push_str(&text);
                        url_script.push('\n');
                    }
                    "GeoGridType" => {
                        source.crs = parse_geo_grid_type(&text);
                    }
                    "SampleSize" => {
                        source.tile_size = text.parse().unwrap_or(256);
                    }
                    "TileRowDir" => {
                        // NorthToSouth → true（XYZ 标准，y=0 在北方）
                        // SouthToNorth → false（TMS 约定，y=0 在南方）
                        source.north_to_south = text
                            .to_lowercase()
                            .starts_with("north");
                    }
                    "MapSpaceType" => {
                        source.coord_type = parse_map_space_type(&text);
                    }
                    "UrlParamOrder" => {
                        source.url_param_order = text
                            .split(',')
                            .map(|s| s.trim().to_uppercase())
                            .collect();
                    }
                    "ServerParts" => {
                        source.subdomains = text
                            .split_whitespace()
                            .map(|s| s.to_string())
                            .collect();
                    }
                    "DataType" | "Format" | "format" => {
                        source.format = detect_format(&text).to_string();
                    }
                    "attribution" | "Attribution" => {
                        source.attribution = Some(text);
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(anyhow!("XML 解析错误: {}", e)),
            _ => {}
        }
        buf.clear();
    }

    // 若 NetPath 为空，尝试从 UrlScript Lua 脚本提取 URL 模板
    if source.url_template.is_empty() && !url_script.is_empty() {
        if let Some(tmpl) = parse_url_script(url_script.trim()) {
            source.url_template = tmpl;
        }
    }

    // 标准化 URL 模板（%d → {z}/{x}/{y}，{$serverpart} → {s}）
    let url = source.url_template.clone();
    let order = source.url_param_order.clone();
    source.url_template = TileSource::normalize_url(&url, &order);

    // 如果 bounds 未设置则使用默认值
    if !source.bounds.is_valid() {
        source.bounds = Bounds::new(-180.0, 180.0, -85.051129, 85.051129);
    }

    Ok(source)
}

// ─── 辅助函数 ────────────────────────────────────────────────────────────────

/// 解析 XML 属性为 HashMap
fn parse_attrs(e: &quick_xml::events::BytesStart) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for attr in e.attributes().flatten() {
        let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
        let val = attr.unescape_value().unwrap_or_default().to_string();
        map.insert(key, val);
    }
    map
}

/// 将 lrc GeoGridType 字符串映射到 CrsType
fn parse_geo_grid_type(s: &str) -> CrsType {
    match s.trim() {
        "WebMercatorWGS84" | "WebMercator" | "2" => CrsType::WebMercator,
        "TianDiTuLatLon" | "WGS84" | "4326" | "1" => CrsType::Wgs84,
        "5" | "Terrain" | "terrain" => CrsType::Terrain,
        _ => CrsType::Unknown,
    }
}

/// 将 lrc MapSpaceType 字符串映射到 CoordType
fn parse_map_space_type(s: &str) -> CoordType {
    match s.trim() {
        "GCJ02" => CoordType::Gcj02,
        "BD09" | "BD09LL" => CoordType::Bd09,
        _ => CoordType::Wgs84,
    }
}

/// 从 DataType 字符串推断图像格式
fn detect_format(s: &str) -> &str {
    let lower = s.to_lowercase();
    if lower.contains("jpg") || lower.contains("jpeg") {
        "jpg"
    } else if lower.contains("webp") {
        "webp"
    } else if lower.contains("terrain") || lower.contains("tdtt") {
        "terrain"
    } else {
        "png"
    }
}

// ─── UrlScript Lua 脚本解析 ──────────────────────────────────────────────────

/// 将 `<UrlScript>` 内的简单 Lua URL 函数转换为 `{z}/{x}/{y}` 模板。
///
/// 仅处理形如以下的简单结构，不支持条件/循环等复杂语法：
/// ```lua
/// function getTileUrl(zoom, x, y)
///   key = {"TOKEN"}
///   url = "http://host" .. math.random(1,3) .. ".domain/path/"
///   return url .. zoom .. "/" .. x .. "/" .. y .. "?token=" .. key[1]
/// end
/// ```
fn parse_url_script(script: &str) -> Option<String> {
    let mut vars: std::collections::HashMap<String, String> = std::collections::HashMap::new();

    // 提取函数参数名（默认 zoom/x/y）
    let param_names = extract_func_params(script);

    // 逐行收集变量赋值
    for line in script.lines() {
        let line = line.trim();
        if line.is_empty()
            || line.starts_with("function")
            || line.starts_with("end")
            || line.starts_with("return")
            || line.starts_with("--")
            || line.starts_with("if ")
            || line.starts_with("for ")
            || line.starts_with("while ")
        {
            continue;
        }

        let eq_pos = match find_assignment_eq(line) {
            Some(p) => p,
            None => continue,
        };
        let varname = line[..eq_pos]
            .trim()
            .trim_start_matches("local")
            .trim()
            .to_string();
        if varname.is_empty() || varname.contains(' ') {
            continue;
        }
        let rhs = line[eq_pos + 1..].trim();

        // 表格字面量 {"VALUE"} → 提取第一个字符串
        if rhs.starts_with('{') {
            if let Some(s) = extract_first_string_literal(rhs) {
                vars.insert(varname, s);
            }
            continue;
        }

        // 普通字符串连接表达式
        if let Some(val) = eval_lua_concat(rhs, &vars, &param_names) {
            vars.insert(varname, val);
        }
    }

    // 求 return 表达式
    for line in script.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("return") {
            let expr = rest.trim();
            if !expr.is_empty() {
                let result = eval_lua_concat(expr, &vars, &param_names)?;
                if result.contains("://") {
                    return Some(result);
                }
            }
        }
    }

    None
}

/// 从 Lua function 定义中提取参数名列表（小写）。
fn extract_func_params(script: &str) -> Vec<String> {
    for line in script.lines() {
        let line = line.trim();
        if line.starts_with("function") {
            if let (Some(l), Some(r)) = (line.find('('), line.find(')')) {
                let params: Vec<String> = line[l + 1..r]
                    .split(',')
                    .map(|s| s.trim().to_lowercase())
                    .filter(|s| !s.is_empty())
                    .collect();
                if !params.is_empty() {
                    return params;
                }
            }
        }
    }
    vec!["zoom".to_string(), "x".to_string(), "y".to_string()]
}

/// 将参数名映射到瓦片模板占位符（{z}/{x}/{y}）。
/// 按语义名匹配，回退到位置顺序（0=z, 1=x, 2=y）。
fn param_to_tmpl(name: &str, params: &[String]) -> Option<&'static str> {
    let lower = name.to_lowercase();
    if lower == "zoom" || lower == "z" || lower == "level" || lower.contains("zoom") {
        return Some("{z}");
    }
    if lower == "x" || lower.contains("col") {
        return Some("{x}");
    }
    if lower == "y" || lower.contains("row") {
        return Some("{y}");
    }
    // 按位置
    if let Some(idx) = params.iter().position(|p| p.as_str() == name) {
        return match idx {
            0 => Some("{z}"),
            1 => Some("{x}"),
            2 => Some("{y}"),
            _ => None,
        };
    }
    None
}

/// 在行中找到赋值 `=` 的位置（排除 `==` / `~=` / `<=` / `>=`）。
fn find_assignment_eq(line: &str) -> Option<usize> {
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '"' | '\'' => {
                let q = chars[i];
                i += 1;
                while i < chars.len() {
                    if chars[i] == '\\' {
                        i += 2;
                    } else if chars[i] == q {
                        i += 1;
                        break;
                    } else {
                        i += 1;
                    }
                }
            }
            '=' => {
                let prev = if i > 0 { chars[i - 1] } else { ' ' };
                let next = if i + 1 < chars.len() { chars[i + 1] } else { ' ' };
                if next != '=' && prev != '=' && prev != '~' && prev != '<' && prev != '>' {
                    return Some(i);
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    None
}

/// 对 Lua 字符串连接表达式求值，返回拼接结果（None 表示为空）。
/// 支持：字符串字面量、`..` 运算符、`math.random(...)` → `{s}`、
/// 函数参数名→`{z}/{x}/{y}`、变量引用、`var[n]` 下标（取变量值）。
fn eval_lua_concat(
    expr: &str,
    vars: &std::collections::HashMap<String, String>,
    params: &[String],
) -> Option<String> {
    let tokens = tokenize_lua_concat(expr);
    if tokens.is_empty() {
        return None;
    }
    let mut result = String::new();
    for token in &tokens {
        let t = token.trim();
        if t.is_empty() {
            continue;
        }
        // 字符串字面量 "..." 或 '...'
        if t.len() >= 2
            && ((t.starts_with('"') && t.ends_with('"'))
                || (t.starts_with('\'') && t.ends_with('\'')))
        {
            result.push_str(&t[1..t.len() - 1]);
            continue;
        }
        // math.xxx(...)
        if t.starts_with("math.") {
            result.push_str("{s}");
            continue;
        }
        // 整数字面量
        if t.chars().all(|c| c.is_ascii_digit()) {
            result.push_str(t);
            continue;
        }
        // 函数参数 → {z}/{x}/{y}
        if let Some(tmpl) = param_to_tmpl(t, params) {
            result.push_str(tmpl);
            continue;
        }
        // 下标访问 var[n] → 取变量值
        if let Some(bracket) = t.find('[') {
            let varname = &t[..bracket];
            if let Some(val) = vars.get(varname) {
                result.push_str(val);
                continue;
            }
        }
        // 变量引用
        if let Some(val) = vars.get(t) {
            result.push_str(val);
            continue;
        }
        // 无法识别 — 跳过
    }
    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

/// 按 `..` 运算符将 Lua 表达式拆分为词语列表（正确跳过引号内及括号内的 `..`）。
fn tokenize_lua_concat(expr: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = expr.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '"' | '\'' => {
                let q = chars[i];
                current.push(q);
                i += 1;
                while i < chars.len() {
                    let c = chars[i];
                    current.push(c);
                    i += 1;
                    if c == '\\' {
                        if i < chars.len() {
                            current.push(chars[i]);
                            i += 1;
                        }
                    } else if c == q {
                        break;
                    }
                }
            }
            '(' => {
                // 整体收集括号内容，如 math.random(1,3)
                current.push('(');
                i += 1;
                let mut depth = 1usize;
                while i < chars.len() && depth > 0 {
                    let c = chars[i];
                    current.push(c);
                    i += 1;
                    if c == '(' {
                        depth += 1;
                    } else if c == ')' {
                        depth -= 1;
                    }
                }
            }
            '.' => {
                if i + 1 < chars.len() && chars[i + 1] == '.' {
                    if i + 2 < chars.len() && chars[i + 2] == '.' {
                        // `...` varargs — 当普通文本处理
                        current.push_str("...");
                        i += 3;
                    } else {
                        // `..` 运算符 — 提交当前词
                        let tok = current.trim().to_string();
                        if !tok.is_empty() {
                            tokens.push(tok);
                        }
                        current = String::new();
                        i += 2;
                    }
                } else {
                    current.push('.');
                    i += 1;
                }
            }
            _ => {
                current.push(chars[i]);
                i += 1;
            }
        }
    }
    let tok = current.trim().to_string();
    if !tok.is_empty() {
        tokens.push(tok);
    }
    tokens
}

/// 从字符串（可能是 Lua 表格字面量）中提取第一个字符串字面量的内容。
fn extract_first_string_literal(s: &str) -> Option<String> {
    let s = s.trim();
    let start = s
        .chars()
        .enumerate()
        .find(|(_, c)| *c == '"' || *c == '\'')?;
    let quote = start.1;
    let rest = &s[start.0 + 1..];
    let end = rest.find(quote)?;
    Some(rest[..end].to_string())
}

// ─── 测试 ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_LRC: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<DataDefine>
  <Name>天地图影像</Name>
  <NetPath>https://t{$serverpart}.tianditu.gov.cn/img_w/wmts?SERVICE=WMTS&amp;REQUEST=GetTile&amp;VERSION=1.0.0&amp;LAYER=img&amp;STYLE=default&amp;TILEMATRIXSET=w&amp;FORMAT=tiles&amp;TILEMATRIX=%d&amp;TILEROW=%d&amp;TILECOL=%d&amp;tk=75f0434f</NetPath>
  <GeoGridType>WebMercatorWGS84</GeoGridType>
  <SampleSize>256</SampleSize>
  <TileRowDir>NorthToSouth</TileRowDir>
  <UrlParamOrder>Z,Y,X</UrlParamOrder>
  <ServerParts>0 1 2 3 4 5 6 7</ServerParts>
  <DataType>urlformat</DataType>
  <Range West="-180" East="180" South="-90" North="90" LevelBegin="1" LevelEnd="18"/>
  <HttpHeaders>
    <Header Name="Referer" Value="https://www.tianditu.gov.cn/"/>
    <Header Name="User-Agent" Value="Mozilla/5.0"/>
  </HttpHeaders>
  <attribution>天地图影像 GS(2024)0568号</attribution>
</DataDefine>"#;

    #[test]
    fn test_parse_lrc_basic() {
        let src = parse_lrc_bytes(SAMPLE_LRC.as_bytes(), "test").unwrap();

        assert_eq!(src.name, "天地图影像");
        assert_eq!(src.crs, CrsType::WebMercator);
        assert_eq!(src.tile_size, 256);
        assert_eq!(src.north_to_south, true);
        assert_eq!(src.subdomains, vec!["0","1","2","3","4","5","6","7"]);
        assert_eq!(src.min_zoom, 1);
        assert_eq!(src.max_zoom, 18);
        assert!(src.headers.contains_key("Referer"));
        assert!(src.url_template.contains("{z}"));
        assert!(src.url_template.contains("{s}"));
    }

    #[test]
    fn test_normalize_url() {
        let url = "https://t{$serverpart}.example.com/%d/%d/%d.png";
        let order = vec!["Z".to_string(), "Y".to_string(), "X".to_string()];
        let result = TileSource::normalize_url(url, &order);
        assert!(result.contains("{s}"));
        assert!(result.contains("{z}"));
        assert!(result.contains("{y}"));
        assert!(result.contains("{x}"));
    }

    /// 腾讯地图 LRC：SouthToNorth + GCJ02
    const TENCENT_LRC: &str = r#"<?xml version="1.0" encoding="GB18030"?><DataDefine>
<Name>img</Name>
<GeoGridType>WebMercatorWGS84</GeoGridType>
<SampleSize>256</SampleSize>
<DataType>urlformat</DataType>
<MapSpaceType>GCJ02</MapSpaceType>
<UrlParamOrder>x,y,z</UrlParamOrder>
<TileRowDir>SouthToNorth</TileRowDir>
<NetPath>https://rt{$serverpart}.map.gtimg.com/tile?x=%d&amp;y=%d&amp;z=%d&amp;type=vector&amp;styleid=5</NetPath>
<ServerParts>0 1 2 3</ServerParts>
<Range>
<West>-180</West><East>180</East><South>-85</South><North>85</North>
<LevelBegin>3</LevelBegin><LevelEnd>18</LevelEnd>
</Range>
</DataDefine>"#;

    #[test]
    fn test_parse_lrc_tencent_south_to_north_gcj02() {
        let src = parse_lrc_bytes(TENCENT_LRC.as_bytes(), "test").unwrap();

        assert_eq!(src.crs, CrsType::WebMercator);
        // SouthToNorth → north_to_south 应为 false
        assert_eq!(src.north_to_south, false, "SouthToNorth 应解析为 north_to_south=false");
        // MapSpaceType GCJ02 应被识别
        assert_eq!(src.coord_type, CoordType::Gcj02, "MapSpaceType GCJ02 应解析为 CoordType::Gcj02");
        assert_eq!(src.min_zoom, 3);
        assert_eq!(src.max_zoom, 18);
        assert_eq!(src.subdomains, vec!["0", "1", "2", "3"]);
        // URL 应含 {s} {x} {y} {z}
        assert!(src.url_template.contains("{s}"));
        assert!(src.url_template.contains("{x}"));
        assert!(src.url_template.contains("{y}"));
        assert!(src.url_template.contains("{z}"));
    }
}
