//! TileGrabber — 瓦片发布服务
//!
//! 基于 axum 提供 TMS / WMTS 服务端点，供 QGIS、OfflineMaps、Cesium 等
//! 客户端直接加载本地已下载的瓦片。
//!
//! 路由说明：
//! - TMS  ：`GET /tiles/{task_id}/{z}/{x}/{y}`
//! - WMTS ：`GET /wmts/{task_id}?SERVICE=WMTS&REQUEST=GetCapabilities`
//!          `GET /wmts/{task_id}?SERVICE=WMTS&REQUEST=GetTile&...`
//! - 状态 ：`GET /api/tasks` (JSON 任务列表)

pub mod handlers;

use std::sync::{Arc, Mutex};

use axum::{Router, routing::get};
use tokio::sync::oneshot;
use tower_http::cors::{Any, CorsLayer};

use crate::storage::app_db::AppDb;

// ─── 共享应用状态 ────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct ServerAppState {
    pub app_db: AppDb,
    pub base_url: String, // e.g. "http://localhost:8765"
}

// ─── 服务控制 ————————————────————────────────────────────————────────————————

pub struct TileServer {
    shutdown_tx: Option<oneshot::Sender<()>>,
    pub port: u16,
}

impl TileServer {
    pub fn new() -> Self {
        Self {
            shutdown_tx: None,
            port: 8765,
        }
    }

    pub fn is_running(&self) -> bool {
        self.shutdown_tx.is_some()
    }
}

/// Tauri 管理的服务器状态（Arc<Mutex<TileServer>>）
pub type TileServerState = Arc<Mutex<TileServer>>;

/// 在后台启动 axum 服务器
///
/// 返回实际监听的端口（若指定端口被占用则返回错误）
pub async fn start_server(
    state: TileServerState,
    port: u16,
    app_db: AppDb,
) -> Result<u16, String> {
    // 若已在运行，先停止
    {
        let mut s = state.lock().map_err(|_| "mutex poisoned")?;
        if s.is_running() {
            if let Some(tx) = s.shutdown_tx.take() {
                let _ = tx.send(());
            }
        }
    }

    let base_url = format!("http://localhost:{port}");
    let app_state = ServerAppState {
        app_db,
        base_url: base_url.clone(),
    };

    let cors = CorsLayer::new()
        .allow_methods(Any)
        .allow_headers(Any)
        .allow_origin(Any);

    let router: Router = Router::new()
        // TMS 端点
        .route(
            "/tiles/:task_id/:z/:x/:y",
            get(handlers::tms_tile),
        )
        // WMTS 端点（单入口，通过 query 参数区分请求类型）
        .route(
            "/wmts/:task_id",
            get(handlers::wmts_dispatch),
        )
        // REST API
        .route("/api/tasks", get(handlers::api_tasks))
        .route("/api/tasks/:id", get(handlers::api_task_get))
        .route("/api/tasks/:id/logs", get(handlers::api_task_logs))
        .route("/api/info", get(handlers::api_info))
        .layer(cors)
        .with_state(app_state);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}"))
        .await
        .map_err(|e| format!("端口 {port} 绑定失败: {e}"))?;

    let actual_port = listener.local_addr().map(|a| a.port()).unwrap_or(port);

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    // 启动异步服务（不阻塞 Tauri 主线程）
    tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .ok();
    });

    // 更新服务器状态
    {
        let mut s = state.lock().map_err(|_| "mutex poisoned")?;
        s.shutdown_tx = Some(shutdown_tx);
        s.port = actual_port;
    }

    Ok(actual_port)
}

/// 优雅关闭 axum 服务器
pub fn stop_server(state: &TileServerState) -> Result<(), String> {
    let mut s = state.lock().map_err(|_| "mutex poisoned")?;
    if let Some(tx) = s.shutdown_tx.take() {
        let _ = tx.send(());
        Ok(())
    } else {
        Err("服务器未在运行".into())
    }
}
