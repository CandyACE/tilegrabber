//! 轻量 MVT 扫描器：仅提取 layer 名称（用于生成 MBTiles/PMTiles `vector_layers` 元数据）。
//!
//! Mapbox Vector Tile 是 protobuf 编码，根 message `Tile` 中 `repeated Layer layers = 3`，
//! `Layer.name = 1`（string）。我们只需读取这两个 tag，不引入完整 prost/mvt crate。
//!
//! Protobuf wire types:
//!   - varint           = 0
//!   - 64-bit           = 1
//!   - length-delimited = 2  ← layer 子消息、name 字符串都属于此
//!   - 32-bit           = 5

use std::collections::BTreeSet;

#[derive(Debug, Default)]
struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.buf.len().saturating_sub(self.pos)
    }

    fn read_byte(&mut self) -> Option<u8> {
        if self.pos >= self.buf.len() {
            return None;
        }
        let b = self.buf[self.pos];
        self.pos += 1;
        Some(b)
    }

    fn read_varint(&mut self) -> Option<u64> {
        let mut result: u64 = 0;
        let mut shift = 0u32;
        loop {
            let b = self.read_byte()?;
            result |= u64::from(b & 0x7F) << shift;
            if b & 0x80 == 0 {
                return Some(result);
            }
            shift += 7;
            if shift > 63 {
                return None;
            }
        }
    }

    fn skip(&mut self, n: usize) -> Option<()> {
        if self.remaining() < n {
            return None;
        }
        self.pos += n;
        Some(())
    }

    fn read_slice(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.remaining() < n {
            return None;
        }
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Some(s)
    }

    // 跳过任意类型的字段值
    fn skip_field(&mut self, wire: u8) -> Option<()> {
        match wire {
            0 => {
                self.read_varint()?;
            }
            1 => {
                self.skip(8)?;
            }
            2 => {
                let len = self.read_varint()? as usize;
                self.skip(len)?;
            }
            5 => {
                self.skip(4)?;
            }
            _ => return None,
        }
        Some(())
    }
}

/// 从单块 MVT 瓦片字节中提取所有 layer 的名称。
/// 解析失败或字段缺失时返回空集合，调用方应当容忍。
pub fn extract_layer_names(tile_bytes: &[u8]) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut cur = Cursor::new(tile_bytes);

    while cur.remaining() > 0 {
        let Some(key) = cur.read_varint() else { break };
        let tag = (key >> 3) as u32;
        let wire = (key & 0x7) as u8;

        if tag == 3 && wire == 2 {
            // Tile.layers
            let Some(layer_len) = cur.read_varint().map(|n| n as usize) else { break };
            let Some(layer_buf) = cur.read_slice(layer_len) else { break };
            if let Some(name) = extract_layer_name(layer_buf) {
                names.insert(name);
            }
        } else if cur.skip_field(wire).is_none() {
            break;
        }
    }

    names
}

fn extract_layer_name(layer_buf: &[u8]) -> Option<String> {
    let mut cur = Cursor::new(layer_buf);
    while cur.remaining() > 0 {
        let key = cur.read_varint()?;
        let tag = (key >> 3) as u32;
        let wire = (key & 0x7) as u8;
        if tag == 1 && wire == 2 {
            let len = cur.read_varint()? as usize;
            let slice = cur.read_slice(len)?;
            return std::str::from_utf8(slice).ok().map(|s| s.to_string());
        }
        cur.skip_field(wire)?;
    }
    None
}

/// 判断字节流是否已是 gzip 压缩（用于决定是否需要再次 gzip 包装）。
pub fn is_gzipped(data: &[u8]) -> bool {
    data.len() >= 2 && data[0] == 0x1F && data[1] == 0x8B
}

/// 解压（若已 gzip）以便扫描；不是 gzip 则原样返回。
pub fn maybe_gunzip(data: &[u8]) -> std::borrow::Cow<'_, [u8]> {
    if !is_gzipped(data) {
        return std::borrow::Cow::Borrowed(data);
    }
    use flate2::read::GzDecoder;
    use std::io::Read;
    let mut out = Vec::with_capacity(data.len() * 3);
    let mut dec = GzDecoder::new(data);
    if dec.read_to_end(&mut out).is_ok() {
        std::borrow::Cow::Owned(out)
    } else {
        std::borrow::Cow::Borrowed(data)
    }
}

/// 将矢量瓦片 raw protobuf 字节用 gzip 包装（MBTiles/PMTiles 规范要求）。
/// 若输入已经是 gzip，则直接返回原数据，避免双重压缩。
pub fn gzip_wrap(data: &[u8]) -> Vec<u8> {
    if is_gzipped(data) {
        return data.to_vec();
    }
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;
    let mut enc = GzEncoder::new(Vec::with_capacity(data.len()), Compression::default());
    let _ = enc.write_all(data);
    enc.finish().unwrap_or_else(|_| data.to_vec())
}
