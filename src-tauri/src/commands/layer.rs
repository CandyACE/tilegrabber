//! Tauri 命令：图层管理
//!
//! 暴露给前端的 invoke 命令：
//! - `create_layer`   — 新建图层（source_config 已由前端解析完毕）
//! - `list_layers`    — 列出全部图层（按 sort_order 排序）
//! - `delete_layer`   — 删除图层
//! - `reorder_layers` — 批量更新排序

use tauri::State;
use uuid::Uuid;

use crate::storage::app_db::{AppDb, Layer, NewLayer};

/// 新建图层
///
/// 前端传入已解析好的 `source_config`（TileSource JSON），不存文件路径。
#[tauri::command]
pub async fn create_layer(
    new_layer: NewLayer,
    app_db: State<'_, AppDb>,
) -> Result<String, String> {
    let id = Uuid::new_v4().to_string();
    app_db
        .create_layer(&id, &new_layer)
        .map_err(|e| e.to_string())?;
    Ok(id)
}

/// 列出全部图层（按 sort_order 升序）
#[tauri::command]
pub async fn list_layers(app_db: State<'_, AppDb>) -> Result<Vec<Layer>, String> {
    app_db.list_layers().map_err(|e| e.to_string())
}

/// 删除图层
#[tauri::command]
pub async fn delete_layer(
    layer_id: String,
    app_db: State<'_, AppDb>,
) -> Result<(), String> {
    app_db.delete_layer(&layer_id).map_err(|e| e.to_string())
}

/// 批量更新图层排序（传入期望顺序的 id 列表）
#[tauri::command]
pub async fn reorder_layers(
    layer_ids: Vec<String>,
    app_db: State<'_, AppDb>,
) -> Result<(), String> {
    app_db
        .reorder_layers(&layer_ids)
        .map_err(|e| e.to_string())
}
/// 重命名图层
#[tauri::command]
pub async fn rename_layer(
    layer_id: String,
    name: String,
    app_db: State<'_, AppDb>,
) -> Result<(), String> {
    app_db
        .rename_layer(&layer_id, &name)
        .map_err(|e| e.to_string())
}