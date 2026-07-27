//! TileGrabber — 任务管理 Tauri 命令
//!
//! 提供给前端的任务 CRUD + 下载引擎控制接口。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::{Arc, PoisonError};

use fs2::available_space as fs2_available_space;
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

use crate::download::engine::DownloadEngine;
use crate::storage::app_db::{AppDb, CompletedTaskPreview, LogEntry, NewTask, Task};

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
    pub status: String, // "running" | "done" | "error" | "cancelled"
    pub error: Option<String>,
}

pub type ExportState =
    std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, ExportJob>>>;

/// 每个活跃导出任务对应的取消令牌（job_id → AtomicBool）
pub type CancelMap =
    std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, Arc<AtomicBool>>>>;

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
const MAX_CONCURRENCY: usize = 64;

/// 从 AppDb 读取用户配置的并发数
fn get_concurrency(app_db: &AppDb) -> usize {
    app_db
        .get_setting("download.concurrency")
        .ok()
        .flatten()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .map(|n| n.min(MAX_CONCURRENCY))
        .unwrap_or(DEFAULT_CONCURRENCY)
}

/// 从 AppDb 读取最大并发任务数（0 = 不限制）
fn get_max_concurrent_tasks(app_db: &AppDb) -> usize {
    app_db
        .get_setting("tasks.max_concurrent")
        .ok()
        .flatten()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0)
}

/// 检查是否超出最大并发任务数限制。超出时返回 Err，否则返回 Ok。
fn check_concurrent_limit(engine: &DownloadEngine, app_db: &AppDb) -> Result<(), String> {
    let max = get_max_concurrent_tasks(app_db);
    if max == 0 {
        return Ok(()); // 不限制
    }
    let running = engine.active_download_count();
    if running >= max {
        Err(format!(
            "已达到最大并发任务数限制（{}/{}），请等待当前任务完成后再启动",
            running, max
        ))
    } else {
        Ok(())
    }
}

// ─── 磁盘空间预检 ────────────────────────────────────────────────────────────

/// 磁盘空间预检结果
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskSpaceInfo {
    /// 预计还需要下载的瓦片数
    pub pending_tiles: u64,
    /// 预计需要的字节数（30 KB/瓦片保守估算）
    pub estimated_bytes: u64,
    /// 瓦片存储目录所在磁盘的可用字节数
    pub available_bytes: u64,
    /// 是否有足够空间
    pub sufficient: bool,
}

/// 每个瓦片的保守空间估算（字节）—— 30 KB 覆盖绝大多数 PNG/JPEG/WebP 场景
const BYTES_PER_TILE_ESTIMATE: u64 = 30 * 1024;

/// 查询指定任务的磁盘空间预检信息
#[tauri::command]
pub async fn check_disk_space(
    task_id: String,
    app_db: State<'_, AppDb>,
    app: AppHandle,
) -> Result<DiskSpaceInfo, String> {
    use crate::tile_math::count_tiles;
    use crate::types::{Bounds, CrsType};

    let task = app_db.get_task(&task_id).map_err(|e| e.to_string())?;

    // 计算待下载瓦片数
    let pending_tiles: u64 = if task.total_tiles > 0 {
        // 任务已初始化：用实际剩余量
        let pending = task.total_tiles - task.downloaded_tiles - task.failed_tiles;
        pending.max(0) as u64
    } else {
        // 任务尚未启动：按范围 + 层级估算总瓦片数
        let source: serde_json::Value = serde_json::from_str(&task.source_config)
            .unwrap_or(serde_json::Value::Null);
        let crs_str = source
            .get("crs")
            .and_then(|v| v.as_str())
            .unwrap_or("WebMercator");
        let crs = if crs_str.contains("84") || crs_str.contains("4326") {
            CrsType::Wgs84
        } else {
            CrsType::WebMercator
        };
        let bounds = Bounds {
            west: task.bounds_west,
            east: task.bounds_east,
            south: task.bounds_south,
            north: task.bounds_north,
        };
        count_tiles(&bounds, task.min_zoom as u8, task.max_zoom as u8, &crs).total
    };

    let estimated_bytes = pending_tiles * BYTES_PER_TILE_ESTIMATE;

    // 获取瓦片存储目录的磁盘可用空间
    let tiles_dir = get_tiles_dir(&app, app_db.inner())?;
    std::fs::create_dir_all(&tiles_dir).ok();

    let available_bytes = fs2_available_space(&tiles_dir)
        .map_err(|e| format!("无法获取磁盘可用空间: {}", e))?;

    Ok(DiskSpaceInfo {
        pending_tiles,
        estimated_bytes,
        available_bytes,
        sufficient: available_bytes >= estimated_bytes,
    })
}

/// 在任务创建前估算下载规模（瓦片数量 + 预估字节数）
#[tauri::command]
pub async fn estimate_download(
    west: f64,
    east: f64,
    south: f64,
    north: f64,
    min_zoom: i32,
    max_zoom: i32,
    crs_str: String,
) -> Result<EstimateInfo, String> {
    use crate::tile_math::count_tiles;
    use crate::types::{Bounds, CrsType};

    let crs = if crs_str.contains("84") || crs_str.contains("4326") {
        CrsType::Wgs84
    } else {
        CrsType::WebMercator
    };
    let bounds = Bounds { west, east, south, north };
    let result = count_tiles(&bounds, min_zoom as u8, max_zoom as u8, &crs);
    let tile_count = result.total;
    let estimated_bytes = tile_count * BYTES_PER_TILE_ESTIMATE;

    Ok(EstimateInfo {
        tile_count,
        estimated_bytes,
    })
}

