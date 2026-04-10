//! 瓦片发布服务 Tauri 命令

use tauri::State;

use crate::server::{ServiceStats, StatsMap, TileServerState, start_server, stop_server};
use crate::storage::app_db::AppDb;

/// 服务器当前状态（返回给前端）
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerStatus {
    pub running: bool,
    pub port: u16,
    pub base_url: String,
    /// 局域网可访问的 URL 列表（含所有非回环 IPv4 地址）
    pub lan_urls: Vec<String>,
}

/// 获取本机所有非回环 IPv4 地址对应的服务 URL
fn get_lan_urls(port: u16) -> Vec<String> {
    use std::net::UdpSocket;
    let mut urls = Vec::new();
    // UDP 无连接 trick：连接外部地址以确定出站 IP
    if let Ok(socket) = UdpSocket::bind("0.0.0.0:0") {
        if socket.connect("8.8.8.8:80").is_ok() {
            if let Ok(addr) = socket.local_addr() {
                let ip = addr.ip().to_string();
                if !ip.starts_with("127.") && !ip.starts_with("::1") {
                    urls.push(format!("http://{}:{}", ip, port));
                }
            }
        }
    }
    urls
}

// ─── Tauri 命令 ──────────────────────────────────────────────────────────────

/// 启动瓦片发布服务
#[tauri::command]
pub async fn start_tile_server(
    port: u16,
    server_state: State<'_, TileServerState>,
    app_db: State<'_, AppDb>,
    stats: State<'_, StatsMap>,
) -> Result<ServerStatus, String> {
    let ss = server_state.inner().clone();
    let db = app_db.inner().clone();
    let st = stats.inner().clone();
    let actual_port = start_server(ss, port, db, st).await?;
    Ok(ServerStatus {
        running: true,
        port: actual_port,
        base_url: format!("http://localhost:{actual_port}"),
        lan_urls: get_lan_urls(actual_port),
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
        lan_urls: if s.is_running() { get_lan_urls(s.port) } else { Vec::new() },
    })
}

/// 获取各服务的请求统计
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskStatsDto {
    pub task_id: String,
    #[serde(flatten)]
    pub stats: ServiceStats,
}

#[tauri::command]
pub async fn get_service_stats(
    stats: State<'_, StatsMap>,
) -> Result<Vec<TaskStatsDto>, String> {
    let map = stats.lock().map_err(|_| "mutex poisoned".to_string())?;
    let result = map
        .iter()
        .map(|(k, v)| TaskStatsDto {
            task_id: k.clone(),
            stats: v.clone(),
        })
        .collect();
    Ok(result)
}
