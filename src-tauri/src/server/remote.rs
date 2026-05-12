//! 远程协作 HTTP 端点
//!
//! 路由（全部需要 `Authorization: Bearer <token>` 头）：
//! - `POST   /remote/tasks`            — 提交新下载任务
//! - `GET    /remote/tasks`            — 查询所有任务
//! - `GET    /remote/tasks/:id`        — 查询单任务
//! - `DELETE /remote/tasks/:id`        — 取消任务
//! - `GET    /remote/tasks/:id/progress` — SSE 实时进度流

use std::convert::Infallible;
use std::net::SocketAddr;

use axum::{
    body::Body,
    extract::{ConnectInfo, Path, Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response, Sse},
    Json,
};
use axum::response::sse::Event;
use chrono::Utc;
use futures_util::stream;
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager};
use uuid::Uuid;

use crate::download::engine::ProgressPayload;
use crate::remote::{ClientGuard, ClientInfo, RemoteConfig};
use crate::server::ServerAppState;
use crate::storage::app_db::NewTask;

// ─── Auth 中间件 ──────────────────────────────────────────────────────────────

/// 从配置缓存读取 RemoteConfig（脏时回写 DB 值）
async fn load_remote_config(state: &ServerAppState) -> RemoteConfig {
    // 快速路径：缓存命中
    {
        let guard = state.remote_config_cache.read().await;
        if let Some(ref cfg) = *guard {
            return cfg.clone();
        }
    }
    // 缓存脏：从 DB 重建
    let enabled = state
        .app_db
        .get_setting("remote.enabled")
        .ok()
        .flatten()
        .map(|v| v == "true")
        .unwrap_or(false);
    let token = state
        .app_db
        .get_setting("remote.token")
        .ok()
        .flatten()
        .unwrap_or_default();
    let cfg = RemoteConfig { enabled, token };
    *state.remote_config_cache.write().await = Some(cfg.clone());
    cfg
}

/// Bearer Token 验证中间件
pub async fn auth_middleware(
    State(state): State<ServerAppState>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let cfg = load_remote_config(&state).await;

    if !cfg.enabled {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "远程功能未开启"})),
        )
            .into_response();
    }

    if cfg.token.is_empty() {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "服务端未配置 Token"})),
        )
            .into_response();
    }

    let auth = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let provided = auth.strip_prefix("Bearer ").unwrap_or("");
    if provided != cfg.token {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "Token 无效"})),
        )
            .into_response();
    }

    next.run(req).await
}

// ─── 请求/响应类型 ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitTaskRequest {
    pub name: String,
    pub source_config: String,
    pub bounds_west: f64,
    pub bounds_east: f64,
    pub bounds_south: f64,
    pub bounds_north: f64,
    pub min_zoom: u8,
    pub max_zoom: u8,
    #[serde(default)]
    pub clip_to_bounds: bool,
    #[serde(default)]
    pub polygon_wgs84: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitTaskResponse {
    pub task_id: String,
}

// ─── 端点处理函数 ─────────────────────────────────────────────────────────────

/// POST /remote/tasks — 提交新任务
pub async fn submit_task(
    State(state): State<ServerAppState>,
    Json(req): Json<SubmitTaskRequest>,
) -> impl IntoResponse {
    let task_id = Uuid::new_v4().to_string();

    // 确定 tiles 存储目录（复用与本地任务相同的逻辑）
    let tiles_dir = {
        let custom = state
            .app_db
            .get_setting("app.tiles_dir")
            .ok()
            .flatten()
            .filter(|s| !s.trim().is_empty());
        if let Some(dir) = custom {
            std::path::PathBuf::from(dir)
        } else {
            let base = state.app_handle.path().document_dir()
                .or_else(|_| state.app_handle.path().app_local_data_dir());
            match base {
                Ok(p) => p.join("御图").join("tiles"),
                Err(e) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({"error": format!("无法确定存储目录: {}", e)})),
                    )
                        .into_response();
                }
            }
        }
    };

    if let Err(e) = std::fs::create_dir_all(&tiles_dir) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("无法创建存储目录: {}", e)})),
        )
            .into_response();
    }

    let tile_store_path = tiles_dir
        .join(format!("{}.tiles", task_id))
        .to_string_lossy()
        .to_string();

    let new_task = NewTask {
        name: req.name,
        source_config: req.source_config,
        bounds_west: req.bounds_west,
        bounds_east: req.bounds_east,
        bounds_south: req.bounds_south,
        bounds_north: req.bounds_north,
        min_zoom: req.min_zoom,
        max_zoom: req.max_zoom,
        clip_to_bounds: req.clip_to_bounds,
        polygon_wgs84: req.polygon_wgs84,
    };

    if let Err(e) = state
        .app_db
        .create_task_with_source(&task_id, &new_task, &tile_store_path, "remote")
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("创建任务失败: {}", e)})),
        )
            .into_response();
    }

    // 立即启动下载引擎（与本地 start_download 逻辑相同）
    let engine = state
        .app_handle
        .state::<crate::download::engine::DownloadEngine>();
    let concurrency = state
        .app_db
        .get_setting("download.concurrency")
        .ok()
        .flatten()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(8);
    if let Err(e) = engine.start(
        task_id.clone(),
        state.app_db.clone(),
        concurrency,
        state.app_handle.clone(),
    ) {
        // 启动失败不影响任务已创建，仅通知前端
        let _ = state.app_handle.emit(
            "remote:task-start-failed",
            serde_json::json!({"taskId": task_id, "error": e.to_string()}),
        );
    }

    // 通知前端刷新任务列表
    let _ = state.app_handle.emit("remote:task-submitted", &task_id);

    (StatusCode::CREATED, Json(SubmitTaskResponse { task_id })).into_response()
}