#[derive(serde::Serialize)]
pub struct EstimateInfo {
    pub tile_count: u64,
    pub estimated_bytes: u64,
}


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

/// 查找与给定数据源匹配的最新已完成任务，返回其 ID 和下载范围/层级信息。
///
/// 匹配条件：url_template + coord_type + kind 三者均相同。
/// 主要用途：GCJ02 来源在完成下载并合成纠偏后，前端可改用本地存储瓦片替代在线拉取。
#[tauri::command]
pub fn find_completed_task_for_source(
    url_template: String,
    coord_type: String,
    kind: String,
    app_db: State<'_, AppDb>,
) -> Option<CompletedTaskPreview> {
    app_db
        .find_completed_task_for_source(&url_template, &coord_type, &kind)
        .unwrap_or(None)
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

    // 新任务启动前检查并发上限
    check_concurrent_limit(&engine, app_db.inner())?;

    let concurrency = get_concurrency(app_db.inner());

    engine
        .start(task_id, app_db.inner().clone(), concurrency, app)
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
        // 已在运行，发送 Run 信号解除暂停（paused→run 不计入新任务，不检查上限）
        return engine.resume(&task_id).map_err(|e| e.to_string());
    }

    // 引擎无记录（应用重启后重新启动），需检查并发上限
    check_concurrent_limit(&engine, app_db.inner())?;

    let concurrency = get_concurrency(app_db.inner());
    engine
        .start(task_id, app_db.inner().clone(), concurrency, app)
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

    // 重试也需检查并发上限
    check_concurrent_limit(&engine, app_db.inner())?;

    let concurrency = get_concurrency(app_db.inner());
    engine
        .start(task_id, app_db.inner().clone(), concurrency, app)
        .map_err(|e| e.to_string())?;

    Ok(count)
}

// ─── F2 / F4：增量下载相关 ──────────────────────────────────────────────

/// F2 扩展任务参数（前端传入）。
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateTaskGeometryArgs {
    pub task_id: String,
    pub bounds_west: f64,
    pub bounds_east: f64,
    pub bounds_south: f64,
    pub bounds_north: f64,
    pub min_zoom: u8,
    pub max_zoom: u8,
    /// 可选的新多边形 JSON（[[lng, lat], ...]）。若为 None 且原任务有多边形，将保留原多边形。
    pub polygon_wgs84: Option<String>,
}

/// F2 扩展任务（仅允许扩大）：合并 bounds、扩展 zoom 范围，重新枚举瓦片并 INSERT OR IGNORE。
/// 返回 (新增 pending 瓦片数, 任务当前 total_tiles)。
#[tauri::command]
pub async fn update_task_geometry(
    args: UpdateTaskGeometryArgs,
    app_db: State<'_, AppDb>,
) -> Result<(i64, i64), String> {
    let task = app_db.get_task(&args.task_id).map_err(|e| e.to_string())?;

    // 仅允许扩大 zoom 范围
    if args.min_zoom > task.min_zoom {
        return Err(format!(
            "最小缩放级别只能扩大或保持（当前 {}，新值 {}）",
            task.min_zoom, args.min_zoom
        ));
    }
    if args.max_zoom < task.max_zoom {
        return Err(format!(
            "最大缩放级别只能扩大或保持（当前 {}，新值 {}）",
            task.max_zoom, args.max_zoom
        ));
    }

    // bounds 取并集
    let new_west = args.bounds_west.min(task.bounds_west);
    let new_east = args.bounds_east.max(task.bounds_east);
    let new_south = args.bounds_south.min(task.bounds_south);
    let new_north = args.bounds_north.max(task.bounds_north);

    // 多边形：若前端未提供且原有，则保留；若前端提供则替换（暂不支持真正的 polygon union，
    // 此简化策略下重新枚举使用 bbox，因此即使保留多边形也只影响后续 UI 展示）
    let new_polygon = args.polygon_wgs84.as_deref().or(task.polygon_wgs84.as_deref());

    app_db
        .update_task_geometry(
            &args.task_id,
            new_west,
            new_east,
            new_south,
            new_north,
            args.min_zoom,
            args.max_zoom,
            new_polygon,
        )
        .map_err(|e| e.to_string())?;

    // 解析 TileSource，取 CRS
    let source: crate::types::TileSource =
        serde_json::from_str(&task.source_config).map_err(|e| e.to_string())?;
    let bounds = crate::types::Bounds {
        west: new_west,
        east: new_east,
        south: new_south,
        north: new_north,
    };
    let tiles = crate::tile_math::enumerate_tiles(
        &bounds,
        args.min_zoom,
        args.max_zoom,
        &source.crs,
        Some(50_000_000),
    );

    let path = task
        .tile_store_path
        .as_deref()
        .ok_or("tile store path not set")?;
    let tile_store =
        crate::storage::tile_store::TileStore::open(std::path::Path::new(path), &args.task_id)
            .map_err(|e| e.to_string())?;

    let progress_before = tile_store.get_progress().map_err(|e| e.to_string())?;
    let total_after = tile_store
        .init_download_state(&tiles)
        .map_err(|e| e.to_string())?;
    let added = total_after - progress_before.total;

    app_db
        .update_task_total(&args.task_id, total_after)
        .map_err(|e| e.to_string())?;

    Ok((added.max(0), total_after))
}

