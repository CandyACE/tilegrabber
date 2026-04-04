//! 瓦片代理命令
//!
//! 通过 Rust 后端代理瓦片请求，绕过浏览器对 Referer 等禁止头的限制。

use std::collections::HashMap;
use tauri::command;

/// 通过 Rust 后端获取瓦片数据，支持设置任意请求头（包括 Referer 等浏览器禁止头）
#[command]
pub async fn fetch_tile(
    url: String,
    headers: HashMap<String, String>,
) -> Result<Vec<u8>, String> {
    let client = reqwest::Client::builder()
        .gzip(true)
        .build()
        .map_err(|e| e.to_string())?;

    let mut request = client.get(&url);
    for (key, value) in &headers {
        request = request.header(key.as_str(), value.as_str());
    }

    let response = request.send().await.map_err(|e| {
        format!("请求失败 {}: {}", url, e)
    })?;

    let status = response.status();
    if !status.is_success() {
        return Err(format!("HTTP {} for {}", status, url));
    }

    let bytes = response.bytes().await.map_err(|e| e.to_string())?;
    Ok(bytes.to_vec())
}
