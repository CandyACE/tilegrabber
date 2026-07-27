//! 瓦片代理命令
//!
//! 通过 Rust 后端代理瓦片请求，绕过浏览器对 Referer 等禁止头的限制。

use std::collections::HashMap;
use std::net::IpAddr;
use std::time::Duration;
use futures_util::StreamExt;
use tauri::command;
use tauri::State;
use url::Url;

use crate::commands::settings::get_active_proxy_url;
use crate::storage::app_db::AppDb;

#[derive(Debug)]
pub(crate) struct UrlClass {
    url: Url,
    pub(crate) is_loopback: bool,
}

const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

fn is_private_or_local(addr: IpAddr) -> bool {
    match addr {
        IpAddr::V4(ip) => ip.is_loopback() || ip.is_private() || ip.is_link_local() || ip.is_unspecified(),
        IpAddr::V6(ip) => {
            let segments = ip.segments();
            ip.is_loopback()
                || ip.is_unspecified()
                || (segments[0] & 0xfe00) == 0xfc00
                || (segments[0] & 0xffc0) == 0xfe80
        }
    }
}

/// 校验并分类目标 URL：仅允许 http/https，loopback 判定用于跳过代理。
fn classify_url(raw: &str) -> Result<UrlClass, String> {
    let url = Url::parse(raw).map_err(|e| format!("非法 URL: {e}"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(format!("仅允许 http/https，收到 {}", url.scheme()));
    }
    let host = url.host_str().unwrap_or("");
    if host.is_empty() || url.username() != "" || url.password().is_some() {
        return Err("URL 必须包含主机且不能包含用户凭据".into());
    }
    let literal_ip = url.host().and_then(|host| match host {
        url::Host::Ipv4(ip) => Some(IpAddr::V4(ip)),
        url::Host::Ipv6(ip) => Some(IpAddr::V6(ip)),
        url::Host::Domain(_) => None,
    });
    let literal_is_loopback = literal_ip.is_some_and(|ip| ip.is_loopback());
    if literal_ip.is_some_and(is_private_or_local) && !literal_is_loopback {
        return Err("禁止请求本机或私有网络地址".into());
    }
    let is_loopback = host.eq_ignore_ascii_case("localhost")
        || host == "127.0.0.1"
        || host == "::1"
        || host == "[::1]"
        || host.starts_with("127.");
    Ok(UrlClass {
        url,
        is_loopback,
    })
}

pub(crate) async fn validate_target(raw: &str) -> Result<UrlClass, String> {
    let classified = classify_url(raw)?;
    if !classified.is_loopback {
        let port = classified.url.port_or_known_default().ok_or("URL 缺少有效端口")?;
        let addresses = tokio::net::lookup_host((classified.url.host_str().unwrap(), port))
            .await
            .map_err(|e| format!("无法解析目标主机: {e}"))?;
        if addresses.into_iter().any(|addr| is_private_or_local(addr.ip())) {
            return Err("禁止请求解析到本机或私有网络的地址".into());
        }
    }
    Ok(classified)
}

/// 通过 Rust 后端获取瓦片数据，支持设置任意请求头（包括 Referer 等浏览器禁止头）
#[command]
pub async fn fetch_tile(
    url: String,
    headers: HashMap<String, String>,
    app_db: State<'_, AppDb>,
) -> Result<Vec<u8>, String> {
    let classified = validate_target(&url).await?;
    let proxy_url = get_active_proxy_url(app_db.inner());
    let mut builder = reqwest::Client::builder()
        .gzip(true)
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none());
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
        if key.len() > 128 || value.len() > 16 * 1024 {
            return Err("请求头大小超出限制".into());
        }
        let lower = key.to_ascii_lowercase();
        if matches!(
            lower.as_str(),
            "host" | "content-length" | "transfer-encoding" | "connection"
                | "proxy-authorization" | "proxy-connection"
        ) {
            return Err(format!("不允许设置请求头: {key}"));
        }
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

    if response
        .content_length()
        .is_some_and(|len| len > MAX_RESPONSE_BYTES as u64)
    {
        return Err("瓦片响应超过 16 MiB 限制".into());
    }
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            return Err("瓦片响应超过 16 MiB 限制".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
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

    #[test]
    fn classify_url_rejects_private_ip() {
        assert!(classify_url("http://192.168.1.10/tile.png").is_err());
        assert!(classify_url("http://169.254.169.254/latest/meta-data").is_err());
    }
}