/// F4 借数据预检：返回源任务可复用的瓦片数量（与目标 pending/failed 瓦片的交集）。
#[tauri::command]
pub async fn count_reusable_tiles(
    target_task_id: String,
    source_task_id: String,
    app_db: State<'_, AppDb>,
) -> Result<i64, String> {
    let target = app_db
        .get_task(&target_task_id)
        .map_err(|e| e.to_string())?;
    let source = app_db
        .get_task(&source_task_id)
        .map_err(|e| e.to_string())?;
    validate_same_source(&target, &source)?;

    let target_path = target
        .tile_store_path
        .as_deref()
        .ok_or("target tile store path not set")?;
    let source_path = source
        .tile_store_path
        .as_deref()
        .ok_or("source tile store path not set")?;

    let tile_store = crate::storage::tile_store::TileStore::open(
        std::path::Path::new(target_path),
        &target_task_id,
    )
    .map_err(|e| e.to_string())?;
    tile_store
        .count_reusable_from(std::path::Path::new(source_path))
        .map_err(|e| e.to_string())
}

/// F4 借数据：从源任务的 .tiles 数据库导入目标任务待下载的瓦片。
/// 返回 (imported_count, total_pending_before)。
#[tauri::command]
pub async fn import_tiles_from_source(
    target_task_id: String,
    source_task_id: String,
    app_db: State<'_, AppDb>,
) -> Result<(i64, i64), String> {
    let target = app_db
        .get_task(&target_task_id)
        .map_err(|e| e.to_string())?;
    let source = app_db
        .get_task(&source_task_id)
        .map_err(|e| e.to_string())?;
    validate_same_source(&target, &source)?;

    let target_path = target
        .tile_store_path
        .as_deref()
        .ok_or("target tile store path not set")?;
    let source_path = source
        .tile_store_path
        .as_deref()
        .ok_or("source tile store path not set")?;

    let tile_store = crate::storage::tile_store::TileStore::open(
        std::path::Path::new(target_path),
        &target_task_id,
    )
    .map_err(|e| e.to_string())?;
    let (imported, pending_before) = tile_store
        .import_from_external(std::path::Path::new(source_path))
        .map_err(|e| e.to_string())?;

    // 同步 tasks 表进度
    let progress = tile_store.get_progress().map_err(|e| e.to_string())?;
    app_db
        .update_task_progress(&target_task_id, progress.downloaded, progress.failed)
        .map_err(|e| e.to_string())?;

    Ok((imported, pending_before))
}

/// 校验两任务来自同一数据源（host + format + CRS + coord_type）。
fn validate_same_source(target: &Task, source: &Task) -> Result<(), String> {
    let t_src: crate::types::TileSource =
        serde_json::from_str(&target.source_config).map_err(|e| e.to_string())?;
    let s_src: crate::types::TileSource =
        serde_json::from_str(&source.source_config).map_err(|e| e.to_string())?;

    if t_src.crs != s_src.crs {
        return Err("源任务与目标任务的 CRS 不一致，无法直接复用瓦片".into());
    }
    if t_src.coord_type != s_src.coord_type {
        return Err("源任务与目标任务的坐标系（GCJ02/WGS84）不一致".into());
    }
    if t_src.format != s_src.format {
        return Err(format!(
            "瓦片格式不一致（源 {} vs 目标 {}），无法直接复用",
            s_src.format, t_src.format
        ));
    }
    // host 比对：从 URL 模板中提取域名
    let t_host = extract_host(&t_src.url_template);
    let s_host = extract_host(&s_src.url_template);
    if t_host != s_host {
        return Err(format!(
            "数据源域名不一致（源 {} vs 目标 {}），瓦片切片方案可能不同",
            s_host.unwrap_or("?"),
            t_host.unwrap_or("?")
        ));
    }
    Ok(())
}

/// 从 URL 模板提取 host（忽略 {s} 占位符）。
fn extract_host(url_template: &str) -> Option<&str> {
    let start = url_template.find("://")? + 3;
    let rest = &url_template[start..];
    let end = rest.find('/').unwrap_or(rest.len());
    Some(&rest[..end])
}

// ─── E 套件：失败瓦片可视化 ──────────────────────────────────────────────────

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FailedZoomStat {
    pub zoom: i32,
    pub count: i64,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FailedTileCoord {
    pub x: u32,
    pub y: u32,
}

/// E1：返回任务每个 zoom 层级的失败瓦片数量（仅 count>0 的层级）。
#[tauri::command]
pub async fn failed_tiles_summary(
    task_id: String,
    app_db: State<'_, AppDb>,
) -> Result<Vec<FailedZoomStat>, String> {
    let task = app_db.get_task(&task_id).map_err(|e| e.to_string())?;
    let path = task
        .tile_store_path
        .as_deref()
        .ok_or("tile_store_path not set")?;
    let store = crate::storage::tile_store::TileStore::open(std::path::Path::new(path), &task_id)
        .map_err(|e| e.to_string())?;
    let rows = store.failed_summary_by_zoom().map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .map(|(zoom, count)| FailedZoomStat { zoom, count })
        .collect())
}

/// E2：列出指定 zoom 的失败瓦片坐标，供前端在地图上绘制覆盖层。
#[tauri::command]
pub async fn list_failed_tiles(
    task_id: String,
    zoom: i32,
    app_db: State<'_, AppDb>,
) -> Result<Vec<FailedTileCoord>, String> {
    let task = app_db.get_task(&task_id).map_err(|e| e.to_string())?;
    let path = task
        .tile_store_path
        .as_deref()
        .ok_or("tile_store_path not set")?;
    let store = crate::storage::tile_store::TileStore::open(std::path::Path::new(path), &task_id)
        .map_err(|e| e.to_string())?;
    let rows = store.list_failed_tiles(zoom).map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .map(|(x, y)| FailedTileCoord { x, y })
        .collect())
}

