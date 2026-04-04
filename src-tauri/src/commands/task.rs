//! TileGrabber — 任务管理 Tauri 命令
//!
//! 提供给前端的任务 CRUD + 下载引擎控制接口。

use std::path::PathBuf;

use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

use crate::download::engine::DownloadEngine;
use crate::storage::app_db::{AppDb, LogEntry, NewTask, Task};

// ─── 导出任务状态 ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportJob {
    pub job_id: String,
    pub task_id: String,
    pub format: String,
    pub dest_path: String,
    pub done: u64,
    pub total: u64,
    pub status: String, // "running" | "done" | "error"
    pub error: Option<String>,
}

pub type ExportState =
    std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, ExportJob>>>;

/// 导出进度/完成事件 payload（通过 Tauri 事件推送到前端）
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportProgressPayload {
    job_id: String,
    done: u64,
    total: u64,
    status: String,
    dest_path: String,
    error: Option<String>,
}

/// 默认并发数（当设置读取失败时的备用值）
const DEFAULT_CONCURRENCY: usize = 16;

/// 从 AppDb 读取用户配置的并发数
fn get_concurrency(app_db: &AppDb) -> usize {
    app_db
        .get_setting("download.concurrency")
        .ok()
        .flatten()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(DEFAULT_CONCURRENCY)
}

// ─── 任务创建 ────────────────────────────────────────────────────────────────

/// 创建新的下载任务（仅入库，不启动下载）
#[tauri::command]
pub async fn create_task(
    new_task: NewTask,
    app_db: State<'_, AppDb>,
    app: AppHandle,
) -> Result<String, String> {
    let task_id = Uuid::new_v4().to_string();
    let tiles_dir = get_tiles_dir(&app, app_db.inner())?;

    // 确保目录存在
    std::fs::create_dir_all(&tiles_dir).map_err(|e| e.to_string())?;

    let tile_store_path = tiles_dir
        .join(format!("{}.tiles", task_id))
        .to_string_lossy()
        .to_string();

    app_db
        .create_task(&task_id, &new_task, &tile_store_path)
        .map_err(|e| e.to_string())?;

    Ok(task_id)
}

// ─── 任务读取 ────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_tasks(app_db: State<'_, AppDb>) -> Result<Vec<Task>, String> {
    app_db.list_tasks().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_task(task_id: String, app_db: State<'_, AppDb>) -> Result<Task, String> {
    app_db.get_task(&task_id).map_err(|e| e.to_string())
}

// ─── 任务删除 ────────────────────────────────────────────────────────────────

/// 删除任务及其 tile store 文件
#[tauri::command]
pub async fn delete_task(
    task_id: String,
    delete_file: Option<bool>,
    app_db: State<'_, AppDb>,
    engine: State<'_, DownloadEngine>,
) -> Result<(), String> {
    // 先取消正在运行的下载
    engine.cancel(&task_id).map_err(|e| e.to_string())?;

    // 获取 tile store 路径后再删除记录
    let task = app_db.get_task(&task_id).map_err(|e| e.to_string())?;
    app_db.delete_task(&task_id).map_err(|e| e.to_string())?;

    // 删除瓦片存储文件
    if let Some(path) = &task.tile_store_path {
        let is_external = path.ends_with(".tgr");
        // 外部 .tgr 文件仅在明确要求时删除；内置 .tiles 文件默认删除
        if !is_external || delete_file.unwrap_or(false) {
            let _ = std::fs::remove_file(path);
        }
    }

    Ok(())
}

// ─── 下载控制 ────────────────────────────────────────────────────────────────

/// 启动下载（首次或从暂停状态恢复）
#[tauri::command]
pub async fn start_download(
    task_id: String,
    app_db: State<'_, AppDb>,
    engine: State<'_, DownloadEngine>,
    app: AppHandle,
) -> Result<(), String> {
    // 如果任务已在运行，先尝试恢复（处理重复点击）
    if engine.is_active(&task_id) {
        return engine.resume(&task_id).map_err(|e| e.to_string());
    }

    let concurrency = get_concurrency(app_db.inner());

    engine
        .start(
            task_id,
            app_db.inner().clone(),
            concurrency,
            app,
        )
        .map_err(|e| e.to_string())
}

/// 暂停下载（保存进度，可随时恢复）
#[tauri::command]
pub async fn pause_download(
    task_id: String,
    app_db: State<'_, AppDb>,
    engine: State<'_, DownloadEngine>,
) -> Result<(), String> {
    if engine.is_active(&task_id) {
        engine.pause(&task_id).map_err(|e| e.to_string())
    } else {
        // 引擎无句柄（可能是应用重启前留下的任务），直接更新数据库状态
        app_db
            .update_task_status(&task_id, "paused")
            .map_err(|e| e.to_string())
    }
}

/// 恢复已暂停的任务
#[tauri::command]
pub async fn resume_download(
    task_id: String,
    app_db: State<'_, AppDb>,
    engine: State<'_, DownloadEngine>,
    app: AppHandle,
) -> Result<(), String> {
    if engine.is_active(&task_id) {
        // 已在运行，发送 Run 信号解除暂停
        return engine.resume(&task_id).map_err(|e| e.to_string());
    }

    // 引擎无记录（应用重启后），重新启动
    let concurrency = get_concurrency(app_db.inner());
    engine
        .start(
            task_id,
            app_db.inner().clone(),
            concurrency,
            app,
        )
        .map_err(|e| e.to_string())
}

