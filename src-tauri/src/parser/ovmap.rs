//! .ovmap 文件解析器
//!
//! .ovmap (OviO Map) 是 LocaSpaceViewer / TXCADPlugin 使用的二进制瓦片源描述格式。
//!
//! ## 文件结构
//!
//! ```text
//! ┌─────────────────────────────── 外层包装头 (24 字节) ───────────────────────────────┐
//! │ Bytes  0- 3: 魔数 "OviO"                                                          │
//! │ Bytes  4- 7: u32 LE = 文件大小                                                     │
//! │ Bytes  8-11: u32 LE = zlib 解压后数据大小                                          │
//! │ Bytes 12-23: 保留字段                                                              │
//! └────────────────────────────────────────────────────────────────────────────────────┘
//! Bytes 24..: zlib 压缩数据 (魔数 0x78 0x01 / 0x78 0x9C / 0x78 0xDA)
//! ```
//!
//! ## 解压后数据结构
//!
//! ```text
//! Offset  0: u32 LE — 未知（与外层头 field16 一致）
//! Offset  4: u32 LE — 未知
//! Offset  8: u64    — 通常为 0
//! Offset 16: u32 LE — 未知（DPI 或其他配置）
//! Offset 20: u32 LE — 协议标志（0=HTTP, 1=HTTPS）
//! Offset 24: u32 LE — 最大缩放级别 (max_zoom)
//! Offset 28: u32 LE — 最小缩放级别 (min_zoom)
//! Offset 36: ...    — 混合标志位
//! Offset 40: u32 LE — 瓦片尺寸（像素，0 时默认 256）
//! Offset 44...: 保留/零填充
//! 字符串区（从偏移 ~120 开始，通过扫描定位）：
//!   u32 LE — serverparts 字符串字节长度
//!   N 字节  — serverparts ASCII 字符，每个字符为一个服务器编号
//!   u32 LE — 主机模板字节长度
//!   N 字节  — 主机模板（可含 {$serverpart} 占位符）
//!   u32 LE — 路径模板字节长度
//!   N 字节  — 路径模板（含 {$x} {$y} {$z} 占位符）
//! ```

use std::io::Read;
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use flate2::read::ZlibDecoder;

use crate::types::{CoordType, CrsType, SourceKind, TileSource};

const MAGIC: &[u8; 4] = b"OviO";
/// 外层包装头大小（字节），之后紧跟 zlib 压缩数据
const OUTER_HEADER_SIZE: usize = 24;
/// 扫描字符串区时跳过的二进制头最小长度
const SCAN_START: usize = 36;

// ─── 公开 API ────────────────────────────────────────────────────────────────

/// 从文件路径解析 .ovmap 文件
pub fn parse_ovmap_file(path: &Path) -> Result<TileSource> {
    let raw = std::fs::read(path)
        .with_context(|| format!("无法读取 ovmap 文件: {}", path.display()))?;

    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("未知图层");

    parse_ovmap_bytes(&raw, name)
}

