//! 磁盘空间 / 任务存储统计命令（G 套件）

use std::collections::HashSet;
use std::path::PathBuf;

use fs2::{available_space, total_space};
use serde::Serialize;
use tauri::{AppHandle, State};

use crate::storage::app_db::AppDb;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskStorageRow {
    pub task_id: String,
    pub name: String,
    pub path: String,
    pub bytes: u64,
    pub exists: bool,
    /// 是否是外部 .tgr 文件（由 import_task 注册的外部存储）
    pub is_external: bool,
    pub downloaded_tiles: i64,
    pub status: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrphanFile {
    pub path: String,
    pub bytes: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageStats {
    pub tiles_dir: String,
    /// 所有任务 .tiles 文件大小之和（含外部 .tgr）
    pub total_bytes: u64,
    /// 默认目录所在卷的可用空间
    pub available_bytes: u64,
    /// 默认目录所在卷的总容量
    pub capacity_bytes: u64,
    pub tasks: Vec<TaskStorageRow>,
    /// 默认目录中存在但未被任何任务引用的孤儿 .tiles 文件
    pub orphans: Vec<OrphanFile>,
    pub orphan_bytes: u64,
}

/// 汇总每个任务的瓦片存储占用 + 磁盘容量
#[tauri::command]
pub async fn storage_stats(
    app_db: State<'_, AppDb>,
    app: AppHandle,
) -> Result<StorageStats, String> {
    let tiles_dir = crate::commands::task::get_tiles_dir(&app, app_db.inner())?;
    std::fs::create_dir_all(&tiles_dir).ok();

    let tasks_meta = app_db.list_tasks().map_err(|e| e.to_string())?;

    let mut tasks: Vec<TaskStorageRow> = Vec::with_capacity(tasks_meta.len());
    let mut referenced: HashSet<PathBuf> = HashSet::new();
    let mut total_bytes: u64 = 0;

    for t in tasks_meta {
        let path_str = t.tile_store_path.clone().unwrap_or_default();
        let (bytes, exists) = if path_str.is_empty() {
            (0u64, false)
        } else {
            match std::fs::metadata(&path_str) {
                Ok(m) => (m.len(), true),
                Err(_) => (0u64, false),
            }
        };
        if exists {
            total_bytes = total_bytes.saturating_add(bytes);
            if let Ok(canon) = PathBuf::from(&path_str).canonicalize() {
                referenced.insert(canon);
            } else {
                referenced.insert(PathBuf::from(&path_str));
            }
        }
        let is_external = path_str.ends_with(".tgr");
        tasks.push(TaskStorageRow {
            task_id: t.id,
            name: t.name,
            path: path_str,
            bytes,
            exists,
            is_external,
            downloaded_tiles: t.downloaded_tiles,
            status: t.status,
        });
    }

    // 按大小降序排序
    tasks.sort_by(|a, b| b.bytes.cmp(&a.bytes));

    // 默认目录扫描孤儿文件（仅扫描 *.tiles，不含外部 .tgr）
    let mut orphans: Vec<OrphanFile> = Vec::new();
    let mut orphan_bytes: u64 = 0;
    if let Ok(rd) = std::fs::read_dir(&tiles_dir) {
        for entry in rd.flatten() {
            let p = entry.path();
            if !p.is_file() {
                continue;
            }
            let ext_ok = p
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("tiles"))
                .unwrap_or(false);
            if !ext_ok {
                continue;
            }
            let canon = p.canonicalize().unwrap_or_else(|_| p.clone());
            if referenced.contains(&canon) {
                continue;
            }
            let bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
            orphan_bytes = orphan_bytes.saturating_add(bytes);
            orphans.push(OrphanFile {
                path: p.to_string_lossy().to_string(),
                bytes,
            });
        }
    }
    orphans.sort_by(|a, b| b.bytes.cmp(&a.bytes));

    let available_bytes = available_space(&tiles_dir).unwrap_or(0);
    let capacity_bytes = total_space(&tiles_dir).unwrap_or(0);

    Ok(StorageStats {
        tiles_dir: tiles_dir.to_string_lossy().to_string(),
        total_bytes,
        available_bytes,
        capacity_bytes,
        tasks,
        orphans,
        orphan_bytes,
    })
}

/// 删除指定路径的孤儿 .tiles 文件（必须位于默认 tiles_dir 内，且未被任何任务引用）
#[tauri::command]
pub async fn cleanup_orphan_tiles(
    paths: Vec<String>,
    app_db: State<'_, AppDb>,
    app: AppHandle,
) -> Result<u64, String> {
    let tiles_dir = crate::commands::task::get_tiles_dir(&app, app_db.inner())?;
    let tiles_dir_canon = tiles_dir
        .canonicalize()
        .unwrap_or_else(|_| tiles_dir.clone());

    // 收集所有任务引用，避免误删
    let referenced: HashSet<PathBuf> = app_db
        .list_tasks()
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter_map(|t| t.tile_store_path)
        .map(|p| PathBuf::from(&p).canonicalize().unwrap_or_else(|_| PathBuf::from(p)))
        .collect();

    let mut reclaimed: u64 = 0;
    for raw in paths {
        let p = PathBuf::from(&raw);
        let canon = p.canonicalize().unwrap_or_else(|_| p.clone());

        // 必须在 tiles_dir 内
        if !canon.starts_with(&tiles_dir_canon) {
            continue;
        }
        // 不能引用某任务
        if referenced.contains(&canon) {
            continue;
        }
        // 后缀必须为 .tiles
        let ext_ok = canon
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("tiles"))
            .unwrap_or(false);
        if !ext_ok {
            continue;
        }

        let bytes = std::fs::metadata(&canon).map(|m| m.len()).unwrap_or(0);
        if std::fs::remove_file(&canon).is_ok() {
            reclaimed = reclaimed.saturating_add(bytes);
            // 同时尝试清理 SQLite WAL / SHM 兄弟文件
            for sib_ext in ["tiles-wal", "tiles-shm"] {
                let mut sib = canon.clone();
                sib.set_extension(sib_ext);
                let _ = std::fs::remove_file(&sib);
            }
        }
    }
    Ok(reclaimed)
}