/// 取消并终止下载（不可恢复，但不删除已下载数据）
#[tauri::command]
pub async fn cancel_download(
    task_id: String,
    app_db: State<'_, AppDb>,
    engine: State<'_, DownloadEngine>,
) -> Result<(), String> {
    if engine.is_active(&task_id) {
        engine.cancel(&task_id).map_err(|e| e.to_string())
    } else {
        // 引擎无句柄，直接将数据库状态标记为已取消
        app_db
            .update_task_status(&task_id, "cancelled")
            .map_err(|e| e.to_string())
    }
}

/// 重试失败的瓦片（将 failed → pending，然后启动下载）
#[tauri::command]
pub async fn retry_failed(
    task_id: String,
    app_db: State<'_, AppDb>,
    engine: State<'_, DownloadEngine>,
    app: AppHandle,
) -> Result<i64, String> {
    // 打开 tile store，重置失败记录
    let task = app_db.get_task(&task_id).map_err(|e| e.to_string())?;
    let path = task
        .tile_store_path
        .as_deref()
        .ok_or("tile store path not set")?;

    let tile_store =
        crate::storage::tile_store::TileStore::open(std::path::Path::new(path), &task_id)
            .map_err(|e| e.to_string())?;
    let count = tile_store.reset_failed().map_err(|e| e.to_string())?;

    // 重新更新 DB 状态并启动
    app_db
        .update_task_status(&task_id, "pending")
        .map_err(|e| e.to_string())?;

    let concurrency = get_concurrency(app_db.inner());
    engine
        .start(
            task_id,
            app_db.inner().clone(),
            concurrency,
            app,
        )
        .map_err(|e| e.to_string())?;

    Ok(count)
}

// ─── 日志 ────────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn get_task_logs(
    task_id: String,
    limit: Option<u32>,
    app_db: State<'_, AppDb>,
) -> Result<Vec<LogEntry>, String> {
    app_db
        .get_task_logs(&task_id, limit.unwrap_or(200))
        .map_err(|e| e.to_string())
}

// ─── 导出 ────────────────────────────────────────────────────────────────────

/// 将任务瓦片包导出为标准 MBTiles（含 TMS tile_row 翻转 + metadata）
///
/// 在后台启动 MBTiles 导出任务，立即返回 job_id。
/// 导出进度通过 `export-progress` Tauri 事件推送。
#[tauri::command]
pub async fn export_mbtiles(
    task_id: String,
    dest_path: String,
    clip_to_bounds: bool,
    app_db: State<'_, AppDb>,
    export_state: State<'_, ExportState>,
    app: AppHandle,
) -> Result<String, String> {
    let task = app_db.get_task(&task_id).map_err(|e| e.to_string())?;
    let src_path = task
        .tile_store_path
        .as_ref()
        .ok_or("该任务尚无瓦片存储文件（可能未开始下载）")?
        .clone();

    if !std::path::Path::new(&src_path).exists() {
        return Err(format!("瓦片文件不存在: {}", src_path));
    }

    let format = serde_json::from_str::<serde_json::Value>(&task.source_config)
        .ok()
        .and_then(|v| v.get("format").and_then(|f| f.as_str()).map(str::to_string))
        .unwrap_or_else(|| "png".into());
    let bounds = [task.bounds_west, task.bounds_south, task.bounds_east, task.bounds_north];
    let polygon: Option<Vec<[f64; 2]>> = task.polygon_wgs84.as_deref().and_then(|s| serde_json::from_str(s).ok());
    let crs = serde_json::from_str::<crate::types::TileSource>(&task.source_config).map(|s| s.crs).unwrap_or_default();
    let task_name = task.name.clone();

    let job_id = Uuid::new_v4().to_string();
    let job = ExportJob {
        job_id: job_id.clone(),
        task_id: task_id.clone(),
        format: "mbtiles".into(),
        dest_path: dest_path.clone(),
        done: 0,
        total: 0,
        status: "running".into(),
        error: None,
    };
    export_state.lock().unwrap().insert(job_id.clone(), job);

    let state_clone = export_state.inner().clone();
    let app_clone = app.clone();
    let jid = job_id.clone();

    tokio::task::spawn_blocking(move || {
        let result = crate::export::mbtiles::export_mbtiles(
            std::path::Path::new(&src_path),
            std::path::Path::new(&dest_path),
            &task_name,
            bounds,
            task.min_zoom,
            task.max_zoom,
            &format,
            clip_to_bounds,
            polygon.as_deref(),
            &crs,
            |done, total| {
                if let Ok(mut map) = state_clone.lock() {
                    if let Some(j) = map.get_mut(&jid) { j.done = done; j.total = total; }
                }
                let _ = app_clone.emit("export-progress", ExportProgressPayload {
                    job_id: jid.clone(), done, total, status: "running".into(),
                    dest_path: dest_path.clone(), error: None,
                });
            },
        );
        let (status, error, done): (String, Option<String>, u64) = match result {
            Ok(n) => ("done".into(), None, n),
            Err(e) => ("error".into(), Some(e.to_string()), 0),
        };
        if let Ok(mut map) = state_clone.lock() {
            if let Some(j) = map.get_mut(&jid) {
                j.status = status.clone(); j.error = error.clone(); if done > 0 { j.done = done; }
            }
        }
        let _ = app_clone.emit("export-progress", ExportProgressPayload {
            job_id: jid.clone(), done, total: done, status, dest_path: dest_path.clone(), error,
        });
    });

    Ok(job_id)
}