/// E3：将失败瓦片重置为 pending 并恢复下载任务。
/// 当 `zoom` 为 None 时重置所有 zoom；否则仅重置该层。
/// 返回被重置的瓦片数量。
#[tauri::command]
pub async fn retry_failed_tiles(
    task_id: String,
    zoom: Option<i32>,
    app_db: State<'_, AppDb>,
    engine: State<'_, DownloadEngine>,
    app: AppHandle,
) -> Result<i64, String> {
    let task = app_db.get_task(&task_id).map_err(|e| e.to_string())?;
    let path = task
        .tile_store_path
        .as_deref()
        .ok_or("tile_store_path not set")?;
    let store = crate::storage::tile_store::TileStore::open(std::path::Path::new(path), &task_id)
        .map_err(|e| e.to_string())?;
    let count = match zoom {
        Some(z) => store.reset_failed_at_zoom(z).map_err(|e| e.to_string())?,
        None => store.reset_failed().map_err(|e| e.to_string())?,
    };
    if count > 0 {
        // 同步 tasks 表进度
        let progress = store.get_progress().map_err(|e| e.to_string())?;
        app_db
            .update_task_progress(&task_id, progress.downloaded, progress.failed)
            .map_err(|e| e.to_string())?;

        // 触发下载：若引擎已有句柄则发 Run 信号；否则按 resume_download 同款逻辑重新 start
        if engine.is_active(&task_id) {
            engine.resume(&task_id).map_err(|e| e.to_string())?;
        } else {
            check_concurrent_limit(&engine, app_db.inner())?;
            let concurrency = get_concurrency(app_db.inner());
            engine
                .start(task_id.clone(), app_db.inner().clone(), concurrency, app)
                .map_err(|e| e.to_string())?;
        }
    }
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
    jpeg_quality: Option<u8>,
    png_level: Option<u8>,
    app_db: State<'_, AppDb>,
    export_state: State<'_, ExportState>,
    cancel_map: State<'_, CancelMap>,
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
    let bounds = [
        task.bounds_west,
        task.bounds_south,
        task.bounds_east,
        task.bounds_north,
    ];
    let polygon: Option<Vec<[f64; 2]>> = task
        .polygon_wgs84
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok());
    let crs = serde_json::from_str::<crate::types::TileSource>(&task.source_config)
        .map(|s| s.crs)
        .unwrap_or_default();
    let task_name = task.name.clone();

    let job_id = Uuid::new_v4().to_string();
    let cancel_token = Arc::new(AtomicBool::new(false));
    cancel_map.lock().unwrap_or_else(PoisonError::into_inner).insert(job_id.clone(), Arc::clone(&cancel_token));
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
    export_state.lock().unwrap_or_else(PoisonError::into_inner).insert(job_id.clone(), job);

    let state_clone = export_state.inner().clone();
    let cancel_map_clone = cancel_map.inner().clone();
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
            jpeg_quality,
            png_level,
            &cancel_token,
            |done, total| {
                if let Ok(mut map) = state_clone.lock() {
                    if let Some(j) = map.get_mut(&jid) {
                        j.done = done;
                        j.total = total;
                    }
                }
                let _ = app_clone.emit(
                    "export-progress",
                    ExportProgressPayload {
                        job_id: jid.clone(),
                        done,
                        total,
                        status: "running".into(),
                        dest_path: dest_path.clone(),
                        error: None,
                    },
                );
            },
        );
        cancel_map_clone.lock().unwrap_or_else(PoisonError::into_inner).remove(&jid);
        let cancelled = matches!(&result, Err(e) if e.to_string().contains("__cancelled__"));
        let (status, error, done): (String, Option<String>, u64) = match result {
            Ok(n) => ("done".into(), None, n),
            Err(_) if cancelled => ("cancelled".into(), None, 0),
            Err(e) => ("error".into(), Some(e.to_string()), 0),
        };
        if let Ok(mut map) = state_clone.lock() {
            if let Some(j) = map.get_mut(&jid) {
                j.status = status.clone();
                j.error = error.clone();
                if done > 0 {
                    j.done = done;
                }
            }
        }
        let _ = app_clone.emit(
            "export-progress",
            ExportProgressPayload {
                job_id: jid.clone(),
                done,
                total: done,
                status,
                dest_path: dest_path.clone(),
                error,
            },
        );
    });

    Ok(job_id)
}