/// 从字节切片解析 .ovmap 内容
pub fn parse_ovmap_bytes(raw: &[u8], default_name: &str) -> Result<TileSource> {
    // ── 1. 验证魔数 ──────────────────────────────────────────────────────────
    if raw.len() < OUTER_HEADER_SIZE + 2 {
        bail!("ovmap 文件太短（{} 字节）", raw.len());
    }
    if &raw[..4] != MAGIC {
        bail!(
            "非 ovmap 格式（魔数不匹配，前4字节: {:02X?}）",
            &raw[..4]
        );
    }

    // ── 2. 解压 zlib 数据 ────────────────────────────────────────────────────
    let compressed = &raw[OUTER_HEADER_SIZE..];
    let dc = decompress_zlib(compressed)
        .with_context(|| "ovmap zlib 解压失败")?;

    if dc.len() < 48 {
        bail!("ovmap 解压数据过短（{} 字节）", dc.len());
    }

    // ── 3. 读取二进制头中的已知字段 ──────────────────────────────────────────
    let max_zoom = (read_u32_le(&dc, 24).min(22)) as u8;
    let min_zoom_raw = read_u32_le(&dc, 28) as u8;
    let min_zoom = min_zoom_raw.min(max_zoom);

    let tile_size = {
        let ts = read_u32_le(&dc, 40);
        if ts == 0 { 256 } else { ts }
    };

    // 协议标志：0=HTTP，非0=HTTPS
    let use_https = read_u32_le(&dc, 20) != 0;
    let scheme = if use_https { "https" } else { "http" };

    // ── 4. 扫描并解析字符串区 ────────────────────────────────────────────────
    let (serverparts_str, host_tmpl, path_tmpl) = find_strings_section(&dc)
        .with_context(|| "ovmap: 无法在解压数据中找到有效的 URL 字符串区")?;

    // ── 5. 转换 LocaSpaceViewer 占位符为标准格式 ─────────────────────────────
    // {$serverpart} → {s}，{$x}/{$y}/{$z} → {x}/{y}/{z}
    let host_converted = convert_placeholders(&host_tmpl);
    let path_converted = convert_placeholders(&path_tmpl);

    // ── 6. 构建子域名列表 ─────────────────────────────────────────────────────
    // serverparts_str 中每个字符代表一个子域名编号，例如 "123" → ["1","2","3"]
    let subdomains: Vec<String> = serverparts_str
        .chars()
        .map(|c| c.to_string())
        .collect();

    // ── 7. 拼装完整 URL 模板 ──────────────────────────────────────────────────
    let url_template = format!("{}://{}{}", scheme, host_converted, path_converted);

    // ── 8. 推断坐标系与坐标偏移类型 ──────────────────────────────────────────
    let (crs, coord_type) = detect_coord_system(&host_tmpl, &path_tmpl);

    Ok(TileSource {
        kind: SourceKind::Ovmap,
        name: default_name.to_string(),
        url_template,
        subdomains,
        crs,
        coord_type,
        tile_size,
        north_to_south: true,
        min_zoom,
        max_zoom,
        ..Default::default()
    })
}

// ─── 内部实现 ────────────────────────────────────────────────────────────────

/// zlib 解压（兼容 0x78 0x01 / 0x78 0x9C / 0x78 0xDA 三种压缩级别）
fn decompress_zlib(data: &[u8]) -> Result<Vec<u8>> {
    if data.len() < 2 {
        bail!("压缩数据太短");
    }
    if data[0] != 0x78 {
        bail!("非 zlib 压缩格式（首字节: 0x{:02X}）", data[0]);
    }
    let mut decoder = ZlibDecoder::new(data);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out).context("zlib 解压错误")?;
    Ok(out)
}

/// 从解压数据中扫描识别字符串区，返回 (serverparts, host_template, path_template)
///
/// 字符串区结构（每个字段均为长度前缀的字节串）：
/// ```text
/// [u32 sp_len] [sp_len bytes: serverpart chars] 
/// [u32 host_len] [host_len bytes: host template]
/// [u32 path_len] [path_len bytes: path template]
/// ```
fn find_strings_section(dc: &[u8]) -> Result<(String, String, String)> {
    let len = dc.len();

    let mut offset = SCAN_START;
    while offset + 4 <= len {
        let sp_len = read_u32_le(dc, offset) as usize;

        // serverparts 通常 1-64 字节（对应最多 64 个子域名）
        if sp_len == 0 || sp_len > 64 {
            offset += 1;
            continue;
        }

        let sp_end = offset + 4 + sp_len;
        if sp_end + 4 > len {
            break;
        }

        let sp_bytes = &dc[offset + 4..sp_end];
        // serverpart 字符必须为 ASCII 字母数字
        if !sp_bytes.iter().all(|b| b.is_ascii_alphanumeric()) {
            offset += 1;
            continue;
        }

        let host_len = read_u32_le(dc, sp_end) as usize;
        // 主机模板通常 5-512 字节
        if host_len < 5 || host_len > 512 {
            offset += 1;
            continue;
        }

        let host_end = sp_end + 4 + host_len;
        if host_end + 4 > len {
            offset += 1;
            continue;
        }

        let host_bytes = &dc[sp_end + 4..host_end];
        if !host_bytes.is_ascii() {
            offset += 1;
            continue;
        }
        let host = std::str::from_utf8(host_bytes).unwrap_or("").to_string();
        // 有效的主机名必须包含 '.'
        if !host.contains('.') {
            offset += 1;
            continue;
        }

        let path_len = read_u32_le(dc, host_end) as usize;
        // 路径模板通常 1-1024 字节
        if path_len == 0 || path_len > 1024 {
            offset += 1;
            continue;
        }

        let path_end = host_end + 4 + path_len;
        if path_end > len {
            offset += 1;
            continue;
        }

        let path_bytes = &dc[host_end + 4..path_end];
        if !path_bytes.is_ascii() {
            offset += 1;
            continue;
        }
        let path = std::str::from_utf8(path_bytes).unwrap_or("").to_string();
        // 路径必须以 '/' 开头或包含参数占位符
        if !path.starts_with('/') && !path.contains('{') {
            offset += 1;
            continue;
        }

        let sp = std::str::from_utf8(sp_bytes).unwrap_or("").to_string();
        return Ok((sp, host, path));
    }

    Err(anyhow!(
        "未找到有效字符串区（扫描范围 {}..{} 字节，解压大小 {} 字节）",
        SCAN_START,
        len,
        dc.len()
    ))
}

