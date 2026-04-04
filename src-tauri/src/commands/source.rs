//! Tauri 命令：数据源管理
//!
//! 暴露给前端的 invoke 命令：
//! - `parse_source_file`   — 解析 .lrc / .lra 文件
//! - `parse_wmts_url`      — 解析 WMTS GetCapabilities
//! - `parse_tms_url`       — 解析 TMS/XYZ URL 模板
//! - `validate_tile_url`   — 探测单个瓦片 URL 是否可访问

use tauri::command;

use crate::parser::{lra, lrc, ovmap, wmts};
use crate::types::TileSource;

/// 解析本地 .lrc 或 .lra 文件
///
/// `path` — 文件绝对路径（由 Tauri dialog 选择后传入）
#[command]
pub async fn parse_source_file(path: String) -> Result<TileSource, String> {
    let p = std::path::Path::new(&path);

    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "lrc" => lrc::parse_lrc_file(p).map_err(|e| e.to_string()),
        "lra" => lra::parse_lra_file(p).map_err(|e| e.to_string()),
        "ovmap" => ovmap::parse_ovmap_file(p).map_err(|e| e.to_string()),
        other => Err(format!("不支持的文件类型: .{other}")),
    }
}

/// 解析 TMS / XYZ URL 模板（无网络请求）
#[command]
pub async fn parse_tms_url(url: String, name: Option<String>) -> Result<TileSource, String> {
    wmts::parse_tms_url(&url, name.as_deref()).map_err(|e| e.to_string())
}

/// 获取 WMTS GetCapabilities 并解析图层列表
///
/// 由于需要网络请求，此命令为 async 且耗时较长。
#[command]
pub async fn parse_wmts_url(url: String) -> Result<Vec<TileSource>, String> {
    // 构建 GetCapabilities 请求 URL
    let caps_url = build_capabilities_url(&url);

    // 发起 HTTP 请求
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent("TileGrabber/0.1")
        .build()
        .map_err(|e| format!("HTTP 客户端初始化失败: {e}"))?;

    let response = client
        .get(&caps_url)
        .send()
        .await
        .map_err(|e| format!("请求 WMTS 服务失败: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("WMTS 服务返回错误状态: {}", response.status()));
    }

    let xml = response
        .text()
        .await
        .map_err(|e| format!("读取响应内容失败: {e}"))?;

    // 解析图层列表
    let layers = wmts::parse_wmts_capabilities(&xml)
        .map_err(|e| format!("解析 WMTS Capabilities 失败: {e}"))?;

    // 将每个图层转换为 TileSource
    let sources: Vec<TileSource> = layers
        .iter()
        .filter_map(|layer| wmts::wmts_layer_to_source(layer, &caps_url))
        .collect();

    if sources.is_empty() {
        return Err("WMTS 服务中没有可用的图层".to_string());
    }

    Ok(sources)
}

/// 验证瓦片 URL 是否可访问（探测 z=0/x=0/y=0 瓦片）
#[command]
pub async fn validate_tile_url(url_template: String) -> Result<bool, String> {
    // 替换占位符为 z=1, x=0, y=0 的实际坐标
    let test_url = url_template
        .replace("{z}", "1")
        .replace("{x}", "0")
        .replace("{y}", "0")
        .replace("{s}", "a");

    // 跳过非 HTTP URL
    if !test_url.starts_with("http://") && !test_url.starts_with("https://") {
        return Ok(false);
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .user_agent("TileGrabber/0.1")
        .build()
        .map_err(|e| format!("HTTP 客户端初始化失败: {e}"))?;

    let result = client
        .head(&test_url)
        .send()
        .await
        .or_else(|_| {
            // HEAD 不可用时降级为 GET
            let _ = &test_url; // 借用检查
            Err(anyhow::anyhow!("HEAD 请求失败"))
        });

    match result {
        Ok(resp) => Ok(resp.status().is_success() || resp.status().as_u16() == 302),
        Err(_) => {
            // 尝试 GET 请求
            let get_resp = client
                .get(&test_url)
                .send()
                .await
                .map_err(|e| format!("验证瓦片 URL 失败: {e}"))?;
            Ok(get_resp.status().is_success())
        }
    }
}

/// 解析区域文件（KML / KMZ / GeoJSON）
///
/// 返回第一个多边形坐标面及其外包围矩形。
/// `polygon` 为 `null` 时表示文件中只有点/线要素，此时只返回 bounds。
#[command]
pub async fn parse_area_file(
    path: String,
) -> Result<crate::parser::area_file::ParsedArea, String> {
    crate::parser::area_file::parse_area_file(std::path::Path::new(&path))
        .map_err(|e| e.to_string())
}

// ─── 辅助函数 ────────────────────────────────────────────────────────────────

/// 确保 URL 包含 GetCapabilities 请求参数
fn build_capabilities_url(url: &str) -> String {
    let lower = url.to_lowercase();

    if lower.contains("request=getcapabilities") {
        return url.to_string();
    }

    let separator = if url.contains('?') { '&' } else { '?' };
    format!(
        "{url}{sep}SERVICE=WMTS&REQUEST=GetCapabilities&VERSION=1.0.0",
        url = url,
        sep = separator
    )
}
