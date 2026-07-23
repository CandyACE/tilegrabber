//! 瓦片代理命令
//!
//! 通过 Rust 后端代理瓦片请求，绕过浏览器对 Referer 等禁止头的限制。

use std::collections::HashMap;
use tauri::command;
use tauri::State;
use url::Url;

use crate::commands::settings::get_active_proxy_url;
use crate::storage::app_db::AppDb;

#[derive(Debug)]
struct UrlClass {
    _url: Url,
    is_loopback: bool,
}

/// 校验并分类目标 URL：仅允许 http/https，loopback 判定用于跳过代理。
fn classify_url(raw: &str) -> Result<UrlClass, String> {
    let url = Url::parse(raw).map_err(|e| format!("非法 URL: {e}"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(format!("仅允许 http/https，收到 {}", url.scheme()));
    }
    let host = url.host_str().unwrap_or("");
    let is_loopback = host.eq_ignore_ascii_case("localhost")
        || host == "127.0.0.1"
        || host == "::1"
        || host == "[::1]"
        || host.starts_with("127.");
    Ok(UrlClass {
        _url: url,
        is_loopback,
    })
}

/// 通过 Rust 后端获取瓦片数据，支持设置任意请求头（包括 Referer 等浏览器禁止头）
#[command]
pub async fn fetch_tile(
    url: String,
    headers: HashMap<String, String>,
    app_db: State<'_, AppDb>,
) -> Result<Vec<u8>, String> {
    let classified = classify_url(&url)?;
    let proxy_url = get_active_proxy_url(app_db.inner());
    let mut builder = reqwest::Client::builder().gzip(true);
    if !classified.is_loopback {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_url_accepts_https_loopback() {
        let c = classify_url("https://127.0.0.1:8080/tile.png").unwrap();
        assert!(c.is_loopback);
    }

    #[test]
    fn classify_url_accepts_localhost() {
        let c = classify_url("http://localhost:4000/foo").unwrap();
        assert!(c.is_loopback);
    }

    #[test]
    fn classify_url_accepts_ipv6_loopback() {
        let c = classify_url("http://[::1]:8080/foo").unwrap();
        assert!(c.is_loopback);
    }

    #[test]
    fn classify_url_rejects_file_scheme() {
        assert!(classify_url("file:///etc/passwd").is_err());
    }

    #[test]
    fn classify_url_rejects_data_scheme() {
        assert!(classify_url("data:text/plain,hello").is_err());
    }

    #[test]
    fn classify_url_non_loopback() {
        let c = classify_url("https://example.com/tile.png").unwrap();
        assert!(!c.is_loopback);
    }
}