/// 在后台启动目录格式导出任务，立即返回 job_id。
/// 导出进度通过 `export-progress` Tauri 事件推送。
#[tauri::command]
pub async fn export_directory(
    task_id: String,
    dest_dir: String,
    clip_to_bounds: bool,
    app_db: State<'_, AppDb>,
    export_state: State<'_, ExportState>,
    app: AppHandle,
) -> Result<String, String> {
    let task = app_db.get_task(&task_id).map_err(|e| e.to_string())?;
    let src_path = task
        .tile_store_path
        .as_ref()
        .ok_or("该任务尚无瓦片存储文件（可能未开始下载）")?
        .clone();

    if !std::path::Path::new(&src_path).exists() {
        return Err(format!("瓦片文件不存在: {}", src_path));
    }

    let format = serde_json::from_str::<serde_json::Value>(&task.source_config)
        .ok()
        .and_then(|v| v.get("format").and_then(|f| f.as_str()).map(str::to_string))
        .unwrap_or_else(|| "png".into());
    let bounds = [task.bounds_west, task.bounds_south, task.bounds_east, task.bounds_north];
    let polygon: Option<Vec<[f64; 2]>> = task.polygon_wgs84.as_deref().and_then(|s| serde_json::from_str(s).ok());
    let crs = serde_json::from_str::<crate::types::TileSource>(&task.source_config).map(|s| s.crs).unwrap_or_default();

    let job_id = Uuid::new_v4().to_string();
    let job = ExportJob {
        job_id: job_id.clone(),
        task_id: task_id.clone(),
        format: "directory".into(),
        dest_path: dest_dir.clone(),
        done: 0,
        total: 0,
        status: "running".into(),
        error: None,
    };
    export_state.lock().unwrap().insert(job_id.clone(), job);

    let state_clone = export_state.inner().clone();
    let app_clone = app.clone();
    let jid = job_id.clone();

    tokio::task::spawn_blocking(move || {
        let dp = dest_dir.clone();
        let result = crate::export::directory::export_directory(
            std::path::Path::new(&src_path),
            std::path::Path::new(&dest_dir),
            &format,
            clip_to_bounds,
            bounds,
            polygon.as_deref(),
            &crs,
            |done, total| {
                if let Ok(mut map) = state_clone.lock() {
                    if let Some(j) = map.get_mut(&jid) { j.done = done; j.total = total; }
                }
                let _ = app_clone.emit("export-progress", ExportProgressPayload {
                    job_id: jid.clone(), done, total, status: "running".into(),
                    dest_path: dp.clone(), error: None,
                });
            },
        );
        let (status, error, done): (String, Option<String>, u64) = match result {
            Ok(n) => ("done".into(), None, n),
            Err(e) => ("error".into(), Some(e.to_string()), 0),
        };
        if let Ok(mut map) = state_clone.lock() {
            if let Some(j) = map.get_mut(&jid) {
                j.status = status.clone(); j.error = error.clone(); if done > 0 { j.done = done; }
            }
        }
        let _ = app_clone.emit("export-progress", ExportProgressPayload {
            job_id: jid.clone(), done, total: done, status, dest_path: dest_dir.clone(), error,
        });
    });

    Ok(job_id)
}

