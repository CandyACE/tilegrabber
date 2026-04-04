//! .lra 文件解析器
//!
//! .lra (Layer Resource Archive) 是压缩的图层资源文件。
//! 实测格式为 **zlib**（魔数 0x78 0xDA），解压后含少量二进制前缀头，
//! 随后是 GB18030 编码的 XML（结构与 .lrc 相同）。
//! 同时兼容 gzip 格式（魔数 0x1F 0x8B）。
//!
//! 解析流程：
//! 1. 识别压缩格式（zlib / gzip）
//! 2. 解压得到原始字节
//! 3. 在字节序列中搜索 XML 起始标记（跳过二进制前缀）
//! 4. 将 XML 片段转发给 lrc 解析器

use std::io::Read;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use flate2::read::{GzDecoder, ZlibDecoder};

use crate::types::{SourceKind, TileSource};

/// 从文件路径解析 .lra 文件
pub fn parse_lra_file(path: &Path) -> Result<TileSource> {
    let raw = std::fs::read(path)
        .with_context(|| format!("无法读取 lra 文件: {}", path.display()))?;

    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("未知图层");

    parse_lra_bytes(&raw, name)
}

/// 从字节切片解析 .lra 内容
pub fn parse_lra_bytes(raw: &[u8], default_name: &str) -> Result<TileSource> {
    if raw.len() < 2 {
        return Err(anyhow!("lra 文件太短，无法识别格式"));
    }

    let decompressed = match (raw[0], raw[1]) {
        // zlib: 0x78 + 任意第二字节
        (0x78, _) => {
            let mut decoder = ZlibDecoder::new(raw);
            let mut out = Vec::new();
            decoder
                .read_to_end(&mut out)
                .with_context(|| "lra zlib 解压失败")?;
            out
        }
        // gzip: 0x1F 0x8B
        (0x1f, 0x8b) => {
            let mut decoder = GzDecoder::new(raw);
            let mut out = Vec::new();
            decoder
                .read_to_end(&mut out)
                .with_context(|| "lra gzip 解压失败")?;
            out
        }
        _ => {
            return Err(anyhow!(
                "lra 压缩格式未知（前2字节: {:02X} {:02X}）",
                raw[0],
                raw[1]
            ));
        }
    };

    parse_decompressed(&decompressed, default_name)
}

fn parse_decompressed(data: &[u8], default_name: &str) -> Result<TileSource> {
    // 在字节序列中搜索 XML 起始标记，跳过任意二进制前缀
    if let Some(offset) = find_xml_start(data) {
        let xml_bytes = &data[offset..];
        let mut source = super::lrc::parse_lrc_bytes(xml_bytes, default_name)?;
        source.kind = SourceKind::Lra;
        return Ok(source);
    }

    // 尝试 JSON 格式（3D Tiles 等）
    if let Ok(text) = std::str::from_utf8(data) {
        let trimmed = text.trim_start();
        if trimmed.starts_with('{') {
            return parse_lra_json(trimmed, default_name);
        }
    }

    Err(anyhow!(
        "lra 解压后内容格式未知（前16字节: {:02X?}）",
        &data[..data.len().min(16)]
    ))
}

/// 在字节切片中搜索 XML 起始标记的偏移量
fn find_xml_start(data: &[u8]) -> Option<usize> {
    for pat in [b"<?xml".as_slice(), b"<DataDefine".as_slice()] {
        if let Some(pos) = data.windows(pat.len()).position(|w| w == pat) {
            return Some(pos);
        }
    }
    None
}

/// 解析 JSON 格式的 lra（3D Tiles 或自定义配置）
fn parse_lra_json(json: &str, default_name: &str) -> Result<TileSource> {
    let v: serde_json::Value =
        serde_json::from_str(json).with_context(|| "lra JSON 解析失败")?;

    if let Some(base_uri) = v.get("baseUri").or_else(|| {
        v.get("root")
            .and_then(|r| r.get("content"))
            .and_then(|c| c.get("uri"))
    }) {
        let url = base_uri.as_str().unwrap_or("").to_string();
        let name = v
            .get("extras")
            .and_then(|e| e.get("name"))
            .and_then(|n| n.as_str())
            .unwrap_or(default_name)
            .to_string();
        return Ok(TileSource {
            kind: SourceKind::Lra,
            name,
            url_template: url,
            ..Default::default()
        });
    }

    Err(anyhow!("lra JSON 内容结构未知，无法提取数据源"))
}

// ─── 测试 ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unknown_magic() {
        let result = parse_lra_bytes(b"not compressed", "test");
        assert!(result.is_err());
    }

    #[test]
    fn test_find_xml_start() {
        let data = b"\x00\x00\x00<?xml version";
        assert_eq!(find_xml_start(data), Some(3));

        let data2 = b"\xAA\xBB<DataDefine><N";
        assert_eq!(find_xml_start(data2), Some(2));

        let data3 = b"\x00\x01\x02\x03";
        assert_eq!(find_xml_start(data3), None);
    }
}
