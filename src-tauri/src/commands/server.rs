//! 瓦片发布服务 Tauri 命令

use tauri::State;

use crate::server::{TileServerState, start_server, stop_server};
use crate::storage::app_db::AppDb;

/// 服务器当前状态（返回给前端）
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerStatus {
    pub running: bool,
    pub port: u16,
    pub base_url: String,
}

// ─── Tauri 命令 ──────────────────────────────────────────────────────────────

/// 启动瓦片发布服务
#[tauri::command]
pub async fn start_tile_server(
    port: u16,
    server_state: State<'_, TileServerState>,
    app_db: State<'_, AppDb>,
) -> Result<ServerStatus, String> {
    let ss = server_state.inner().clone();
    let db = app_db.inner().clone();
    let actual_port = start_server(ss, port, db).await?;
    Ok(ServerStatus {
        running: true,
        port: actual_port,
        base_url: format!("http://localhost:{actual_port}"),
    })
}

/// 停止瓦片发布服务
#[tauri::command]
pub async fn stop_tile_server(
    server_state: State<'_, TileServerState>,
) -> Result<(), String> {
    stop_server(server_state.inner())
}

/// 查询服务器当前状态
#[tauri::command]
pub async fn get_server_status(
    server_state: State<'_, TileServerState>,
) -> Result<ServerStatus, String> {
    let s = server_state
        .lock()
        .map_err(|_| "mutex poisoned".to_string())?;
    Ok(ServerStatus {
        running: s.is_running(),
        port: s.port,
        base_url: format!("http://localhost:{}", s.port),
    })
}