/// 将 LocaSpaceViewer 占位符转换为标准格式
///
/// `{$serverpart}` → `{s}`  
/// `{$x}` → `{x}`，`{$y}` → `{y}`，`{$z}` → `{z}`
fn convert_placeholders(s: &str) -> String {
    s.replace("{$serverpart}", "{s}")
        .replace("{$x}", "{x}")
        .replace("{$y}", "{y}")
        .replace("{$z}", "{z}")
}

/// 根据主机/路径模板推断坐标系和坐标偏移类型
///
/// - 高德 (autonavi / amap)：WebMercator + GCJ02
/// - 百度 (baidu)：WebMercator + BD09
/// - 其他：WebMercator + WGS84（默认）
fn detect_coord_system(host: &str, _path: &str) -> (CrsType, CoordType) {
    let h = host.to_lowercase();
    if h.contains("autonavi") || h.contains("amap") {
        (CrsType::WebMercator, CoordType::Gcj02)
    } else if h.contains("baidu") {
        (CrsType::WebMercator, CoordType::Bd09)
    } else if h.contains("qq.com") || h.contains("map.qq") {
        (CrsType::WebMercator, CoordType::Gcj02)
    } else {
        (CrsType::WebMercator, CoordType::Wgs84)
    }
}

/// 以小端序读取 4 字节 u32，若越界返回 0
#[inline]
fn read_u32_le(data: &[u8], offset: usize) -> u32 {
    if offset + 4 > data.len() {
        return 0;
    }
    u32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ])
}

// ─── 测试 ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_real_ovmap() {
        let path = std::path::Path::new(
            r"d:\Tools\lsv\LocaSpaceViewer4\Resource\CADPlugin\TXCADPlugin.bundle\Contents\Layer\道路图\001.ovmap",
        );
        if !path.exists() {
            eprintln!("跳过：测试文件不存在");
            return;
        }
        let source = parse_ovmap_file(path).expect("解析 ovmap 失败");
        assert_eq!(source.kind, SourceKind::Ovmap);
        assert_eq!(source.max_zoom, 18);
        assert_eq!(source.tile_size, 256);
        assert!(source.url_template.contains("autonavi.com"), "URL 应包含 autonavi.com");
        assert!(source.url_template.contains("{x}"), "URL 应含 {{x}}");
        assert!(source.url_template.contains("{y}"), "URL 应含 {{y}}");
        assert!(source.url_template.contains("{z}"), "URL 应含 {{z}}");
        assert_eq!(source.coord_type, CoordType::Gcj02, "高德地图应为 GCJ02 坐标");
        assert_eq!(source.subdomains, vec!["1", "2", "3"]);
        println!("url_template: {}", source.url_template);
        println!("subdomains: {:?}", source.subdomains);
    }

    #[test]
    fn test_invalid_magic() {
        // 构造 26+ 字节以通过长度检查，但魔数错误
        let mut bad = Vec::from(&b"NOPE"[..]);
        bad.extend_from_slice(&[0u8; 30]);
        let result = parse_ovmap_bytes(&bad, "test");
        assert!(result.is_err(), "魔数错误时应返回 Err");
    }

    #[test]
    fn test_too_short() {
        let result = parse_ovmap_bytes(b"OviO\x00\x00", "test");
        assert!(result.is_err());
    }

    #[test]
    fn test_convert_placeholders() {
        let host = "webrd0{$serverpart}.is.autonavi.com";
        let path = "/appmaptile?x={$x}&y={$y}&z={$z}";
        assert_eq!(
            convert_placeholders(host),
            "webrd0{s}.is.autonavi.com"
        );
        assert_eq!(
            convert_placeholders(path),
            "/appmaptile?x={x}&y={y}&z={z}"
        );
    }
}