/// 在后台启动 GeoTIFF 导出任务，立即返回 job_id。
/// 导出进度通过 `export-progress` Tauri 事件推送。
#[tauri::command]
pub async fn export_geotiff(
    task_id: String,
    dest_path: String,
    zoom: u8,
    clip_to_bounds: bool,
    app_db: State<'_, AppDb>,
    export_state: State<'_, ExportState>,
    app: AppHandle,
) -> Result<String, String> {
    let task = app_db.get_task(&task_id).map_err(|e| e.to_string())?;
    let src_path = task
        .tile_store_path
        .as_ref()
        .ok_or("该任务尚无瓦片存储文件（可能未开始下载）")?
        .clone();

    if !std::path::Path::new(&src_path).exists() {
        return Err(format!("瓦片文件不存在: {}", src_path));
    }

    let bounds = [task.bounds_west, task.bounds_south, task.bounds_east, task.bounds_north];
    let crs = serde_json::from_str::<crate::types::TileSource>(&task.source_config)
        .map(|s| s.crs)
        .unwrap_or_default();

    // 若启用精确裁剪且任务带有多边形，解析顶点用于像素级掩膜
    let polygon: Option<Vec<[f64; 2]>> = if clip_to_bounds {
        task.polygon_wgs84
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
    } else {
        None
    };

    let job_id = Uuid::new_v4().to_string();
    let job = ExportJob {
        job_id: job_id.clone(),
        task_id: task_id.clone(),
        format: "geotiff".into(),
        dest_path: dest_path.clone(),
        done: 0,
        total: 0,
        status: "running".into(),
        error: None,
    };
    export_state.lock().unwrap().insert(job_id.clone(), job);

    let state_clone = export_state.inner().clone();
    let app_clone = app.clone();
    let jid = job_id.clone();

    tokio::task::spawn_blocking(move || {
        let dp = dest_path.clone();
        let state_clone2 = state_clone.clone();
        let app_clone2 = app_clone.clone();
        let jid2 = jid.clone();
        let dp2 = dp.clone();
        let result = crate::export::geotiff::export_geotiff(
            std::path::Path::new(&src_path),
            std::path::Path::new(&dest_path),
            bounds,
            zoom,
            clip_to_bounds,
            polygon,
            &crs,
            move |done, total| {
                if let Ok(mut map) = state_clone2.lock() {
                    if let Some(j) = map.get_mut(&jid2) { j.done = done; j.total = total; }
                }
                let _ = app_clone2.emit("export-progress", ExportProgressPayload {
                    job_id: jid2.clone(), done, total, status: "running".into(),
                    dest_path: dp2.clone(), error: None,
                });
            },
        );
        let (status, error, done): (String, Option<String>, u64) = match result {
            Ok(n) => ("done".into(), None, n),
            Err(e) => ("error".into(), Some(e.to_string()), 0),
        };
        if let Ok(mut map) = state_clone.lock() {
            if let Some(j) = map.get_mut(&jid) {
                j.status = status.clone(); j.error = error.clone(); j.done = done; j.total = done;
            }
        }
        let _ = app_clone.emit("export-progress", ExportProgressPayload {
            job_id: jid.clone(), done, total: done, status, dest_path: dp, error,
        });
    });

    Ok(job_id)
}

/// 查询所有导出任务的当前状态（用于页面刷新后重新连接）
#[tauri::command]
pub async fn get_export_jobs(
    export_state: State<'_, ExportState>,
) -> Result<Vec<ExportJob>, String> {
    let map = export_state.lock().map_err(|e| e.to_string())?;
    Ok(map.values().cloned().collect())
}

// ─── 工具 ────────────────────────────────────────────────────────────────────

