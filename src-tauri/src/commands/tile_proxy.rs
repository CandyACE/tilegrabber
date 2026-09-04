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
use crate::download::{throttle, worker};
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
        .brotli(true)
        .deflate(true)
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(20))
        .user_agent(throttle::random_user_agent())
        .redirect(reqwest::redirect::Policy::none());
    if classified.is_loopback {
        tracing::debug!("[fetch_tile] 本地回环地址使用直连模式");
        builder = builder.no_proxy();
    } else if let Some(p) = proxy_url.as_deref() {
        let proxy = reqwest::Proxy::all(p).map_err(|e| format!("代理 URL 格式无效: {e}"))?;
        tracing::debug!("[fetch_tile] 使用用户配置的网络代理");
        builder = builder.proxy(proxy);
    } else {
        tracing::debug!("[fetch_tile] 未配置自定义代理，使用操作系统代理设置");
    }
    let client = builder.build().map_err(|e| {
        tracing::error!(error = %e, "[fetch_tile] HTTP 客户端初始化失败");
        e.to_string()
    })?;

    let mut request = client.get(&url);
    let mut ignored_headers = Vec::new();
    for (key, value) in &headers {
        if key.len() > 128 || value.len() > 16 * 1024 {
            return Err("请求头大小超出限制".into());
        }
        if worker::is_managed_request_header(key) {
            ignored_headers.push(key.as_str());
            continue;
        }
        let name = reqwest::header::HeaderName::from_bytes(key.as_bytes())
            .map_err(|_| format!("请求头名称无效: {key}"))?;
        let header_value = reqwest::header::HeaderValue::from_str(value)
            .map_err(|_| format!("请求头值无效: {key}"))?;
        request = request.header(name, header_value);
    }
    if !ignored_headers.is_empty() {
        tracing::warn!(
            host = classified.url.host_str().unwrap_or_default(),
            headers = ?ignored_headers,
            "[fetch_tile] 已忽略由 HTTP 客户端管理的旧图层请求头"
        );
    }

    let response = request
        .send()
        .await
        .map_err(|e| {
            tracing::warn!(
                host = classified.url.host_str().unwrap_or_default(),
                error = %e,
                "[fetch_tile] 瓦片请求失败"
            );
            format!(
                "请求失败（{}）: {}。若浏览器可访问，请检查系统代理，或在设置中配置自定义代理",
                classified.url.host_str().unwrap_or_default(),
                e
            )
        })?;

    let status = response.status();
    if !status.is_success() {
        tracing::warn!(
            host = classified.url.host_str().unwrap_or_default(),
            status = %status,
            "[fetch_tile] 瓦片服务返回非成功状态"
        );
        return Err(format!(
            "瓦片服务返回 HTTP {}（{}）",
            status,
            classified.url.host_str().unwrap_or_default()
        ));
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
