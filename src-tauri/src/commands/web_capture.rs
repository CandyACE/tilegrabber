//! 网页抓取捕获会话命令
//!
//! 工作原理：
//! 1. `open_capture_window` 打开一个新 Tauri WebView 窗口加载目标 URL
//! 2. 向窗口注入 JS，拦截 `Image.src`、`fetch()`、`XMLHttpRequest.open()`
//! 3. 拦截到的瓦片 URL via HTTP POST 发送到本进程监听的临时端口
//! 4. 后端将原始 URL 转换为 `{z}/{x}/{y}` 模板存入共享状态
//! 5. 前端通过 `get_captured_tiles` 轮询结果，展示给用户

use std::sync::Arc;

use axum::extract::State as AxumState;
use axum::http::StatusCode;
use axum::routing::post;
use axum::Router;
use tauri::{command, AppHandle, Manager, State};
use tokio::sync::Mutex;
use tower_http::cors::{Any, CorsLayer};

use crate::parser::web_capture as wc;
use crate::types::TileSource;

// ─── 捕获会话状态 ─────────────────────────────────────────────────────────────

/// Tauri 托管的单例：当前捕获会话
pub struct CaptureSession {
    /// 已捕获的去重瓦片数据源列表
    tiles: Mutex<Vec<TileSource>>,
    /// 临时 HTTP 服务器的关闭发送端
    shutdown_tx: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}

impl CaptureSession {
    pub fn new() -> Self {
        Self {
            tiles: Mutex::new(Vec::new()),
            shutdown_tx: Mutex::new(None),
        }
    }
}

// ─── 本地 HTTP 服务器（供注入 JS 上报瓦片 URL）────────────────────────────────

/// 启动一个随机端口的 axum 服务器，返回监听端口号
async fn start_capture_server(session: Arc<CaptureSession>) -> Result<u16, String> {
    use axum::http::Method;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("无法绑定捕获端口: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("获取端口失败: {e}"))?
        .port();

    // 允许浏览器跨域（HTTP HTTPS 页面 → 本地 127.0.0.1）
    let cors = CorsLayer::new()
        .allow_methods([Method::POST, Method::OPTIONS])
        .allow_headers(Any)
        .allow_origin(Any);

    let app = Router::new()
        .route("/tile", post(handle_tile_report))
        .with_state(session.clone())
        .layer(cors);

    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    *session.shutdown_tx.lock().await = Some(tx);

    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = rx.await;
            })
            .await
            .ok();
    });

    Ok(port)
}

/// 接收注入 JS 上报的原始瓦片 URL（请求体为纯文本 URL）
async fn handle_tile_report(
    AxumState(session): AxumState<Arc<CaptureSession>>,
    body: String,
) -> StatusCode {
    let raw = body.trim().to_string();
    if let Some(source) = wc::tile_url_to_source(&raw) {
        let mut tiles = session.tiles.lock().await;
        // 按模板去重
        if !tiles.iter().any(|t| t.url_template == source.url_template) {
            tiles.push(source);
        }
    }
    StatusCode::OK
}

// ─── Tauri 命令 ──────────────────────────────────────────────────────────────