/// 将任务瓦片包导出为 PMTiles v3 单文件
///
/// 在后台启动导出任务，立即返回 job_id。
/// 导出进度通过 `export-progress` Tauri 事件推送。
#[tauri::command]
pub async fn export_pmtiles(
    task_id: String,
    dest_path: String,
    clip_to_bounds: bool,
    jpeg_quality: Option<u8>,
    png_level: Option<u8>,
    app_db: State<'_, AppDb>,
    export_state: State<'_, ExportState>,
    cancel_map: State<'_, CancelMap>,
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
    let bounds = [
        task.bounds_west,
        task.bounds_south,
        task.bounds_east,
        task.bounds_north,
    ];
    let polygon: Option<Vec<[f64; 2]>> = task
        .polygon_wgs84
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok());
    let crs = serde_json::from_str::<crate::types::TileSource>(&task.source_config)
        .map(|s| s.crs)
        .unwrap_or_default();

    if !matches!(crs, crate::types::CrsType::WebMercator) {
        return Err("PMTiles 仅支持 WebMercator (EPSG:3857) 坐标系；当前任务为 WGS84，请改用 MBTiles 或目录导出".into());
    }

    let task_name = task.name.clone();

    let job_id = Uuid::new_v4().to_string();
    let cancel_token = Arc::new(AtomicBool::new(false));
    cancel_map
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(job_id.clone(), Arc::clone(&cancel_token));
    let job = ExportJob {
        job_id: job_id.clone(),
        task_id: task_id.clone(),
        format: "pmtiles".into(),
        dest_path: dest_path.clone(),
        done: 0,
        total: 0,
        status: "running".into(),
        error: None,
    };
    export_state
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(job_id.clone(), job);

    let state_clone = export_state.inner().clone();
    let cancel_map_clone = cancel_map.inner().clone();
    let app_clone = app.clone();
    let jid = job_id.clone();

    tokio::task::spawn_blocking(move || {
        let result = crate::export::pmtiles::export_pmtiles(
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
            jpeg_quality,
            png_level,
            &cancel_token,
            |done, total| {
                if let Ok(mut map) = state_clone.lock() {
                    if let Some(j) = map.get_mut(&jid) {
                        j.done = done;
                        j.total = total;
                    }
                }
                let _ = app_clone.emit(
                    "export-progress",
                    ExportProgressPayload {
                        job_id: jid.clone(),
                        done,
                        total,
                        status: "running".into(),
                        dest_path: dest_path.clone(),
                        error: None,
                    },
                );
            },
        );
        cancel_map_clone
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&jid);
        let cancelled = matches!(&result, Err(e) if e.to_string().contains("__cancelled__"));
        let (status, error, done): (String, Option<String>, u64) = match result {
            Ok(n) => ("done".into(), None, n),
            Err(_) if cancelled => ("cancelled".into(), None, 0),
            Err(e) => ("error".into(), Some(e.to_string()), 0),
        };
        if let Ok(mut map) = state_clone.lock() {
            if let Some(j) = map.get_mut(&jid) {
                j.status = status.clone();
                j.error = error.clone();
                if done > 0 {
                    j.done = done;
                }
            }
        }
        let _ = app_clone.emit(
            "export-progress",
            ExportProgressPayload {
                job_id: jid.clone(),
                done,
                total: done,
                status,
                dest_path: dest_path.clone(),
                error,
            },
        );
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
    jpeg_quality: Option<u8>,
    png_level: Option<u8>,
    app_db: State<'_, AppDb>,
    export_state: State<'_, ExportState>,
    cancel_map: State<'_, CancelMap>,
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
    let bounds = [
        task.bounds_west,
        task.bounds_south,
        task.bounds_east,
        task.bounds_north,
    ];
    let polygon: Option<Vec<[f64; 2]>> = task
        .polygon_wgs84
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok());
    let crs = serde_json::from_str::<crate::types::TileSource>(&task.source_config)
        .map(|s| s.crs)
        .unwrap_or_default();

    let job_id = Uuid::new_v4().to_string();
    let cancel_token = Arc::new(AtomicBool::new(false));
    cancel_map.lock().unwrap_or_else(PoisonError::into_inner).insert(job_id.clone(), Arc::clone(&cancel_token));
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
    export_state.lock().unwrap_or_else(PoisonError::into_inner).insert(job_id.clone(), job);

    let state_clone = export_state.inner().clone();
    let cancel_map_clone = cancel_map.inner().clone();
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
            jpeg_quality,
            png_level,
            &cancel_token,
            |done, total| {
                if let Ok(mut map) = state_clone.lock() {
                    if let Some(j) = map.get_mut(&jid) {
                        j.done = done;
                        j.total = total;
                    }
                }
                let _ = app_clone.emit(
                    "export-progress",
                    ExportProgressPayload {
                        job_id: jid.clone(),
                        done,
                        total,
                        status: "running".into(),
                        dest_path: dp.clone(),
                        error: None,
                    },
                );
            },
        );
        cancel_map_clone.lock().unwrap_or_else(PoisonError::into_inner).remove(&jid);
        let cancelled = matches!(&result, Err(e) if e.to_string().contains("__cancelled__"));
        let (status, error, done): (String, Option<String>, u64) = match result {
            Ok(n) => ("done".into(), None, n),
            Err(_) if cancelled => ("cancelled".into(), None, 0),
            Err(e) => ("error".into(), Some(e.to_string()), 0),
        };
        if let Ok(mut map) = state_clone.lock() {
            if let Some(j) = map.get_mut(&jid) {
                j.status = status.clone();
                j.error = error.clone();
                if done > 0 {
                    j.done = done;
                }
            }
        }
        let _ = app_clone.emit(
            "export-progress",
            ExportProgressPayload {
                job_id: jid.clone(),
                done,
                total: done,
                status,
                dest_path: dest_dir.clone(),
                error,
            },
        );
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
    compression: Option<String>,
    output_crs: Option<String>,
    app_db: State<'_, AppDb>,
    export_state: State<'_, ExportState>,
    cancel_map: State<'_, CancelMap>,
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

    let bounds = [
        task.bounds_west,
        task.bounds_south,
        task.bounds_east,
        task.bounds_north,
    ];
    let source_crs = serde_json::from_str::<crate::types::TileSource>(&task.source_config)
        .map(|s| s.crs)
        .unwrap_or_default();

    // 解析目标 EPSG 代号：数字字符串直接解析，"auto_utm" 按任务中心经度自动选带
    let target_epsg: Option<u32> = match output_crs.as_deref() {
        None | Some("auto") => None, // 跟随源图层
        Some("auto_utm") => {
            let center_lon = (bounds[0] + bounds[2]) / 2.0;
            let zone = crate::export::utm::zone_from_lon(center_lon);
            let center_lat = (bounds[1] + bounds[3]) / 2.0;
            let epsg = if center_lat >= 0.0 {
                32600 + zone as u32
            } else {
                32700 + zone as u32
            };
            Some(epsg)
        }
        Some(s) => s.parse::<u32>().ok(),
    };

    // 若启用精确裁剪且任务带有多边形，解析顶点用于像素级掩膜
    let polygon: Option<Vec<[f64; 2]>> = if clip_to_bounds {
        task.polygon_wgs84
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
    } else {
        None
    };

    let job_id = Uuid::new_v4().to_string();
    let cancel_token = Arc::new(AtomicBool::new(false));
    cancel_map.lock().unwrap_or_else(PoisonError::into_inner).insert(job_id.clone(), Arc::clone(&cancel_token));
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
    export_state.lock().unwrap_or_else(PoisonError::into_inner).insert(job_id.clone(), job);

    let state_clone = export_state.inner().clone();
    let cancel_map_clone = cancel_map.inner().clone();
    let app_clone = app.clone();
    let jid = job_id.clone();

    tokio::task::spawn_blocking(move || {
        let dp = dest_path.clone();
        let state_clone2 = state_clone.clone();
        let app_clone2 = app_clone.clone();
        let jid2 = jid.clone();
        let dp2 = dp.clone();
        let compression_str = compression.as_deref().unwrap_or("none").to_string();
        let result = crate::export::geotiff::export_geotiff(
            std::path::Path::new(&src_path),
            std::path::Path::new(&dest_path),
            bounds,
            zoom,
            clip_to_bounds,
            polygon,
            &source_crs,
            target_epsg,
            &compression_str,
            &cancel_token,
            move |done, total| {
                if let Ok(mut map) = state_clone2.lock() {
                    if let Some(j) = map.get_mut(&jid2) {
                        j.done = done;
                        j.total = total;
                    }
                }
                let _ = app_clone2.emit(
                    "export-progress",
                    ExportProgressPayload {
                        job_id: jid2.clone(),
                        done,
                        total,
                        status: "running".into(),
                        dest_path: dp2.clone(),
                        error: None,
                    },
                );
            },
        );
        cancel_map_clone.lock().unwrap_or_else(PoisonError::into_inner).remove(&jid);
        let cancelled = matches!(&result, Err(e) if e.to_string().contains("__cancelled__"));
        let (status, error, done): (String, Option<String>, u64) = match result {
            Ok(n) => ("done".into(), None, n),
            Err(_) if cancelled => ("cancelled".into(), None, 0),
            Err(e) => ("error".into(), Some(e.to_string()), 0),
        };
        if let Ok(mut map) = state_clone.lock() {
            if let Some(j) = map.get_mut(&jid) {
                j.status = status.clone();
                j.error = error.clone();
                j.done = done;
                j.total = done;
            }
        }
        let _ = app_clone.emit(
            "export-progress",
            ExportProgressPayload {
                job_id: jid.clone(),
                done,
                total: done,
                status,
                dest_path: dp,
                error,
            },
        );
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

/// 取消指定导出任务（设置取消令牌，导出函数在下一个条带/批次时退出）
#[tauri::command]
pub async fn cancel_export(
    job_id: String,
    cancel_map: State<'_, CancelMap>,
) -> Result<(), String> {
    let map = cancel_map.lock().map_err(|e| e.to_string())?;
    if let Some(token) = map.get(&job_id) {
        token.store(true, AtomicOrdering::Relaxed);
    }
    Ok(())
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
    let path = task.tile_store_path.ok_or("tile_store_path not set")?;

    let clip_to_bounds = task.clip_to_bounds;
    let task_bounds = crate::types::Bounds {
        west: task.bounds_west,
        east: task.bounds_east,
        south: task.bounds_south,
        north: task.bounds_north,
    };
    // 解析多边形坐标（用于精确裁剪）
    let polygon: Option<Vec<[f64; 2]>> = task
        .polygon_wgs84
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
        west: task.bounds_west,
        east: task.bounds_east,
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
        .round()
        .max(0.0) as u32;
    let crop_x2 = ((bounds.east - grid_west) / (grid_east - grid_west) * total_w)
        .round()
        .min(total_w) as u32;

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
    let crop_y = ((merc(bounds.north) - grid_merc_top) / (grid_merc_bottom - grid_merc_top)
        * total_h)
        .round()
        .max(0.0) as u32;
    let crop_y2 = ((merc(bounds.south) - grid_merc_top) / (grid_merc_bottom - grid_merc_top)
        * total_h)
        .round()
        .min(total_h) as u32;

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
pub(crate) fn get_tiles_dir(app: &AppHandle, app_db: &AppDb) -> Result<PathBuf, String> {
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
    std::fs::copy(tile_path, &dest_path).map_err(|e| format!("无法复制瓦片文件: {}", e))?;

    // 在副本中写入任务元数据
    let store =
        crate::storage::tile_store::TileStore::open(std::path::Path::new(&dest_path), &task_id)
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
            (
                "tgr.clip_to_bounds",
                if task.clip_to_bounds { "1" } else { "0" },
            ),
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

// ─── MBTiles 反导入 ───────────────────────────────────────────────────────────

/// 将标准 MBTiles 文件反导入为 TileGrabber 任务（状态置为 completed）。
///
/// - 读取 MBTiles metadata（bounds / minzoom / maxzoom / format / name）
/// - 逐批复制瓦片（TMS y 翻转回 XYZ y）到新建的 `.tiles` 存储
/// - 为所有瓦片写入 download_state = 'downloaded'，使任务可直接导出
/// - 以 ExportJob 机制跟踪进度；完成时 emit `mbtiles-import-done` 事件
///
/// 返回 job_id，前端轮询 `get_export_jobs` 查看进度。
#[tauri::command]
pub async fn import_mbtiles(
    src_path: String,
    task_name: Option<String>,
    app_db: State<'_, AppDb>,
    export_state: State<'_, ExportState>,
    app: AppHandle,
) -> Result<String, String> {
    use rusqlite::{Connection, OpenFlags};
    use std::collections::HashMap;
    use std::io::Read;

    // 验证 SQLite 文件头
    let mut header = [0u8; 4];
    {
        let mut f = std::fs::File::open(&src_path).map_err(|e| format!("无法打开文件: {}", e))?;
        f.read_exact(&mut header)
            .map_err(|_| "文件太小或无法读取".to_string())?;
    }
    if header[0..4] != [0x53, 0x51, 0x4C, 0x69] {
        return Err("不是有效的 MBTiles 文件（非 SQLite 格式）".to_string());
    }

    // 读取 MBTiles metadata
    let mbtiles_meta: HashMap<String, String> = {
        let conn = Connection::open_with_flags(&src_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| format!("无法打开 MBTiles: {}", e))?;
        let mut stmt = conn
            .prepare("SELECT name, value FROM metadata")
            .map_err(|e| e.to_string())?;
        let rows: Vec<(String, String)> = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        rows
    }
    .into_iter()
    .collect();

    // 解析 bounds
    let bounds_str = mbtiles_meta
        .get("bounds")
        .cloned()
        .unwrap_or_else(|| "-180,-85.05,180,85.05".to_string());
    let bp: Vec<f64> = bounds_str
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();
    if bp.len() < 4 {
        return Err("MBTiles bounds 格式无效".to_string());
    }
    let (bounds_west, bounds_south, bounds_east, bounds_north) = (bp[0], bp[1], bp[2], bp[3]);

    let min_zoom: u8 = mbtiles_meta
        .get("minzoom")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let max_zoom: u8 = mbtiles_meta
        .get("maxzoom")
        .and_then(|v| v.parse().ok())
        .unwrap_or(18);
    let format = mbtiles_meta
        .get("format")
        .cloned()
        .unwrap_or_else(|| "png".to_string());
    let name = task_name.unwrap_or_else(|| {
        mbtiles_meta
            .get("name")
            .cloned()
            .unwrap_or_else(|| "Imported MBTiles".to_string())
    });

    // 构造任务记录
    let task_id = Uuid::new_v4().to_string();
    let tiles_dir = get_tiles_dir(&app, app_db.inner())?;
    std::fs::create_dir_all(&tiles_dir).map_err(|e| e.to_string())?;
    let tile_store_path = tiles_dir.join(format!("{}.tiles", task_id));
    let tile_path_str = tile_store_path.to_string_lossy().to_string();

    let source_config = serde_json::json!({
        "kind": "Tms",
        "name": name,
        "url_template": "",
        "url_param_order": [],
        "subdomains": [],
        "format": format,
        "crs": "WebMercator",
        "north_to_south": true,
        "headers": {},
        "extra_params": {},
    })
    .to_string();

    let new_task = NewTask {
        name: name.clone(),
        source_config,
        bounds_west,
        bounds_east,
        bounds_south,
        bounds_north,
        min_zoom,
        max_zoom,
        clip_to_bounds: false,
        polygon_wgs84: None,
    };

    app_db
        .create_task(&task_id, &new_task, &tile_path_str)
        .map_err(|e| e.to_string())?;
    app_db
        .update_task_status(&task_id, "processing")
        .map_err(|e| e.to_string())?;

    // 创建进度跟踪 Job
    let job_id = Uuid::new_v4().to_string();
    let job = ExportJob {
        job_id: job_id.clone(),
        task_id: task_id.clone(),
        format: "mbtiles_import".into(),
        dest_path: tile_path_str.clone(),
        done: 0,
        total: 0,
        status: "running".into(),
        error: None,
    };
    export_state.lock().unwrap_or_else(PoisonError::into_inner).insert(job_id.clone(), job);

    // 后台执行：大文件拷贝放在 spawn_blocking 中
    let state_arc = export_state.inner().clone();
    let app_clone = app.clone();
    let db_clone = app_db.inner().clone();
    let jid = job_id.clone();
    let tid = task_id.clone();

    tokio::task::spawn_blocking(move || {
        let result = mbtiles_import_blocking(&src_path, &tile_path_str, |done, total| {
            if let Ok(mut map) = state_arc.lock() {
                if let Some(j) = map.get_mut(&jid) {
                    j.done = done;
                    j.total = total;
                }
            }
        });

        match result {
            Ok(total) => {
                db_clone.update_task_total(&tid, total as i64).ok();
                db_clone.update_task_progress(&tid, total as i64, 0).ok();
                db_clone.update_task_status(&tid, "completed").ok();
                if let Ok(mut map) = state_arc.lock() {
                    if let Some(j) = map.get_mut(&jid) {
                        j.status = "done".into();
                        j.done = total;
                        j.total = total;
                    }
                }
                let _ = app_clone.emit("mbtiles-import-done", &tid);
            }
            Err(e) => {
                db_clone.update_task_status(&tid, "failed").ok();
                if let Ok(mut map) = state_arc.lock() {
                    if let Some(j) = map.get_mut(&jid) {
                        j.status = "error".into();
                        j.error = Some(e.to_string());
                    }
                }
                let _ = app_clone.emit("mbtiles-import-error", e.to_string());
            }
        }
    });

    Ok(job_id)
}

/// 实际执行 MBTiles → .tiles 拷贝（在 spawn_blocking 内运行）
fn mbtiles_import_blocking<F>(
    src_path: &str,
    dst_path: &str,
    mut progress_cb: F,
) -> anyhow::Result<u64>
where
    F: FnMut(u64, u64),
{
    use rusqlite::{params, Connection, OpenFlags};

    let src = Connection::open_with_flags(src_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    src.execute_batch("PRAGMA cache_size=-65536; PRAGMA mmap_size=268435456;")?;

    let dst = Connection::open(dst_path)?;
    dst.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=OFF;
         PRAGMA cache_size=-200000;
         PRAGMA mmap_size=268435456;
         PRAGMA temp_store=MEMORY;",
    )?;
    dst.execute_batch(
        "CREATE TABLE IF NOT EXISTS metadata (
             name  TEXT PRIMARY KEY,
             value TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS tiles (
             zoom_level  INTEGER NOT NULL,
             tile_column INTEGER NOT NULL,
             tile_row    INTEGER NOT NULL,
             tile_data   BLOB    NOT NULL,
             PRIMARY KEY (zoom_level, tile_column, tile_row)
         );
         CREATE TABLE IF NOT EXISTS download_state (
             zoom_level  INTEGER NOT NULL,
             tile_column INTEGER NOT NULL,
             tile_row    INTEGER NOT NULL,
             status      TEXT    NOT NULL DEFAULT 'downloaded',
             PRIMARY KEY (zoom_level, tile_column, tile_row)
         );",
    )?;

    let total: u64 =
        src.query_row("SELECT COUNT(*) FROM tiles", [], |r| r.get::<_, i64>(0))? as u64;
    if total == 0 {
        return Ok(0);
    }
    progress_cb(0, total);

    const BATCH: i64 = 2000;
    let mut offset: i64 = 0;
    let mut done: u64 = 0;

    loop {
        // 分批读取 MBTiles（避免一次性把整个 DB 装入内存）
        let batch: Vec<(i64, i64, i64, Vec<u8>)> = {
            let mut stmt = src.prepare(
                "SELECT zoom_level, tile_column, tile_row, tile_data
                 FROM tiles
                 ORDER BY zoom_level, tile_column, tile_row
                 LIMIT ?1 OFFSET ?2",
            )?;
            let rows: Vec<(i64, i64, i64, Vec<u8>)> = stmt
                .query_map(params![BATCH, offset], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, Vec<u8>>(3)?,
                    ))
                })?
                .collect::<std::result::Result<_, _>>()?;
            rows
        };

        if batch.is_empty() {
            break;
        }

        let tx = dst.unchecked_transaction()?;
        for (z, x, tms_y, data) in &batch {
            // MBTiles 使用 TMS 坐标（y 从南往北），转换为 XYZ（y 从北往南）
            let xyz_y = (1i64 << z) - 1 - tms_y;
            tx.execute(
                "INSERT OR REPLACE INTO tiles
                 (zoom_level, tile_column, tile_row, tile_data)
                 VALUES (?1, ?2, ?3, ?4)",
                params![z, x, xyz_y, data],
            )?;
            tx.execute(
                "INSERT OR REPLACE INTO download_state
                 (zoom_level, tile_column, tile_row, status)
                 VALUES (?1, ?2, ?3, 'downloaded')",
                params![z, x, xyz_y],
            )?;
        }
        tx.commit()?;

        done += batch.len() as u64;
        offset += batch.len() as i64;
        progress_cb(done, total);
    }

    Ok(done)
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
        let mut f = std::fs::File::open(&src_path).map_err(|e| format!("无法打开文件: {}", e))?;
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
        let bounds_west: f64 = get("tgr.bounds_west")?
            .parse()
            .map_err(|e: std::num::ParseFloatError| e.to_string())?;
        let bounds_east: f64 = get("tgr.bounds_east")?
            .parse()
            .map_err(|e: std::num::ParseFloatError| e.to_string())?;
        let bounds_south: f64 = get("tgr.bounds_south")?
            .parse()
            .map_err(|e: std::num::ParseFloatError| e.to_string())?;
        let bounds_north: f64 = get("tgr.bounds_north")?
            .parse()
            .map_err(|e: std::num::ParseFloatError| e.to_string())?;
        let min_zoom: u8 = get("tgr.min_zoom")?
            .parse()
            .map_err(|e: std::num::ParseIntError| e.to_string())?;
        let max_zoom: u8 = get("tgr.max_zoom")?
            .parse()
            .map_err(|e: std::num::ParseIntError| e.to_string())?;
        let clip_to_bounds = meta
            .get("tgr.clip_to_bounds")
            .map(|v| v == "1")
            .unwrap_or(false);
        let polygon_wgs84 = meta
            .get("tgr.polygon_wgs84")
            .filter(|s| !s.is_empty())
            .cloned();
        let total_tiles: i64 = meta
            .get("tgr.total_tiles")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let downloaded_tiles: i64 = meta
            .get("tgr.downloaded_tiles")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let failed_tiles: i64 = meta
            .get("tgr.failed_tiles")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);

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
        app_db
            .update_task_total(&new_id, total_tiles)
            .map_err(|e| e.to_string())?;
        app_db
            .update_task_progress(&new_id, downloaded_tiles, failed_tiles)
            .map_err(|e| e.to_string())?;
        app_db
            .update_task_status(&new_id, "paused")
            .map_err(|e| e.to_string())?;

        Ok(new_id)
    } else if is_zip {
        // ── v1 兼容：解压 tiles.db 后注册 ───────────────────────────────────
        let file = std::fs::File::open(&src_path).map_err(|e| format!("无法打开文件: {}", e))?;
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
        app_db
            .create_task(&new_id, &new_task, &tile_path_str)
            .map_err(|e| e.to_string())?;
        app_db
            .update_task_total(&new_id, task.total_tiles)
            .map_err(|e| e.to_string())?;
        app_db
            .update_task_progress(&new_id, task.downloaded_tiles, task.failed_tiles)
            .map_err(|e| e.to_string())?;
        app_db
            .update_task_status(&new_id, "paused")
            .map_err(|e| e.to_string())?;

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
            let conn = rusqlite::Connection::open(&tile_store_path).map_err(|e| e.to_string())?;
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
