//! 瓦片代理命令
//!
//! 通过 Rust 后端代理瓦片请求，绕过浏览器对 Referer 等禁止头的限制。

use std::collections::HashMap;
use tauri::command;
use tauri::State;

use crate::commands::settings::get_active_proxy_url;
use crate::storage::app_db::AppDb;

/// 通过 Rust 后端获取瓦片数据，支持设置任意请求头（包括 Referer 等浏览器禁止头）
#[command]
pub async fn fetch_tile(
    url: String,
    headers: HashMap<String, String>,
    app_db: State<'_, AppDb>,
) -> Result<Vec<u8>, String> {
    let proxy_url = get_active_proxy_url(app_db.inner());
    let mut builder = reqwest::Client::builder().gzip(true);
    let is_local = url.starts_with("http://localhost")
        || url.starts_with("http://127.0.0.1");
    if !is_local {
        if let Some(p) = proxy_url.as_deref() {
            if let Ok(proxy) = reqwest::Proxy::all(p) {
                builder = builder.proxy(proxy);
            }
        }
    }
    let client = builder.build().map_err(|e| e.to_string())?;

    let mut request = client.get(&url);
    for (key, value) in &headers {
        request = request.header(key.as_str(), value.as_str());
    }

    let response = request
        .send()
        .await
        .map_err(|e| format!("请求失败 {}: {}", url, e))?;

    let status = response.status();
    if !status.is_success() {
        return Err(format!("HTTP {} for {}", status, url));
    }

    let bytes = response.bytes().await.map_err(|e| e.to_string())?;
    Ok(bytes.to_vec())
}
