//! Tauri 命令：数据源管理
//!
//! 暴露给前端的 invoke 命令：
//! - `parse_source_file`   — 解析 .lrc / .lra 文件
//! - `parse_wmts_url`      — 解析 WMTS GetCapabilities
//! - `parse_tms_url`       — 解析 TMS/XYZ URL 模板
//! - `validate_tile_url`   — 探测单个瓦片 URL 是否可访问

use tauri::command;
use tauri::State;

use crate::commands::settings::get_active_proxy_url;
use crate::parser::{lra, lrc, ovmap, wmts};
use crate::storage::app_db::AppDb;
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
pub async fn parse_wmts_url(
    url: String,
    app_db: State<'_, AppDb>,
) -> Result<Vec<TileSource>, String> {
    // 构建 GetCapabilities 请求 URL
    let caps_url = build_capabilities_url(&url);
    let proxy_url = get_active_proxy_url(app_db.inner());

    // 发起 HTTP 请求
    let mut builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent("TileGrabber/0.1");
    if let Some(p) = proxy_url.as_deref() {
        if let Ok(proxy) = reqwest::Proxy::all(p) {
            builder = builder.proxy(proxy);
        }
    }
    let client = builder
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
pub async fn validate_tile_url(
    url_template: String,
    app_db: State<'_, AppDb>,
) -> Result<bool, String> {
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
    let proxy_url = get_active_proxy_url(app_db.inner());

    let mut builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .user_agent("TileGrabber/0.1");
    if let Some(p) = proxy_url.as_deref() {
        if let Ok(proxy) = reqwest::Proxy::all(p) {
            builder = builder.proxy(proxy);
        }
    }
    let client = builder
        .build()
        .map_err(|e| format!("HTTP 客户端初始化失败: {e}"))?;

    let result = client.head(&test_url).send().await.or_else(|_| {
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
pub async fn parse_area_file(path: String) -> Result<crate::parser::area_file::ParsedArea, String> {
    crate::parser::area_file::parse_area_file(std::path::Path::new(&path))
        .map_err(|e| e.to_string())
}

/// 解析 MBTiles 文件元数据，返回统一 TileSource
///
/// 读取 `metadata` 表中的 name / bounds / minzoom / maxzoom / format。
/// `url_template` 字段用于存储文件路径，供下载引擎使用。
#[command]
pub async fn parse_mbtiles_source(path: String) -> Result<TileSource, String> {
    use crate::types::{Bounds, CrsType, SourceKind};
    tokio::task::spawn_blocking(move || {
        let conn = rusqlite::Connection::open_with_flags(
            &path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .map_err(|e| format!("无法打开 MBTiles 文件: {e}"))?;

        // 读取 metadata 表
        let mut meta = std::collections::HashMap::<String, String>::new();
        {
            let mut stmt = conn
                .prepare("SELECT name, value FROM metadata")
                .map_err(|e| e.to_string())?;
            let rows: Vec<_> = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|e| e.to_string())?
                .filter_map(|r| r.ok())
                .collect();
            for (k, v) in rows {
                meta.insert(k, v);
            }
        }

        let name = meta.get("name").cloned().unwrap_or_else(|| {
            std::path::Path::new(&path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("MBTiles")
                .to_string()
        });

        let (west, south, east, north) = if let Some(s) = meta.get("bounds") {
            let p: Vec<f64> = s.split(',').filter_map(|v| v.trim().parse().ok()).collect();
            if p.len() == 4 {
                (p[0], p[1], p[2], p[3])
            } else {
                (-180.0, -85.0511, 180.0, 85.0511)
            }
        } else {
            (-180.0, -85.0511, 180.0, 85.0511)
        };

        let min_zoom: u8 = meta
            .get("minzoom")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let max_zoom: u8 = meta
            .get("maxzoom")
            .and_then(|s| s.parse().ok())
            .unwrap_or(18);
        let format = meta.get("format").cloned().unwrap_or_else(|| "png".into());

        Ok(TileSource {
            kind: SourceKind::MbtileFile,
            name,
            url_template: path,
            bounds: Bounds {
                west,
                south,
                east,
                north,
            },
            min_zoom,
            max_zoom,
            format,
            north_to_south: true,
            crs: CrsType::WebMercator,
            ..Default::default()
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 从 MBTiles 文件读取单张瓦片（用于向导预览）
///
/// MBTiles 使用 TMS Y 轴（y=0 在南），传入 XYZ y 后自动转换。
#[command]
pub async fn fetch_mbtiles_tile(path: String, z: i64, x: i64, y: i64) -> Result<Vec<u8>, String> {
    tokio::task::spawn_blocking(move || {
        let conn = rusqlite::Connection::open_with_flags(
            &path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .map_err(|e| format!("无法打开文件: {e}"))?;

        let tms_y = (1i64 << z) - 1 - y;
        let data: Vec<u8> = conn
            .query_row(
                "SELECT tile_data FROM tiles WHERE zoom_level=?1 AND tile_column=?2 AND tile_row=?3",
                rusqlite::params![z, x, tms_y],
                |row| row.get(0),
            )
            .map_err(|e| format!("瓦片不存在: {e}"))?;
        Ok(data)
    })
    .await
    .map_err(|e| e.to_string())?
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
