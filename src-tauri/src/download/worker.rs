//! TileGrabber — 瓦片 HTTP 下载工作单元
//!
//! 负责：
//! - 将 TileSource URL 模板展开为具体请求 URL
//! - 发起 HTTP GET 请求并返回原始字节
//! - 3 次重试（指数退避），识别 429/403 错误

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use reqwest::Client;
use tokio::time::sleep;

use crate::tile_math::TileCoord;
use crate::types::TileSource;

use super::throttle;

// ─── HTTP 客户端 ─────────────────────────────────────────────────────────────

/// 构建带通用 Headers 的 reqwest 客户端
pub fn build_client(extra_headers: &HashMap<String, String>) -> Result<Client> {
    let mut builder = Client::builder()
        .timeout(Duration::from_secs(15))
        .connect_timeout(Duration::from_secs(6))
        .pool_max_idle_per_host(96)
        .pool_idle_timeout(Duration::from_secs(90))
        .tcp_nodelay(true)
        .tcp_keepalive(Duration::from_secs(30))
        .user_agent(throttle::random_user_agent())
        .gzip(true)
        .brotli(true)
        .deflate(true)
        .http2_adaptive_window(true);

    if !extra_headers.is_empty() {
        let mut headers = reqwest::header::HeaderMap::new();
        for (k, v) in extra_headers {
            if let (Ok(name), Ok(value)) = (
                reqwest::header::HeaderName::from_bytes(k.as_bytes()),
                reqwest::header::HeaderValue::from_str(v),
            ) {
                headers.insert(name, value);
            }
        }
        builder = builder.default_headers(headers);
    }

    builder.build().context("failed to build HTTP client")
}

// ─── URL 构造 ────────────────────────────────────────────────────────────────

/// 将 TileSource URL 模板展开为指定瓦片的完整请求 URL
///
/// 支持占位符：`{z}` `{x}` `{y}` `{s}` `{tk}`
pub fn build_tile_url(source: &TileSource, coord: TileCoord) -> String {
    let mut url = source.url_template.clone();

    // 负载均衡子域名（取模轮询）
    let subdomain = if !source.subdomains.is_empty() {
        let idx = ((coord.x as usize).wrapping_add(coord.y as usize)) % source.subdomains.len();
        source.subdomains[idx].as_str()
    } else {
        ""
    };

    url = url.replace("{z}", &coord.z.to_string());
    url = url.replace("{x}", &coord.x.to_string());

    // SouthToNorth (TMS 约定): y=0 在南方，需将 XYZ y 翻转为 TMS y
    let tile_y = if source.north_to_south {
        coord.y
    } else {
        let n = 1u32 << coord.z;
        n.saturating_sub(1).saturating_sub(coord.y)
    };
    url = url.replace("{y}", &tile_y.to_string());
    url = url.replace("{s}", subdomain);

    // 替换前端预计算的额外参数（如 Token、时间戳等）
    for (k, v) in &source.extra_params {
        url = url.replace(&format!("{{{{{}}}}}" , k), v);
    }

    url
}

// ─── 下载 ────────────────────────────────────────────────────────────────────

/// 下载单个瓦片，失败时最多重试 3 次（指数退避）
///
/// 返回空 Vec 表示该瓦片为空（HTTP 404），调用方可选择跳过或存空数据。
pub async fn download_tile(client: &Client, coord: TileCoord, source: &TileSource) -> Result<Vec<u8>> {
    let url = build_tile_url(source, coord);
    let origin = extract_origin(&url);
    const MAX_RETRIES: u32 = 5;

    // 判断 TileSource 是否已提供 Referer，避免下方自动推断值覆盖它
    let source_has_referer = source
        .headers
        .keys()
        .any(|k| k.eq_ignore_ascii_case("referer"));

    for attempt in 0..MAX_RETRIES {
        if attempt > 0 {
            // 退避：1 s → 2 s → 4 s
            sleep(Duration::from_secs(2u64.pow(attempt - 1))).await;
        }

        let mut req = client
            .get(&url)
            .header("Accept", "image/webp,image/apng,image/*,*/*;q=0.8")
            .header("Accept-Language", throttle::random_accept_language())
            .header("Cache-Control", "no-cache")
            .header("Sec-Fetch-Dest", "image")
            .header("Sec-Fetch-Mode", "no-cors")
            .header("Sec-Fetch-Site", "cross-site")
            .header("DNT", "1");

        // 只有 TileSource 未提供 Referer 时才自动推断（避免覆盖 LRC/WMTS 中已配置的正确值）
        if !source_has_referer {
            req = req.header("Referer", &origin);
        }

        let result = req.send().await;

        match result {
            Ok(resp) => {
                let status = resp.status();

                if status == reqwest::StatusCode::TOO_MANY_REQUESTS
                    || status.as_u16() == 418
                {
                    // 429 / 418（天地图反爬）：触发自适应节流，延长等待后重试
                    throttle::ADAPTIVE.report_failure();
                    sleep(Duration::from_secs(8 + attempt as u64 * 10)).await;
                    continue;
                }

                if status.as_u16() == 502
                    || status.as_u16() == 503
                    || status.as_u16() == 504
                {
                    // 502/503/504：服务器临时过载，使用更长的退避重试
                    throttle::ADAPTIVE.report_failure();
                    let wait = 5 + attempt as u64 * 8; // 5s → 13s → 21s
                    sleep(Duration::from_secs(wait)).await;
                    continue;
                }

                if status == reqwest::StatusCode::FORBIDDEN {
                    bail!("HTTP 403 Forbidden: {}", url);
                }

                if status == reqwest::StatusCode::NOT_FOUND {
                    // 图源对空瓦片返回 404，视为空数据
                    return Ok(Vec::new());
                }

                if !status.is_success() {
                    if attempt + 1 < MAX_RETRIES {
                        continue;
                    }
                    bail!("HTTP {}: {}", status, url);
                }

                let bytes = resp
                    .bytes()
                    .await
                    .with_context(|| format!("failed to read tile body: {}", url))?;
                return Ok(bytes.to_vec());
            }
            Err(e) => {
                if attempt + 1 < MAX_RETRIES {
                    continue;
                }
                return Err(anyhow::Error::from(e))
                    .with_context(|| format!("request failed after {} retries: {}", MAX_RETRIES, url));
            }
        }
    }

    bail!("max retries exceeded for {}", url)
}

// ─── 工具函数 ────────────────────────────────────────────────────────────────

/// 从完整 URL 中提取 Origin（scheme + host），用作 Referer
fn extract_origin(url: &str) -> String {
    if let Some(pos) = url.find("://") {
        let rest = &url[pos + 3..];
        let end = rest.find('/').unwrap_or(rest.len());
        format!("{}://{}", &url[..pos], &rest[..end])
    } else {
        url.to_string()
    }
}