/// 从本地瓦片存储读取单张瓦片字节（供前端地图图层预览）
/// 若任务开启了 clip_to_bounds，对边缘瓦片进行像素级精确裁剪后返回。
#[tauri::command]
pub async fn get_stored_tile(
    task_id: String,
    z: i64,
    x: i64,
    y: i64,
    app_db: State<'_, AppDb>,
) -> Result<Vec<u8>, String> {
    let task = app_db.get_task(&task_id).map_err(|e| e.to_string())?;
    let path = task
        .tile_store_path
        .ok_or("tile_store_path not set")?;

    let clip_to_bounds = task.clip_to_bounds;
    let task_bounds = crate::types::Bounds {
        west:  task.bounds_west,
        east:  task.bounds_east,
        south: task.bounds_south,
        north: task.bounds_north,
    };
    // 解析多边形坐标（用于精确裁剪）
    let polygon: Option<Vec<[f64; 2]>> = task.polygon_wgs84
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok());
    // 解析数据源以获取 CRS
    let crs = serde_json::from_str::<crate::types::TileSource>(&task.source_config)
        .map(|s| s.crs)
        .unwrap_or_default();

    tokio::task::spawn_blocking(move || {
        let conn = rusqlite::Connection::open(&path).map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT tile_data FROM tiles WHERE zoom_level=?1 AND tile_column=?2 AND tile_row=?3",
            )
            .map_err(|e| e.to_string())?;
        let data: Option<Vec<u8>> = stmt
            .query_row(rusqlite::params![z, x, y], |row| row.get(0))
            .ok();
        let data = data.ok_or_else(|| "tile not found".to_string())?;

        // 如果瓦片已经在下载后被统一裁剪过，直接返回
        let already_clipped: bool = conn
            .query_row(
                "SELECT value FROM metadata WHERE name='tiles.clipped'",
                [],
                |row| row.get::<_, String>(0),
            )
            .map(|v| v == "1")
            .unwrap_or(false);

        if clip_to_bounds && !already_clipped {
            if let Some(ref poly) = polygon {
                // 多边形裁剪
                crate::export::tile_clip::clip_tile_to_polygon_crs(
                    &data, x as u32, y as u32, z as u8, poly, &crs,
                )
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "tile not found".to_string())
            } else {
                crate::export::tile_clip::clip_tile_to_bounds_crs(
                    &data, x as u32, y as u32, z as u8, &task_bounds, &crs,
                )
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "tile not found".to_string())
            }
        } else {
            Ok(data)
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 生成任务瓦片数据的方形缩略图（PNG 字节），用于发布面板预览。
///
/// 自动选择合适的缩放级别，合成多张瓦片为一张覆盖完整范围的方形图像。
#[tauri::command]
pub async fn get_task_thumbnail(
    task_id: String,
    size: u32,
    app_db: State<'_, AppDb>,
) -> Result<Vec<u8>, String> {
    let task = app_db.get_task(&task_id).map_err(|e| e.to_string())?;
    let path = task
        .tile_store_path
        .clone()
        .ok_or("tile_store_path not set")?;
    let bounds = crate::types::Bounds {
        west:  task.bounds_west,
        east:  task.bounds_east,
        south: task.bounds_south,
        north: task.bounds_north,
    };
    let crs = serde_json::from_str::<crate::types::TileSource>(&task.source_config)
        .map(|s| s.crs)
        .unwrap_or_default();
    let min_zoom = task.min_zoom as u8;
    let max_zoom = task.max_zoom as u8;
    let size = size.max(64).min(512);

    tokio::task::spawn_blocking(move || {
        generate_thumbnail(&path, &bounds, &crs, min_zoom, max_zoom, size)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn generate_thumbnail(
    store_path: &str,
    bounds: &crate::types::Bounds,
    crs: &crate::types::CrsType,
    min_zoom: u8,
    max_zoom: u8,
    size: u32,
) -> Result<Vec<u8>, String> {
    use crate::tile_math::{bounds_to_tile_range_xyz, tile_to_lonlat_bounds};
    use image::{imageops, RgbaImage};
    use std::f64::consts::PI;

    let conn = rusqlite::Connection::open(store_path).map_err(|e| e.to_string())?;

    // 选择一个瓦片数在 1~64 且尽量多的缩放级别
    let mut chosen_zoom = min_zoom;
    for z in min_zoom..=max_zoom {
        let ((x_min, x_max), (y_min, y_max)) = bounds_to_tile_range_xyz(bounds, z);
        let cols = (x_max - x_min + 1) as u64;
        let rows = (y_max - y_min + 1) as u64;
        chosen_zoom = z;
        if cols * rows > 64 {
            // 太多了，用前一个；如果是第一个就用当前
            if z > min_zoom {
                chosen_zoom = z - 1;
            }
            break;
        }
    }

    let ((x_min, x_max), (y_min, y_max)) = bounds_to_tile_range_xyz(bounds, chosen_zoom);
    let cols = (x_max - x_min + 1) as u32;
    let rows = (y_max - y_min + 1) as u32;

    // 合成底图
    let mut composite = RgbaImage::new(cols * 256, rows * 256);

    {
        let mut stmt = conn
            .prepare(
                "SELECT tile_data FROM tiles WHERE zoom_level=?1 AND tile_column=?2 AND tile_row=?3",
            )
            .map_err(|e| e.to_string())?;

        for tile_x in x_min..=x_max {
            for tile_y in y_min..=y_max {
                let data: Option<Vec<u8>> = stmt
                    .query_row(
                        rusqlite::params![chosen_zoom as i64, tile_x as i64, tile_y as i64],
                        |row| row.get(0),
                    )
                    .ok();
                if let Some(data) = data {
                    if let Ok(img) = image::load_from_memory(&data) {
                        let px = (tile_x - x_min) as i64 * 256;
                        let py = (tile_y - y_min) as i64 * 256;
                        imageops::overlay(&mut composite, &img.to_rgba8(), px, py);
                    }
                }
            }
        }
    }

    // 计算裁剪区域：将 bounds 映射到 composite 像素坐标
    let total_w = composite.width() as f64;
    let total_h = composite.height() as f64;

    // 瓦片网格覆盖的地理范围
    let grid_bounds = tile_to_lonlat_bounds(x_min, y_min, chosen_zoom, crs);
    let grid_bounds_se = tile_to_lonlat_bounds(x_max, y_max, chosen_zoom, crs);
    let grid_west = grid_bounds.west;
    let grid_north = grid_bounds.north;
    let grid_east = grid_bounds_se.east;
    let grid_south = grid_bounds_se.south;

    // 经度方向线性映射
    let crop_x = ((bounds.west - grid_west) / (grid_east - grid_west) * total_w)
        .round().max(0.0) as u32;
    let crop_x2 = ((bounds.east - grid_west) / (grid_east - grid_west) * total_w)
        .round().min(total_w) as u32;

    // 纬度方向：用 Mercator 投影 (north→上) 映射到像素
    let merc = |lat: f64| -> f64 {
        match crs {
            crate::types::CrsType::Wgs84 => {
                // WGS84: 线性
                (90.0 - lat) / 180.0
            }
            _ => {
                // WebMercator
                let lat_rad = lat.to_radians();
                (1.0 - (lat_rad.tan() + 1.0 / lat_rad.cos()).ln() / PI) / 2.0
            }
        }
    };
    let grid_merc_top = merc(grid_north);
    let grid_merc_bottom = merc(grid_south);
    let crop_y = ((merc(bounds.north) - grid_merc_top) / (grid_merc_bottom - grid_merc_top) * total_h)
        .round().max(0.0) as u32;
    let crop_y2 = ((merc(bounds.south) - grid_merc_top) / (grid_merc_bottom - grid_merc_top) * total_h)
        .round().min(total_h) as u32;

    let crop_w = (crop_x2.saturating_sub(crop_x)).max(1);
    let crop_h = (crop_y2.saturating_sub(crop_y)).max(1);

    let cropped = imageops::crop_imm(&composite, crop_x, crop_y, crop_w, crop_h).to_image();

    // 缩放到正方形：保持宽高比，居中放置在透明背景上
    let scale = size as f64 / cropped.width().max(cropped.height()) as f64;
    let new_w = (cropped.width() as f64 * scale).round() as u32;
    let new_h = (cropped.height() as f64 * scale).round() as u32;
    let resized = imageops::resize(&cropped, new_w, new_h, imageops::FilterType::Lanczos3);

    let mut square = RgbaImage::new(size, size);
    let offset_x = (size - new_w) / 2;
    let offset_y = (size - new_h) / 2;
    imageops::overlay(&mut square, &resized, offset_x as i64, offset_y as i64);

    // 编码为 PNG
    let mut buf = std::io::Cursor::new(Vec::new());
    square
        .write_to(&mut buf, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(buf.into_inner())
}

/// 返回瓦片存储根目录
/// 读取设置中的 app.tiles_dir；若为空，则使用用户文档目录下的 御图/tiles（可写，非安装目录）
fn get_tiles_dir(app: &AppHandle, app_db: &AppDb) -> Result<PathBuf, String> {
    let custom = app_db
        .get_setting("app.tiles_dir")
        .ok()
        .flatten()
        .filter(|s| !s.trim().is_empty());
    if let Some(dir) = custom {
        return Ok(PathBuf::from(dir));
    }
    // 优先使用"文档/御图/tiles"，方便用户找到；若系统不支持则回退到 AppLocalData
    let base = app
        .path()
        .document_dir()
        .or_else(|_| app.path().app_local_data_dir())
        .map_err(|e| format!("cannot get user data dir: {}", e))?;
    Ok(base.join("御图").join("tiles"))
}

// ─── 系统集成 ─────────────────────────────────────────────────────────────────

/// 在系统文件管理器中显示并选中文件或文件夹（Windows: explorer /select, macOS: open -R）
#[tauri::command]
pub async fn reveal_in_explorer(path: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        // 将正斜杠统一为反斜杠；用 raw_arg 避免 Rust 对路径二次转义
        // explorer 语法：explorer /select,"C:\path with spaces\file"
        let normalized = path.replace('/', "\\");
        std::process::Command::new("explorer")
            .raw_arg(format!("/select,\"{}\"", normalized))
            .spawn()
            .map_err(|e| format!("打开文件管理器失败: {}", e))?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .args(["-R", &path])
            .spawn()
            .map_err(|e| format!("打开 Finder 失败: {}", e))?;
    }
    #[cfg(target_os = "linux")]
    {
        // 尝试 xdg-open 打开父目录
        let parent = std::path::Path::new(&path)
            .parent()
            .and_then(|p| p.to_str())
            .unwrap_or(&path)
            .to_string();
        std::process::Command::new("xdg-open")
            .arg(&parent)
            .spawn()
            .map_err(|e| format!("打开文件管理器失败: {}", e))?;
    }
    Ok(())
}

// ─── 任务导入/导出 ──────────────────────────────────────────────────────────

/// 将任务打包导出为 .tgr 文件（v2：直接复制 SQLite + 写入 metadata，零压缩开销）
///
/// 旧版（v1）使用 ZIP 格式，已废弃，仅保留导入兼容。
#[tauri::command]
pub async fn export_task(
    task_id: String,
    dest_path: String,
    app_db: State<'_, AppDb>,
) -> Result<(), String> {
    let task = app_db.get_task(&task_id).map_err(|e| e.to_string())?;

    let tile_path = task
        .tile_store_path
        .as_deref()
        .ok_or("该任务尚无瓦片存储文件（可能未开始下载）")?;

    if !std::path::Path::new(tile_path).exists() {
        return Err(format!("瓦片文件不存在: {}", tile_path));
    }

    // 直接复制 SQLite 文件（O(n) 磁盘读写，无压缩/解压开销）
    std::fs::copy(tile_path, &dest_path)
        .map_err(|e| format!("无法复制瓦片文件: {}", e))?;

    // 在副本中写入任务元数据
    let store = crate::storage::tile_store::TileStore::open(
        std::path::Path::new(&dest_path),
        &task_id,
    )
    .map_err(|e| e.to_string())?;

    store
        .write_meta(&[
            ("tgr.version", "2"),
            ("tgr.name", &task.name),
            ("tgr.source_config", &task.source_config),
            ("tgr.bounds_west", &task.bounds_west.to_string()),
            ("tgr.bounds_east", &task.bounds_east.to_string()),
            ("tgr.bounds_south", &task.bounds_south.to_string()),
            ("tgr.bounds_north", &task.bounds_north.to_string()),
            ("tgr.min_zoom", &task.min_zoom.to_string()),
            ("tgr.max_zoom", &task.max_zoom.to_string()),
            ("tgr.clip_to_bounds", if task.clip_to_bounds { "1" } else { "0" }),
            (
                "tgr.polygon_wgs84",
                task.polygon_wgs84.as_deref().unwrap_or(""),
            ),
            ("tgr.total_tiles", &task.total_tiles.to_string()),
            ("tgr.downloaded_tiles", &task.downloaded_tiles.to_string()),
            ("tgr.failed_tiles", &task.failed_tiles.to_string()),
        ])
        .map_err(|e| e.to_string())?;

    // 将导出文件中失败的瓦片重置为 pending（跨机器可继续下载）
    store.reset_failed().map_err(|e| e.to_string())?;

    Ok(())
}

/// 从 .tgr 文件导入任务（状态置为 paused，可在本机继续下载）
///
/// 自动检测文件格式：
/// - v2（SQLite）：直接以原路径注册任务，零拷贝
/// - v1（ZIP，旧版兼容）：解压 tiles.db 到 tiles_dir 后注册
#[tauri::command]
pub async fn import_task(
    src_path: String,
    app_db: State<'_, AppDb>,
    app: AppHandle,
) -> Result<String, String> {
    use std::io::Read;

    // 读取文件头 4 字节判断格式
    let mut header = [0u8; 4];
    {
        let mut f = std::fs::File::open(&src_path)
            .map_err(|e| format!("无法打开文件: {}", e))?;
        f.read_exact(&mut header)
            .map_err(|_| "文件太小或无法读取".to_string())?;
    }

    // ZIP magic: 50 4B 03 04 → v1 旧格式
    let is_zip = header == [0x50, 0x4B, 0x03, 0x04];
    // SQLite magic: 53 51 4C 69 ("SQLi") → v2 新格式
    let is_sqlite = header[0..4] == [0x53, 0x51, 0x4C, 0x69];

    if is_sqlite {
        // ── v2：直接注册，零拷贝 ─────────────────────────────────────────────
        let store = crate::storage::tile_store::TileStore::open(
            std::path::Path::new(&src_path),
            "import_probe",
        )
        .map_err(|e| format!("无法打开瓦片文件: {}", e))?;

        let meta = store.read_meta().map_err(|e| e.to_string())?;

        let get = |k: &str| -> Result<String, String> {
            meta.get(k)
                .cloned()
                .ok_or_else(|| format!("缺少元数据字段: {}", k))
        };

        let name = get("tgr.name")?;
        let source_config = get("tgr.source_config")?;
        let bounds_west: f64 = get("tgr.bounds_west")?.parse().map_err(|e: std::num::ParseFloatError| e.to_string())?;
        let bounds_east: f64 = get("tgr.bounds_east")?.parse().map_err(|e: std::num::ParseFloatError| e.to_string())?;
        let bounds_south: f64 = get("tgr.bounds_south")?.parse().map_err(|e: std::num::ParseFloatError| e.to_string())?;
        let bounds_north: f64 = get("tgr.bounds_north")?.parse().map_err(|e: std::num::ParseFloatError| e.to_string())?;
        let min_zoom: u8 = get("tgr.min_zoom")?.parse().map_err(|e: std::num::ParseIntError| e.to_string())?;
        let max_zoom: u8 = get("tgr.max_zoom")?.parse().map_err(|e: std::num::ParseIntError| e.to_string())?;
        let clip_to_bounds = meta.get("tgr.clip_to_bounds").map(|v| v == "1").unwrap_or(false);
        let polygon_wgs84 = meta
            .get("tgr.polygon_wgs84")
            .filter(|s| !s.is_empty())
            .cloned();
        let total_tiles: i64 = meta.get("tgr.total_tiles").and_then(|v| v.parse().ok()).unwrap_or(0);
        let downloaded_tiles: i64 = meta.get("tgr.downloaded_tiles").and_then(|v| v.parse().ok()).unwrap_or(0);
        let failed_tiles: i64 = meta.get("tgr.failed_tiles").and_then(|v| v.parse().ok()).unwrap_or(0);

        let new_id = Uuid::new_v4().to_string();
        let new_task = NewTask {
            name,
            source_config,
            bounds_west,
            bounds_east,
            bounds_south,
            bounds_north,
            min_zoom,
            max_zoom,
            clip_to_bounds,
            polygon_wgs84,
        };

        // tile_store_path 直接指向用户选择的原始 .tgr 文件
        app_db
            .create_task(&new_id, &new_task, &src_path)
            .map_err(|e| e.to_string())?;
        app_db.update_task_total(&new_id, total_tiles).map_err(|e| e.to_string())?;
        app_db.update_task_progress(&new_id, downloaded_tiles, failed_tiles).map_err(|e| e.to_string())?;
        app_db.update_task_status(&new_id, "paused").map_err(|e| e.to_string())?;

        Ok(new_id)
    } else if is_zip {
        // ── v1 兼容：解压 tiles.db 后注册 ───────────────────────────────────
        let file = std::fs::File::open(&src_path)
            .map_err(|e| format!("无法打开文件: {}", e))?;
        let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;

        let task: crate::storage::app_db::Task = {
            let mut entry = archive
                .by_name("task.json")
                .map_err(|_| "任务文件损坏：缺少 task.json".to_string())?;
            let mut json = String::new();
            entry.read_to_string(&mut json).map_err(|e| e.to_string())?;
            serde_json::from_str(&json).map_err(|e| format!("任务元数据解析失败: {}", e))?
        };

        let new_id = Uuid::new_v4().to_string();
        let tiles_dir = get_tiles_dir(&app, app_db.inner())?;
        std::fs::create_dir_all(&tiles_dir).map_err(|e| e.to_string())?;
        let new_tile_path = tiles_dir.join(format!("{}.tiles", new_id));

        if let Ok(mut tile_entry) = archive.by_name("tiles.db") {
            let mut dest = std::fs::File::create(&new_tile_path)
                .map_err(|e| format!("无法写入瓦片文件: {}", e))?;
            std::io::copy(&mut tile_entry, &mut dest).map_err(|e| e.to_string())?;
        }

        let new_task = NewTask {
            name: task.name,
            source_config: task.source_config,
            bounds_west: task.bounds_west,
            bounds_east: task.bounds_east,
            bounds_south: task.bounds_south,
            bounds_north: task.bounds_north,
            min_zoom: task.min_zoom,
            max_zoom: task.max_zoom,
            clip_to_bounds: task.clip_to_bounds,
            polygon_wgs84: task.polygon_wgs84,
        };
        let tile_path_str = new_tile_path.to_string_lossy().to_string();
        app_db.create_task(&new_id, &new_task, &tile_path_str).map_err(|e| e.to_string())?;
        app_db.update_task_total(&new_id, task.total_tiles).map_err(|e| e.to_string())?;
        app_db.update_task_progress(&new_id, task.downloaded_tiles, task.failed_tiles).map_err(|e| e.to_string())?;
        app_db.update_task_status(&new_id, "paused").map_err(|e| e.to_string())?;

        Ok(new_id)
    } else {
        Err("不支持的文件格式（既不是 .tgr v2 也不是旧版 ZIP）".to_string())
    }
}

/// 返回任务在指定层级的瓦片下载状态 GeoJSON（用于地图进度可视化）
///
/// 返回 GeoJSON FeatureCollection，每个 Feature 对应一个瓦片，
/// properties.status = "downloaded" | "pending" | "failed"
/// 自动选择合适层级（从 minZoom 开始，瓦片数 ≤ 2000）。
/// zoom 传 0 时自动选择，传非零值时强制使用该层级。
#[tauri::command]
pub async fn get_download_progress_geojson(
    task_id: String,
    app_db: State<'_, AppDb>,
) -> Result<String, String> {
    use crate::tile_math::{bounds_to_tile_range_xyz, tile_xyz_to_bounds};
    use crate::types::{Bounds, CrsType, TileSource};

    let task = app_db.get_task(&task_id).map_err(|e| e.to_string())?;
    let tile_store_path = task
        .tile_store_path
        .as_deref()
        .ok_or("tile_store_path not set")?
        .to_owned();

    // 解析 CRS（预留给后续 WGS84 支持）
    let _crs = serde_json::from_str::<TileSource>(&task.source_config)
        .map(|s| s.crs)
        .unwrap_or(CrsType::WebMercator);

    let bounds = Bounds {
        west: task.bounds_west,
        east: task.bounds_east,
        south: task.bounds_south,
        north: task.bounds_north,
    };

    // 找到瓦片数 ≤ 2000 的最合适层级（从 minZoom 起）
    let mut chosen_zoom = task.min_zoom;
    for z in task.min_zoom..=task.max_zoom {
        let ((x_min, x_max), (y_min, y_max)) = bounds_to_tile_range_xyz(&bounds, z);
        let count = ((x_max as u64).saturating_sub(x_min as u64) + 1)
            * ((y_max as u64).saturating_sub(y_min as u64) + 1);
        chosen_zoom = z;
        if count <= 2000 {
            break;
        }
    }

    let zoom = chosen_zoom;
    let ((x_min, x_max), (y_min, y_max)) = bounds_to_tile_range_xyz(&bounds, zoom);

    // 从 tile store 查询已下载的坐标
    let downloaded_set: std::collections::HashSet<(u32, u32)> =
        tokio::task::spawn_blocking(move || {
            let conn = rusqlite::Connection::open(&tile_store_path)
                .map_err(|e| e.to_string())?;
            let mut stmt = conn
                .prepare(
                    "SELECT tile_column, tile_row FROM download_state
                     WHERE zoom_level=?1 AND status='downloaded'",
                )
                .map_err(|e| e.to_string())?;
            let coords = stmt
                .query_map(rusqlite::params![zoom as i64], |row| {
                    Ok((row.get::<_, i64>(0)? as u32, row.get::<_, i64>(1)? as u32))
                })
                .map_err(|e| e.to_string())?
                .filter_map(|r| r.ok())
                .collect();
            Ok::<_, String>(coords)
        })
        .await
        .map_err(|e| e.to_string())??;

    // 生成 GeoJSON
    let mut features: Vec<String> = Vec::new();
    for y in y_min..=y_max {
        for x in x_min..=x_max {
            let tb = tile_xyz_to_bounds(x, y, zoom);
            let status = if downloaded_set.contains(&(x, y)) {
                "downloaded"
            } else {
                "pending"
            };
            let feature = format!(
                r#"{{"type":"Feature","properties":{{"status":"{}"}},"geometry":{{"type":"Polygon","coordinates":[[{},{},{},{},{}]]}}}}"#,
                status,
                format!("[{:.6},{:.6}]", tb.west, tb.north),
                format!("[{:.6},{:.6}]", tb.east, tb.north),
                format!("[{:.6},{:.6}]", tb.east, tb.south),
                format!("[{:.6},{:.6}]", tb.west, tb.south),
                format!("[{:.6},{:.6}]", tb.west, tb.north),
            );
            features.push(feature);
        }
    }

    let geojson = format!(
        r#"{{"type":"FeatureCollection","features":[{}]}}"#,
        features.join(",")
    );
    Ok(geojson)
}