/// 打开网页抓取窗口
///
/// 1. 清空上次结果  
/// 2. 启动临时 HTTP 服务器  
/// 3. 创建 WebView 窗口并注入拦截脚本  
/// 返回本地服务器端口号（供前端调试或状态判断使用）
#[command]
pub async fn open_capture_window(
    app: AppHandle,
    url: String,
    session: State<'_, Arc<CaptureSession>>,
) -> Result<u16, String> {
    // 清空上次捕获结果
    session.tiles.lock().await.clear();

    // 停止上次服务器（若存在）
    if let Some(sender) = session.shutdown_tx.lock().await.take() {
        let _ = sender.send(());
        tokio::time::sleep(tokio::time::Duration::from_millis(60)).await;
    }

    // 启动新的捕获服务器
    let port = start_capture_server(session.inner().clone()).await?;

    // 注入到页面的拦截脚本
    // 使用 Rust format!，JS 花括号需要 {{ }} 转义，
    // 而 {port} 是 Rust 格式参数
    let inject_js = format!(
        r#"
(() => {{
    'use strict';
    const REPORT_URL = 'http://127.0.0.1:{port}/tile';
    const seen = new Set();

    /** 判断 url 是否符合瓦片 URL（路径 /z/x/y 或查询参数 x=/y=/l= 形式） */
    function isTileUrl(url) {{
        if (!url || typeof url !== 'string') return false;
        if (!url.startsWith('http://') && !url.startsWith('https://')) return false;
        // 路径形式：匹配路径中连续三段数字，其中第一段 <= 24（缩放级别）
        if (/\/(\d{{1,2}})\/(\d+)\/(\d+)(\.[a-zA-Z0-9]+)?(\?|$)/.test(url)) return true;
        // 查询参数形式：同时含有 x/y 坐标参数 + z/l/level/zoom 参数，且值为整数
        // 注意：URLSearchParams.has() 区分大小写，WMTS 规范使用大写参数名（如 TILECOL/TILEROW/TILEMATRIX）
        // 因此先将所有参数键转为小写后再匹配
        try {{
            const qs = new URL(url).searchParams;
            const lqs = new Map();
            qs.forEach((v, k) => lqs.set(k.toLowerCase(), v));
            const hasXY = (lqs.has('x') || lqs.has('col') || lqs.has('tilecol') || lqs.has('tilecolumn'))
                       && (lqs.has('y') || lqs.has('row') || lqs.has('tilerow'));
            const hasZ  = lqs.has('z') || lqs.has('l') || lqs.has('level')
                       || lqs.has('zoom') || lqs.has('tilematrix') || lqs.has('lev');
            if (!hasXY || !hasZ) return false;
            // z/l/tilematrix 值需为有效缩放级别 (0-24)
            const zRaw = lqs.get('z') ?? lqs.get('l') ?? lqs.get('level')
                          ?? lqs.get('zoom') ?? lqs.get('tilematrix') ?? lqs.get('lev') ?? '99';
            const zVal = parseInt(zRaw, 10);
            return !isNaN(zVal) && zVal >= 0 && zVal <= 24;
        }} catch (e) {{ return false; }}
    }}

    /** 上报瓦片 URL（去重，忽略错误） */
    function report(url) {{
        // 去重 key：路径形式取域名+路径，查询参数形式取非坐标参数部分
        let key;
        try {{
            const u = new URL(url);
            const ZXY_PARAMS = new Set(['x','y','z','l','level','zoom','lev','tilematrix',
                                        'col','row','tilecol','tilerow','tilecolumn']);
            const stable = [];
            u.searchParams.forEach((v, k) => {{
                if (!ZXY_PARAMS.has(k.toLowerCase())) stable.push(k + '=' + v);
            }});
            key = u.origin + u.pathname + (stable.length ? '?' + stable.join('&') : '');
        }} catch(e) {{
            key = url.split('?')[0];
        }}
        if (seen.has(key)) return;
        seen.add(key);
        fetch(REPORT_URL, {{
            method: 'POST',
            headers: {{ 'Content-Type': 'text/plain' }},
            body: url,
        }}).catch(() => {{}});
    }}

    // —— 1. 拦截 <img src> ——
    const imgDescriptor = Object.getOwnPropertyDescriptor(HTMLImageElement.prototype, 'src');
    if (imgDescriptor && imgDescriptor.set) {{
        Object.defineProperty(HTMLImageElement.prototype, 'src', {{
            set(v) {{
                if (isTileUrl(v)) report(v);
                imgDescriptor.set.call(this, v);
            }},
            get() {{ return imgDescriptor.get.call(this); }},
            configurable: true,
        }});
    }}

    // —— 2. 拦截 fetch() ——
    const _fetch = window.fetch;
    window.fetch = function(input, init) {{
        const u = typeof input === 'string' ? input : (input && input.url) || '';
        if (isTileUrl(u)) report(u);
        return _fetch.apply(this, arguments);
    }};

    // —— 3. 拦截 XMLHttpRequest.open() ——
    const _xhrOpen = XMLHttpRequest.prototype.open;
    XMLHttpRequest.prototype.open = function(method, url) {{
        if (isTileUrl(String(url || ''))) report(String(url));
        return _xhrOpen.apply(this, arguments);
    }};
}})();
"#,
        port = port
    );

    // 关闭已存在的抓取窗口（防止重复）
    if let Some(w) = app.get_webview_window("tile-capture") {
        let _ = w.close();
        tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;
    }

    let parsed_url = url
        .parse::<tauri::Url>()
        .map_err(|e| format!("无效 URL: {e}"))?;

    tauri::WebviewWindowBuilder::new(
        &app,
        "tile-capture",
        tauri::WebviewUrl::External(parsed_url),
    )
    .title("网页抓取 — 浏览地图以捕获瓦片 URL")
    .inner_size(1280.0, 800.0)
    .initialization_script(&inject_js)
    .build()
    .map_err(|e| format!("创建抓取窗口失败: {e}"))?;

    Ok(port)
}

/// 获取当前会话已捕获的瓦片数据源列表
#[command]
pub async fn get_captured_tiles(
    session: State<'_, Arc<CaptureSession>>,
) -> Result<Vec<TileSource>, String> {
    Ok(session.tiles.lock().await.clone())
}

/// 清空当前会话的捕获结果
#[command]
pub async fn clear_captured_tiles(session: State<'_, Arc<CaptureSession>>) -> Result<(), String> {
    session.tiles.lock().await.clear();
    Ok(())
}

/// 关闭抓取窗口并停止临时 HTTP 服务器
#[command]
pub async fn close_capture_window(
    app: AppHandle,
    session: State<'_, Arc<CaptureSession>>,
) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("tile-capture") {
        let _ = w.close();
    }
    if let Some(sender) = session.shutdown_tx.lock().await.take() {
        let _ = sender.send(());
    }
    Ok(())
}
