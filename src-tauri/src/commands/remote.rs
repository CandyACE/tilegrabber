//! 远程协作 Tauri 命令

use tauri::State;
use uuid::Uuid;

use crate::remote::{ClientInfo, RemoteClients, RemoteConfigCache};
use crate::storage::app_db::{AppDb, NewRemoteServer, RemoteServer};

/// 生成新的访问令牌并保存到数据库，同时使配置缓存失效
#[tauri::command]
pub async fn generate_remote_token(
    app_db: State<'_, AppDb>,
    remote_cache: State<'_, RemoteConfigCache>,
) -> Result<String, String> {
    let token = format!("tg_{}", Uuid::new_v4());
    app_db
        .set_setting("remote.token", &token)
        .map_err(|e| e.to_string())?;
    *remote_cache.write().await = None;
    Ok(token)
}

/// 获取当前在线的远程客户端列表
#[tauri::command]
pub fn get_remote_clients(clients: State<'_, RemoteClients>) -> Vec<ClientInfo> {
    clients.list()
}

/// 列出已保存的远程服务器
#[tauri::command]
pub fn list_remote_servers(app_db: State<'_, AppDb>) -> Result<Vec<RemoteServer>, String> {
    app_db.list_remote_servers().map_err(|e| e.to_string())
}

/// 添加远程服务器
#[tauri::command]
pub fn add_remote_server(
    server: NewRemoteServer,
    app_db: State<'_, AppDb>,
) -> Result<RemoteServer, String> {
    let id = uuid::Uuid::new_v4().to_string();
    app_db.create_remote_server(&id, &server).map_err(|e| e.to_string())?;
    app_db
        .list_remote_servers()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|s| s.id == id)
        .ok_or_else(|| "创建后未能查到服务器记录".to_string())
}

/// 删除远程服务器
#[tauri::command]
pub fn remove_remote_server(id: String, app_db: State<'_, AppDb>) -> Result<(), String> {
    app_db.delete_remote_server(&id).map_err(|e| e.to_string())
}

/// 更新远程服务器信息
#[tauri::command]
pub fn update_remote_server(
    id: String,
    server: NewRemoteServer,
    app_db: State<'_, AppDb>,
) -> Result<(), String> {
    app_db
        .update_remote_server(&id, &server)
        .map_err(|e| e.to_string())
}