/// GET /remote/tasks — 列出所有任务
pub async fn list_remote_tasks(State(state): State<ServerAppState>) -> impl IntoResponse {
    match state.app_db.list_tasks() {
        Ok(tasks) => Json(tasks).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// GET /remote/tasks/:id — 查询单任务
pub async fn get_remote_task(
    State(state): State<ServerAppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.app_db.get_task(&id) {
        Ok(task) => Json(task).into_response(),
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "任务不存在"})),
        )
            .into_response(),
    }
}

/// DELETE /remote/tasks/:id — 取消任务
pub async fn cancel_remote_task(
    State(state): State<ServerAppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let engine = state
        .app_handle
        .state::<crate::download::engine::DownloadEngine>();
    match engine.cancel(&id) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

// ─── SSE 进度流 ───────────────────────────────────────────────────────────────

struct SseState {
    rx: tokio::sync::broadcast::Receiver<ProgressPayload>,
    task_id: String,
    _guard: ClientGuard,
}

/// GET /remote/tasks/:id/progress — SSE 实时进度流
pub async fn sse_progress(
    State(state): State<ServerAppState>,
    Path(task_id): Path<String>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let client_id = Uuid::new_v4().to_string();
    let ip = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(',').next().unwrap_or("").trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| addr.ip().to_string());

    let info = ClientInfo {
        id: client_id.clone(),
        ip: ip.clone(),
        connected_at: Utc::now().timestamp(),
    };
    state.remote_clients.insert(info);
    let _ = state
        .app_handle
        .emit("remote:clients-changed", state.remote_clients.count());

    let guard = ClientGuard {
        id: client_id,
        clients: state.remote_clients.clone(),
        app: state.app_handle.clone(),
    };

    let rx = state.remote_broadcast_tx.subscribe();

    let sse_state = SseState {
        rx,
        task_id: task_id.clone(),
        _guard: guard,
    };

    let stream = stream::unfold(sse_state, |mut s| async move {
        loop {
            match s.rx.recv().await {
                Ok(payload) if payload.task_id == s.task_id => {
                    let is_terminal = matches!(
                        payload.status.as_str(),
                        "completed" | "completed_with_errors" | "failed" | "cancelled"
                    );
                    let data = serde_json::to_string(&payload).unwrap_or_default();
                    let event = Event::default().event("progress").data(data);
                    if is_terminal {
                        return Some((Ok::<_, Infallible>(event), s));
                    }
                    return Some((Ok(event), s));
                }
                Ok(_) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
            }
        }
    });

    Sse::new(stream)
        .keep_alive(axum::response::sse::KeepAlive::default())
        .into_response()
}

// ─── 暂停 / 恢复 ──────────────────────────────────────────────────────────────

pub async fn pause_remote_task(
    State(state): State<ServerAppState>,
    Path(task_id): Path<String>,
) -> impl IntoResponse {
    let engine = state.app_handle.state::<crate::download::engine::DownloadEngine>();
    match engine.pause(&task_id) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

pub async fn resume_remote_task(
    State(state): State<ServerAppState>,
    Path(task_id): Path<String>,
) -> impl IntoResponse {
    let engine = state.app_handle.state::<crate::download::engine::DownloadEngine>();
    match engine.resume(&task_id) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}
