//! 解析器模块
//!
//! - `lrc`         — .lrc (GB18030 XML) 文件解析
//! - `lra`         — .lra (zlib/gzip 压缩) 文件解析
//! - `wmts`        — WMTS GetCapabilities + TMS URL 模板解析
//! - `web_capture` — 网页抓取瓦片 URL 识别
//! - `area_file`   — KML / KMZ / GeoJSON 区域文件解析

pub mod area_file;
pub mod lra;
pub mod lrc;
pub mod ovmap;
pub mod web_capture;
pub mod wmts;
