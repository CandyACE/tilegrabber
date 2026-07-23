//! TileGrabber — 异步多任务下载引擎
//!
//! 管理多个并发下载任务，每个任务通过 watch channel 发送暂停/恢复/取消信号。
//! 下载进度通过 Tauri 事件 `tilegrab-progress` 推送到前端。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::Result;
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::{watch, Semaphore};
use tokio::task::JoinSet;

use crate::storage::app_db::AppDb;
use crate::storage::tile_store::TileStore;
use crate::tile_math::{enumerate_tiles, enumerate_tiles_with_polygon};
use crate::types::{Bounds, TileSource};

use super::clip_pipeline::{self, ClipMsg, ClipOutcome, ClipPipelineConfig};
use super::rules::DownloadRules;
use super::throttle;
use super::worker;

// ─── 控制信号 ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum CtrlSignal {
    Run,
    Pause,
    Cancel,
}

// ─── 进度事件 payload ────────────────────────────────────────────────────────

/// Tauri 事件 `tilegrab-progress` 的 payload
#[derive(Debug, Clone, Serialize)]
pub struct ProgressPayload {
    pub task_id: String,
    pub total: i64,
    pub downloaded: i64,
    pub failed: i64,
    /// 下载速度（瓦片/秒）
    pub speed: f64,
    /// 下载速度（字节/秒）
    pub bytes_per_sec: f64,
    /// 剩余秒数估算（None 表示无法估算）
    pub eta_secs: Option<f64>,
    /// 当前状态字符串
    pub status: String,
    /// 批次失败率过高时的重试冷却倒计时（秒），None 表示无冷却
    pub retry_in_secs: Option<u64>,
}

/// 单个瓦片的经纬度边界（用于闪烁显示）
#[derive(Debug, Clone, Serialize)]
pub struct TileFlashBounds {
    pub west: f64,
    pub east: f64,
    pub south: f64,
    pub north: f64,
}

/// Tauri 事件 `tilegrab-tile-flash` 的 payload
#[derive(Debug, Clone, Serialize)]
pub struct TileFlashPayload {
    pub task_id: String,
    pub tiles: Vec<TileFlashBounds>,
}

/// 流水线写入批次消息
struct WriteBatchMsg {
    success_tiles: Vec<(crate::tile_math::TileCoord, Vec<u8>)>,
    failed_tiles: Vec<(crate::tile_math::TileCoord, String)>,
    flash_tiles: Vec<TileFlashBounds>,
}

// ─── 任务句柄 ────────────────────────────────────────────────────────────────

struct TaskHandle {
    ctrl_tx: watch::Sender<CtrlSignal>,
}

// ─── DownloadEngine ──────────────────────────────────────────────────────────

/// 下载引擎（可 Clone，线程安全）
#[derive(Clone)]
pub struct DownloadEngine {
    handles: Arc<Mutex<HashMap<String, TaskHandle>>>,
    broadcast_tx: Arc<std::sync::OnceLock<tokio::sync::broadcast::Sender<ProgressPayload>>>,
}

impl DownloadEngine {
    pub fn new() -> Self {
        DownloadEngine {
            handles: Arc::new(Mutex::new(HashMap::new())),
            broadcast_tx: Arc::new(std::sync::OnceLock::new()),
        }
    }

    /// 设置进度广播通道（在 lib.rs::setup 中调用一次）
    pub fn set_broadcast_tx(&self, tx: tokio::sync::broadcast::Sender<ProgressPayload>) {
        let _ = self.broadcast_tx.set(tx);
    }

    /// 启动或新建一个下载任务
    pub fn start(
        &self,
        task_id: String,
        app_db: AppDb,
        concurrency: usize,
        app: AppHandle,
    ) -> Result<()> {
        let mut handles = self
            .handles
            .lock()
            .map_err(|_| anyhow::anyhow!("engine lock poisoned"))?;

        // 如果已存在（正在运行），直接返回
        if handles.contains_key(&task_id) {
            return Ok(());
        }

        let (ctrl_tx, ctrl_rx) = watch::channel(CtrlSignal::Run);
        handles.insert(task_id.clone(), TaskHandle { ctrl_tx });

        // 任务结束后自动清理句柄
        let engine_ref = self.handles.clone();
        let tid = task_id.clone();
        let bcast = self.broadcast_tx.get().cloned();

        tokio::spawn(async move {
            use futures_util::FutureExt;
            let result = std::panic::AssertUnwindSafe(run_download(
                task_id.clone(),
                app_db.clone(),
                concurrency,
                ctrl_rx,
                app.clone(),
                bcast.clone(),
            ))
            .catch_unwind()
            .await;
            if let Err(_panic) = result {
                let _ = app_db.update_task_status(&task_id, "failed");
                let _ = app_db.add_log(
                    Some(&task_id),
                    "error",
                    "下载任务因内部 panic 异常终止",
                );
            }
            if let Ok(mut h) = engine_ref.lock() {
                h.remove(&tid);
            }
        });

        Ok(())
    }

    /// 暂停指定任务
    pub fn pause(&self, task_id: &str) -> Result<()> {
        let handles = self
            .handles
            .lock()
            .map_err(|_| anyhow::anyhow!("engine lock poisoned"))?;
        if let Some(h) = handles.get(task_id) {
            let _ = h.ctrl_tx.send(CtrlSignal::Pause);
        }
        Ok(())
    }

    /// 恢复已暂停的任务
    pub fn resume(&self, task_id: &str) -> Result<()> {
        let handles = self
            .handles
            .lock()
            .map_err(|_| anyhow::anyhow!("engine lock poisoned"))?;
        if let Some(h) = handles.get(task_id) {
            let _ = h.ctrl_tx.send(CtrlSignal::Run);
        }
        Ok(())
    }

    /// 取消并终止指定任务
    pub fn cancel(&self, task_id: &str) -> Result<()> {
        let handles = self
            .handles
            .lock()
            .map_err(|_| anyhow::anyhow!("engine lock poisoned"))?;
        if let Some(h) = handles.get(task_id) {
            let _ = h.ctrl_tx.send(CtrlSignal::Cancel);
        }
        Ok(())
    }

    /// 查询任务是否正在运行
    pub fn is_active(&self, task_id: &str) -> bool {
        self.handles
            .lock()
            .map(|h| h.contains_key(task_id))
            .unwrap_or(false)
    }

    /// 统计当前正在实际下载（Run 信号）的任务数（不含已暂停的任务）
    pub fn active_download_count(&self) -> usize {
        self.handles
            .lock()
            .map(|h| {
                h.values()
                    .filter(|th| *th.ctrl_tx.borrow() == CtrlSignal::Run)
                    .count()
            })
            .unwrap_or(0)
    }

    /// 返回所有正在实际下载（Run 信号）的任务 ID 列表
    pub fn running_task_ids(&self) -> Vec<String> {
        self.handles
            .lock()
            .map(|h| {
                h.iter()
                    .filter(|(_, th)| *th.ctrl_tx.borrow() == CtrlSignal::Run)
                    .map(|(id, _)| id.clone())
                    .collect()
            })
            .unwrap_or_default()
    }
}

// ─── 下载主循环 ──────────────────────────────────────────────────────────────

async fn run_download(
    task_id: String,
    app_db: AppDb,
    concurrency: usize,
    mut ctrl_rx: watch::Receiver<CtrlSignal>,
    app: AppHandle,
    broadcast_tx: Option<tokio::sync::broadcast::Sender<ProgressPayload>>,
) {
    // 1. 从数据库加载任务信息
    app_db.add_log(Some(&task_id), "info", "开始下载任务").ok();
    let task = match app_db.get_task(&task_id) {
        Ok(t) => t,
            Err(e) => {
                tracing::error!(task_id, error = %e, "[engine] cannot load task");
                app_db
                    .add_log(Some(&task_id), "error", &format!("加载任务失败: {}", e))
                    .ok();
                return;
            }
    };

    // 2. 解析 TileSource
    let source: TileSource = match serde_json::from_str(&task.source_config) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(task_id, error = %e, "[engine] invalid source_config");
            app_db
                .add_log(
                    Some(&task_id),
                    "error",
                    &format!("数据源配置解析失败: {}", e),
                )
                .ok();
            if let Err(e) = app_db.update_task_status(&task_id, "failed") {
                app_db.soft_err(Some(&task_id), "标记任务失败状态", e);
            }
            return;
        }
    };

    // MBTiles 文件数据源：走独立的复制路径
    if source.kind == crate::types::SourceKind::MbtileFile {
        let tile_store_path = match task.tile_store_path.as_deref() {
            Some(p) => p.to_owned(),
            None => {
                app_db
                    .add_log(Some(&task_id), "error", "任务缺少瓦片存储路径")
                    .ok();
                app_db.update_task_status(&task_id, "failed").ok();
                return;
            }
        };
        run_mbtiles_import(
            task_id,
            app_db,
            app,
            source.url_template,
            tile_store_path,
            ctrl_rx,
        )
        .await;
        return;
    }

    // 3. 打开瓦片存储（路径在任务创建时已持久化到 DB）
    let tile_store_path = match task.tile_store_path.as_deref() {
        Some(p) => p.to_owned(),
        None => {
            app_db
                .add_log(Some(&task_id), "error", "任务缺少瓦片存储路径")
                .ok();
            app_db.update_task_status(&task_id, "failed").ok();
            return;
        }
    };
    // 确保父目录存在（首次或目录被手动删除时）
    if let Some(parent) = std::path::Path::new(&tile_store_path).parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let tile_store = match TileStore::open(std::path::Path::new(&tile_store_path), &task_id) {
        Ok(ts) => ts,
        Err(e) => {
            tracing::error!(task_id, error = %e, "[engine] cannot open tile store");
            if let Err(e) = app_db.update_task_status(&task_id, "failed") {
                app_db.soft_err(Some(&task_id), "标记任务失败状态", e);
            }
            return;
        }
    };

    // 4. 将"下载中"回退为 pending（断点续传支持）
    tile_store.reset_stale_downloading().ok();

    // 将数据源格式写入 metadata，确保本地服务器以正确 MIME 类型提供瓦片
    let tile_format = serde_json::from_str::<serde_json::Value>(&task.source_config)
        .ok()
        .and_then(|v| {
            v.get("format")
                .and_then(|f| f.as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "png".to_string());
    tile_store.write_meta(&[("format", &tile_format)]).ok();

    // 矢量瓦片（Mapbox Vector Tile / Protobuf）旁路：跳过 GCJ02 像素纠偏与像素级裁剪
    // —— 这些后处理基于栅格像素，对 protobuf 几何无意义。
    let is_vector_format = matches!(
        tile_format.to_ascii_lowercase().as_str(),
        "pbf" | "mvt"
    );
    if is_vector_format && source.coord_type == crate::types::CoordType::Gcj02 {
        app_db
            .add_log(
                Some(&task_id),
                "warn",
                "矢量瓦片暂不支持 GCJ02 几何级纠偏；瓦片将按原坐标保存，预览/导出时可能存在 100–700 m 偏移",
            )
            .ok();
    }

    // 5. 首次运行：枚举瓦片并写入 download_state
    let init_total = tile_store.get_progress().map(|p| p.total).unwrap_or(0);

    if init_total == 0 {
        let base_bounds = Bounds {
            west: task.bounds_west,
            east: task.bounds_east,
            south: task.bounds_south,
            north: task.bounds_north,
        };

        // GCJ02 源显示纠偏需要访问邻接瓦片（最多 7 块）进行合成。
        // 下载时扩大约 0.01°（≈ zoom 18 下 7 块），保证边缘区域合成所需瓦片完整。
        // 矢量瓦片不做像素合成，无需扩边。
        let bounds = if source.coord_type == crate::types::CoordType::Gcj02 && !is_vector_format {
            const GCJ02_PAD: f64 = 0.01;
            Bounds {
                west: (base_bounds.west - GCJ02_PAD).max(-180.0),
                east: (base_bounds.east + GCJ02_PAD).min(180.0),
                south: (base_bounds.south - GCJ02_PAD).max(-85.051_129),
                north: (base_bounds.north + GCJ02_PAD).min(85.051_129),
            }
        } else {
            base_bounds.clone()
        };

        // 若任务附带多边形范围，仅枚举与多边形相交的瓦片，跳过外包围矩形中多余的瓦片
        let polygon: Option<Vec<[f64; 2]>> = task
            .polygon_wgs84
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok());

        let tiles = if let Some(ref poly) = polygon {
            if source.coord_type == crate::types::CoordType::Gcj02 && !is_vector_format {
                // GCJ02 纠偏合成需要从目标瓦片东/北方向读取 2×2 源瓦片拼合。
                // 若仅按多边形过滤下载，多边形东/北边缘以外的源瓦片将缺失，
                // 导致合成时 has_data=false，边缘瓦片保留 GCJ02 偏移。
                // 解决方案：GCJ02 + 多边形任务下载整个扩展边距范围（orig+GCJ02_PAD）
                // 的全部瓦片，多边形遮罩由后续的 clip_to_bounds_or_polygon 步骤处理。
                app_db
                    .add_log(
                        Some(&task_id),
                        "info",
                        "GCJ02 多边形任务：下载完整扩展范围以保证纠偏合成质量，多边形遮罩在后处理阶段应用",
                    )
                    .ok();
                enumerate_tiles(
                    &bounds,
                    task.min_zoom,
                    task.max_zoom,
                    &source.crs,
                    Some(2_000_000),
                )
            } else {
                app_db
                    .add_log(
                        Some(&task_id),
                        "info",
                        "已检测到多边形范围，将按多边形过滤下载瓦片",
                    )
                    .ok();
                enumerate_tiles_with_polygon(
                    &bounds,
                    task.min_zoom,
                    task.max_zoom,
                    &source.crs,
                    poly,
                    Some(2_000_000),
                )
            }
        } else {
            enumerate_tiles(
                &bounds,
                task.min_zoom,
                task.max_zoom,
                &source.crs,
                Some(2_000_000),
            )
        };
        match tile_store.init_download_state(&tiles) {
            Ok(total) => {
                app_db.update_task_total(&task_id, total).ok();
                app_db
                    .add_log(
                        Some(&task_id),
                        "info",
                        &format!(
                            "共枚举 {} 个瓦片，z{}-z{}",
                            total, task.min_zoom, task.max_zoom
                        ),
                    )
                    .ok();
            }
            Err(e) => {
                tracing::error!(task_id, error = %e, "[engine] init_download_state failed");
                app_db
                    .add_log(
                        Some(&task_id),
                        "error",
                        &format!("初始化瓦片列表失败: {}", e),
                    )
                    .ok();
                if let Err(e) = app_db.update_task_status(&task_id, "failed") {
                    app_db.soft_err(Some(&task_id), "标记任务失败状态", e);
                }
                return;
            }
        }
    }

    // 6. 加载下载规则（时间窗口 + 速率限制）
    let rules = DownloadRules::load(&app_db);

    // 读取可配置的最大重试次数（默认 3）
    let max_retries = app_db
        .get_setting("download.max_retries")
        .ok()
        .flatten()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(3);

    // 若当前不在时间窗口内，等待（每分钟检查一次）
    if rules.time_window_enabled && !rules.is_in_window() {
        app_db
            .add_log(
                Some(&task_id),
                "info",
                &format!(
                    "当前不在下载时间窗口（{:02}:00–{:02}:00），等待中…",
                    rules.time_window_start, rules.time_window_end
                ),
            )
            .ok();
        app_db.update_task_status(&task_id, "paused").ok();
        rules.wait_for_window().await;
        app_db.update_task_status(&task_id, "downloading").ok();
    }

    // 7. 创建 HTTP 客户端
    let proxy_url = if rules.proxy_enabled && !rules.proxy_url.is_empty() {
        Some(rules.proxy_url.clone())
    } else {
        None
    };
    let client = match worker::build_client_with_proxy(&source.headers, proxy_url.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(task_id, error = %e, "[engine] cannot build http client");
            if let Err(e) = app_db.update_task_status(&task_id, "failed") {
                app_db.soft_err(Some(&task_id), "标记任务失败状态", e);
            }
            return;
        }
    };

    app_db.update_task_status(&task_id, "downloading").ok();

    // === 流水线裁剪 ===
    // 启用条件：开启 clip_to_bounds + 非矢量瓦片 + 非 GCJ02（GCJ02 必须先合成再裁剪）。
    // 满足时，启动后台消费者；主写入循环在每次 save_tiles_batch 成功后转发坐标。
    let streaming_clip_enabled = task.clip_to_bounds
        && !is_vector_format
        && source.coord_type != crate::types::CoordType::Gcj02;
    let (clip_tx, clip_handle): (
        Option<tokio::sync::mpsc::UnboundedSender<ClipMsg>>,
        Option<tokio::task::JoinHandle<ClipOutcome>>,
    ) = if streaming_clip_enabled {
        let cfg = ClipPipelineConfig {
            store_path: tile_store_path.clone(),
            bounds: Bounds {
                west: task.bounds_west,
                east: task.bounds_east,
                south: task.bounds_south,
                north: task.bounds_north,
            },
            polygon: task
                .polygon_wgs84
                .as_deref()
                .and_then(|s| serde_json::from_str::<Vec<[f64; 2]>>(s).ok()),
            crs: source.crs.clone(),
            task_id: task_id.clone(),
        };
        let (tx, handle) = clip_pipeline::spawn(cfg, app.clone(), broadcast_tx.clone());
        app_db
            .add_log(
                Some(&task_id),
                "info",
                "流水线裁剪已启用：下载过程中并发完成边界瓦片裁剪",
            )
            .ok();
        (Some(tx), Some(handle))
    } else {
        (None, None)
    };

    // 速率限制：每瓦片最小间隔（由规则计算得出，0 = 不限速）
    let tile_delay_ms = rules.per_tile_delay_ms();
    let delay_min_ms = rules.delay_min_ms;
    let delay_max_ms = rules.delay_max_ms;
    // 批次高失败率冷却时长（秒）；来自 download.retry_delay_ms 设置
    let retry_cooldown_secs = rules.retry_delay_ms / 1000;

    // 辅助闭包：同时向本地前端和 SSE 客户端广播进度事件
    let emit_prog = {
        let app_e = app.clone();
        let btx = broadcast_tx.clone();
        move |payload: ProgressPayload| {
            let _ = app_e.emit("tilegrab-progress", payload.clone());
            if let Some(ref tx) = btx {
                tx.send(payload).ok();
            }
        }
    };

    // 立即发送初始进度事件，前端立刻将任务状态改为 "downloading"
    let init_prog = tile_store.get_progress().unwrap_or_default();
    emit_prog(ProgressPayload {
        task_id: task_id.clone(),
        total: init_prog.total,
        downloaded: init_prog.downloaded,
        failed: init_prog.failed,
        speed: 0.0,
        bytes_per_sec: 0.0,
        eta_secs: None,
        status: "downloading".to_string(),
        retry_in_secs: None,
    });

    let sem = Arc::new(Semaphore::new(concurrency.max(1)));
    let mut last_downloaded: i64 = 0;
    let mut last_tick = Instant::now();
    let mut last_bytes: u64 = 0; // 每个计时周期内下载的总字节数
    let mut batch_counter: u32 = 0; // 批次计数器，用于视口停顿模拟
    let mut disk_full_abort = false; // 磁盘空间不足时提前中止

    // === 流水线写入通道 ===
    // 单独的 tokio 任务负责 DB 写入 + 事件发送，主循环只管下载
    let (write_tx, mut write_rx) = tokio::sync::mpsc::channel::<WriteBatchMsg>(4);
    let write_store = tile_store.clone();
    let write_app_db = app_db.clone();
    let write_app = app.clone();
    let write_task_id = task_id.clone();
    let write_clip_tx = clip_tx.clone();
    let write_handle = tokio::spawn(async move {
        while let Some(msg) = write_rx.recv().await {
            let WriteBatchMsg {
                success_tiles,
                failed_tiles,
                flash_tiles,
            } = msg;
            // 批量写入成功的瓦片（单事务）——先保存原始数据，下载完成后再统一裁剪
            if !success_tiles.is_empty() {
                let store = write_store.clone();
                let coords_for_clip: Option<Vec<crate::tile_math::TileCoord>> =
                    write_clip_tx.as_ref().map(|_| {
                        success_tiles.iter().map(|(c, _)| c.clone()).collect()
                    });
                tokio::task::spawn_blocking(move || {
                    store.save_tiles_batch(&success_tiles).ok();
                })
                .await
                .ok();
                // 落盘成功后转发给流水线裁剪消费者
                if let (Some(tx), Some(coords)) = (write_clip_tx.as_ref(), coords_for_clip) {
                    let _ = tx.send(coords);
                }
            }
            // 批量标记失败（单事务）
            if !failed_tiles.is_empty() {
                for (coord, err) in &failed_tiles {
                    let log_msg = format!(
                        "瓦片 z{}/x{}/y{} 下载失败: {}",
                        coord.z, coord.x, coord.y, err
                    );
                    write_app_db
                        .add_log(Some(&write_task_id), "warn", &log_msg)
                        .ok();
                }
                write_store.mark_failed_batch(&failed_tiles).ok();
            }
            // 发送瓦片闪烁事件
            if !flash_tiles.is_empty() {
                let _ = write_app.emit(
                    "tilegrab-tile-flash",
                    TileFlashPayload {
                        task_id: write_task_id.clone(),
                        tiles: flash_tiles,
                    },
                );
            }
        }
    });

    // 7. 主下载循环
    'outer: loop {
        // 暂停 / 取消检查
        {
            let signal = ctrl_rx.borrow().clone();
            match signal {
                CtrlSignal::Cancel => break 'outer,
                CtrlSignal::Pause => {
                    // 立即更新 DB 并通知前端，使 UI 立刻脱离"正在暂停"状态
                    app_db.update_task_status(&task_id, "paused").ok();
                    if let Ok(p) = tile_store.get_progress() {
                        emit_prog(ProgressPayload {
                            task_id: task_id.clone(),
                            total: p.total,
                            downloaded: p.downloaded,
                            failed: p.failed,
                            speed: 0.0,
                            bytes_per_sec: 0.0,
                            eta_secs: None,
                            status: "paused".to_string(),
                            retry_in_secs: None,
                        });
                    }
                    // 等待恢复或取消信号
                    loop {
                        if ctrl_rx.changed().await.is_err() {
                            break 'outer;
                        }
                        let sig = ctrl_rx.borrow().clone();
                        match sig {
                            CtrlSignal::Run => {
                                app_db.update_task_status(&task_id, "downloading").ok();
                                break;
                            }
                            CtrlSignal::Cancel => break 'outer,
                            CtrlSignal::Pause => {} // 继续等待
                        }
                    }
                }
                CtrlSignal::Run => {}
            }
        }

        // 取下一批 pending 瓦片（不排序，避免大表 ORDER BY 开销）
        let mut batch = match tile_store.get_pending_batch(concurrency * 8) {
            Ok(b) if b.is_empty() => break 'outer, // 全部完成
            Ok(b) => b,
            Err(e) => {
                tracing::error!(task_id, error = %e, "[engine] get_pending_batch error");
                break 'outer;
            }
        };

        // 空间局部性排序：模拟人类浏览地图的空间聚类模式
        throttle::sort_spatial_locality(&mut batch);

        // 并发下载本批瓦片
        let mut join_set = JoinSet::new();
        for tile in batch {
            if *ctrl_rx.borrow() == CtrlSignal::Cancel {
                break;
            }
            // 信号量永不显式关闭，acquire 失败仅可能是 sem 被 drop —— 此处理论不可达
            let permit = sem
                .clone()
                .acquire_owned()
                .await
                .expect("download semaphore unexpectedly closed");
            let client = client.clone();
            let source = source.clone();
            let ctrl = ctrl_rx.clone();

            join_set.spawn(async move {
                let _permit = permit;
                if *ctrl.borrow() == CtrlSignal::Cancel {
                    return (
                        tile,
                        Err::<Vec<u8>, anyhow::Error>(anyhow::anyhow!("cancelled")),
                    );
                }
                // 瓦片间随机微延迟（防封禁，使用用户配置的延迟范围）
                throttle::random_delay(delay_min_ms, delay_max_ms).await;
                // 速率限制额外延迟
                if tile_delay_ms > 0 {
                    tokio::time::sleep(tokio::time::Duration::from_millis(tile_delay_ms)).await;
                }
                (tile, worker::download_tile(&client, tile, &source, max_retries).await)
            });
        }

        // 收集下载结果
        let mut flash_tiles: Vec<TileFlashBounds> = Vec::new();
        let mut success_tiles: Vec<(crate::tile_math::TileCoord, Vec<u8>)> = Vec::new();
        let mut failed_tiles: Vec<(crate::tile_math::TileCoord, String)> = Vec::new();
        while let Some(res) = join_set.join_next().await {
            match res {
                Ok((coord, Ok(data))) => {
                    throttle::ADAPTIVE.report_success();
                    last_bytes += data.len() as u64;
                    let b = crate::tile_math::tile_to_lonlat_bounds(
                        coord.x,
                        coord.y,
                        coord.z,
                        &source.crs,
                    );
                    flash_tiles.push(TileFlashBounds {
                        west: b.west,
                        east: b.east,
                        south: b.south,
                        north: b.north,
                    });
                    success_tiles.push((coord, data));
                }
                Ok((coord, Err(ref e))) if e.to_string() != "cancelled" => {
                    throttle::ADAPTIVE.report_failure();
                    failed_tiles.push((coord, e.to_string()));
                }
                _ => {}
            }
        }

        // 流水线：将本批写入任务发送到后台写入线程，主循环立即开始下一批下载
        let batch_failed_count = failed_tiles.len();
        let batch_total = success_tiles.len() + batch_failed_count;
        let batch_downloaded = (batch_total - batch_failed_count) as i64;
        let _ = write_tx
            .send(WriteBatchMsg {
                success_tiles,
                failed_tiles,
                flash_tiles,
            })
            .await;

        // 批次间停顿：优先使用规则配置，否则使用 throttle 默认随机值
        match rules.batch_pause_ms() {
            Some(ms) => tokio::time::sleep(tokio::time::Duration::from_millis(ms)).await,
            None => throttle::burst_pause().await,
        }

        // 每 16 批次插入一次"视口停顿"，模拟人类浏览地图时
        // 看完一个区域后拖拽到下一个区域的行为
        batch_counter += 1;
        if batch_counter % 16 == 0 {
            throttle::viewport_pause().await;
        }

        // 每 50 批次检查一次磁盘剩余空间，防止将磁盘写满
        if batch_counter % 50 == 0 {
            const MIN_FREE_BYTES: u64 = 200 * 1024 * 1024; // 200 MB
            if let Ok(free) = fs2::available_space(std::path::Path::new(&tile_store_path)) {
                if free < MIN_FREE_BYTES {
                    app_db
                        .add_log(
                            Some(&task_id),
                            "warn",
                            &format!(
                                "磁盘剩余空间不足（{}），任务已自动暂停",
                                format_bytes(free)
                            ),
                        )
                        .ok();
                    disk_full_abort = true;
                    break 'outer;
                }
            }
        }

        // 时间窗口检查：如果离开了时间窗口，暂停等待下次窗口
        if rules.time_window_enabled && !rules.is_in_window() {
            app_db
                .add_log(
                    Some(&task_id),
                    "info",
                    &format!(
                        "已离开下载时间窗口，暂停等待（{:02}:00–{:02}:00）",
                        rules.time_window_start, rules.time_window_end
                    ),
                )
                .ok();
            rules.wait_for_window().await;
        }

        // 推送进度事件（使用本批次计数快速计算，避免每次查询 DB）
        last_downloaded += batch_downloaded;
        let elapsed = last_tick.elapsed().as_secs_f64();
        if elapsed > 0.5 {
            // 定期从 DB 获取精确进度
            if let Ok(progress) = tile_store.get_progress() {
                let delta = progress.downloaded - (last_downloaded - batch_downloaded);
                let speed = if elapsed > 0.1 {
                    delta as f64 / elapsed
                } else {
                    0.0
                };
                let remaining = progress.pending + progress.failed;
                let eta_secs = if speed > 0.1 {
                    Some(remaining as f64 / speed)
                } else {
                    None
                };

                let bytes_per_sec = if elapsed > 0.1 {
                    last_bytes as f64 / elapsed
                } else {
                    0.0
                };
                last_downloaded = progress.downloaded;
                last_tick = Instant::now();
                last_bytes = 0;

                app_db
                    .update_task_progress(&task_id, progress.downloaded, progress.failed)
                    .ok();

                emit_prog(ProgressPayload {
                    task_id: task_id.clone(),
                    total: progress.total,
                    downloaded: progress.downloaded,
                    failed: progress.failed,
                    speed,
                    bytes_per_sec,
                    eta_secs,
                    status: "downloading".to_string(),
                    retry_in_secs: None,
                });
            }
        }

        // 批次失败率过高时推送重试倒计时（≥75% 失败且至少 4 个失败瓦片）
        if retry_cooldown_secs > 0
            && batch_failed_count >= 4
            && batch_failed_count * 4 >= batch_total
        {
            if let Ok(p) = tile_store.get_progress() {
                for remaining in (1..=retry_cooldown_secs).rev() {
                    let sig = ctrl_rx.borrow().clone();
                    if sig == CtrlSignal::Cancel || sig == CtrlSignal::Pause {
                        break;
                    }
                    emit_prog(ProgressPayload {
                        task_id: task_id.clone(),
                        total: p.total,
                        downloaded: p.downloaded,
                        failed: p.failed,
                        speed: 0.0,
                        bytes_per_sec: 0.0,
                        eta_secs: None,
                        status: "downloading".to_string(),
                        retry_in_secs: Some(remaining),
                    });
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                }
            }
        }
    }

    // 关闭写入通道，等待后台写入完成
    drop(write_tx);
    write_handle.await.ok();

    // 关闭流水线裁剪通道，等待消费者排空。
    // 若仍有未刷写的边界瓦片，下载循环已退出但 clip 仍在做尾部 batch；
    // 把任务状态翻为 "processing" 让前端显示「裁剪中」，避免 UI 看起来卡住。
    let pre_clip_cancelled = *ctrl_rx.borrow() == CtrlSignal::Cancel;
    if clip_handle.is_some() && !pre_clip_cancelled && !disk_full_abort {
        app_db.update_task_status(&task_id, "processing").ok();
        if let Ok(p) = tile_store.get_progress() {
            emit_prog(ProgressPayload {
                task_id: task_id.clone(),
                total: p.total,
                downloaded: p.downloaded,
                failed: p.failed,
                speed: 0.0,
                bytes_per_sec: 0.0,
                eta_secs: None,
                retry_in_secs: None,
                status: "processing".to_string(),
            });
        }
    }
    drop(clip_tx);
    let streaming_clip_outcome: Option<ClipOutcome> = if let Some(h) = clip_handle {
        match h.await {
            Ok(o) => Some(o),
            Err(e) => {
                app_db
                    .add_log(
                        Some(&task_id),
                        "warn",
                        &format!("流水线裁剪任务异常: {}", e),
                    )
                    .ok();
                None
            }
        }
    } else {
        None
    };
    // 仅在下载未被取消时，把 tiles.clipped 标记写入 .tiles，
    // 让未来重启不再触发 post_clip_tiles 全表扫描。
    let was_cancelled = *ctrl_rx.borrow() == CtrlSignal::Cancel;
    let streaming_clip_boundary: Option<u64> = match &streaming_clip_outcome {
        Some(ClipOutcome::Completed { boundary_total }) if !was_cancelled => Some(*boundary_total),
        _ => None,
    };
    let streaming_clip_completed = streaming_clip_boundary.is_some();
    if streaming_clip_completed {
        tile_store
            .write_meta(&[("tiles.clipped", "1")])
            .ok();
    }

    // 磁盘满时提前中止：暂停任务并通知前端
    if disk_full_abort {
        app_db.update_task_status(&task_id, "paused").ok();
        if let Ok(p) = tile_store.get_progress() {
            emit_prog(ProgressPayload {
                task_id: task_id.clone(),
                total: p.total,
                downloaded: p.downloaded,
                failed: p.failed,
                speed: 0.0,
                bytes_per_sec: 0.0,
                eta_secs: None,
                status: "paused".to_string(),
                retry_in_secs: None,
            });
        }
        let _ = app.emit(
            "tilegrab-disk-full",
            serde_json::json!({ "task_id": task_id }),
        );
        return;
    }

    // 补发下载完成（100%）事件：下载循环的最后一次 tick 可能还未到 100%，
    // 若紧接着进入裁剪阶段，进度条会直接从未满跳到 0%。
    // 这里先让进度条填满再切换状态，用户体验更连贯。
    {
        let done_progress = tile_store.get_progress().unwrap_or_default();
        emit_prog(ProgressPayload {
            task_id: task_id.clone(),
            total: done_progress.total,
            downloaded: done_progress.downloaded,
            failed: done_progress.failed,
            speed: 0.0,
            bytes_per_sec: 0.0,
            eta_secs: None,
            status: "downloading".to_string(),
            retry_in_secs: None,
        });
    }

    // ── GCJ02 纠偏合成（下载完成后，裁剪之前）──────────────────────────────────
    // 将下载的原始高德瓦片合成为 WGS84 对齐的纠偏瓦片，使本地服务器无需额外处理。
    // 矢量瓦片不走该像素合成流水线。
    if source.coord_type == crate::types::CoordType::Gcj02
        && !is_vector_format
        && *ctrl_rx.borrow() != CtrlSignal::Cancel
    {
        app_db
            .add_log(Some(&task_id), "info", "开始 GCJ02 纠偏合成…")
            .ok();
        app_db.update_task_status(&task_id, "processing").ok();

        let gcj02_store_path = tile_store_path.clone();
        let gcj02_app = app.clone();
        let gcj02_task_id = task_id.clone();
        let gcj02_bcast = broadcast_tx.clone();
        let gcj02_bounds = crate::types::Bounds {
            west: task.bounds_west,
            east: task.bounds_east,
            south: task.bounds_south,
            north: task.bounds_north,
        };
        let gcj02_crs = source.crs.clone();
        let gcj02_min_zoom = task.min_zoom;
        let gcj02_max_zoom = task.max_zoom;

        let gcj02_result = tokio::task::spawn_blocking(move || {
            post_gcj02_composite(
                &gcj02_store_path,
                &gcj02_bounds,
                gcj02_min_zoom,
                gcj02_max_zoom,
                &gcj02_crs,
                |done, total| {
                    let payload = ProgressPayload {
                        task_id: gcj02_task_id.clone(),
                        total: total as i64,
                        downloaded: done as i64,
                        failed: 0,
                        speed: 0.0,
                        bytes_per_sec: 0.0,
                        eta_secs: None,
                        status: "processing".to_string(),
                        retry_in_secs: None,
                    };
                    let _ = gcj02_app.emit("tilegrab-progress", payload.clone());
                    if let Some(ref tx) = gcj02_bcast {
                        tx.send(payload).ok();
                    }
                },
            )
        })
        .await;

        match gcj02_result {
            Ok(Ok(())) => {
                app_db
                    .add_log(Some(&task_id), "info", "GCJ02 纠偏合成完成")
                    .ok();
            }
            Ok(Err(e)) => {
                app_db
                    .add_log(
                        Some(&task_id),
                        "warn",
                        &format!("GCJ02 纠偏合成异常: {}", e),
                    )
                    .ok();
            }
            Err(e) => {
                app_db
                    .add_log(
                        Some(&task_id),
                        "warn",
                        &format!("GCJ02 合成任务异常: {}", e),
                    )
                    .ok();
            }
        }
    }

    // ── 下载完成后的精确裁剪 ─────────────────────────────────────────────────
    // 先保存原始瓦片，确保数据完整；下载全部结束后再统一做像素级裁剪，
    // 这样既不影响下载速度，又能保证裁剪结果一致。
    // 矢量瓦片不做像素级裁剪（几何级裁剪在后续阶段考虑）。
    // 若流水线裁剪已完成（streaming_clip_completed == true），跳过后处理全表扫描。
    if streaming_clip_completed {
        let count = streaming_clip_boundary.unwrap_or(0);
        let msg = if count > 0 {
            format!("瓦片裁剪已通过流水线并发完成：{} 块边界瓦片", count)
        } else {
            "瓦片裁剪已通过流水线并发完成".to_string()
        };
        app_db.add_log(Some(&task_id), "info", &msg).ok();
    }
    if task.clip_to_bounds
        && !is_vector_format
        && !streaming_clip_completed
        && *ctrl_rx.borrow() != CtrlSignal::Cancel
    {
        app_db
            .add_log(Some(&task_id), "info", "开始精确裁剪瓦片…")
            .ok();
        app_db.update_task_status(&task_id, "processing").ok();

        // 查询边界瓦片总数，用于裁剪进度（第一遍扫描完后才知道，这里先发 0/0 的占位事件）
        // 立即发送裁剪启动事件，让前端切换到"裁剪中"状态；
        // downloaded 保持 total 使进度条暂时停在 100%（等第一遍扫描完毕后再重置）
        let download_total: i64 = tile_store.get_progress().map(|p| p.total).unwrap_or(0);
        emit_prog(ProgressPayload {
            task_id: task_id.clone(),
            total: download_total,
            downloaded: download_total,
            failed: 0,
            speed: 0.0,
            bytes_per_sec: 0.0,
            eta_secs: None,
            status: "processing".to_string(),
            retry_in_secs: None,
        });

        let clip_bounds = Bounds {
            west: task.bounds_west,
            east: task.bounds_east,
            south: task.bounds_south,
            north: task.bounds_north,
        };
        let clip_polygon: Option<Vec<[f64; 2]>> = task
            .polygon_wgs84
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok());
        let clip_crs = source.crs.clone();
        let clip_store_path = tile_store_path.clone();
        let clip_app = app.clone();
        let clip_task_id = task_id.clone();
        let clip_bcast = broadcast_tx.clone();

        let clip_result = tokio::task::spawn_blocking(move || {
            post_clip_tiles(
                &clip_store_path,
                &clip_bounds,
                clip_polygon.as_deref(),
                &clip_crs,
                |done, total| {
                    let payload = ProgressPayload {
                        task_id: clip_task_id.clone(),
                        total: total as i64,
                        downloaded: done as i64,
                        failed: 0,
                        speed: 0.0,
                        bytes_per_sec: 0.0,
                        eta_secs: None,
                        status: "processing".to_string(),
                        retry_in_secs: None,
                    };
                    let _ = clip_app.emit("tilegrab-progress", payload.clone());
                    if let Some(ref tx) = clip_bcast {
                        tx.send(payload).ok();
                    }
                },
                |tiles| {
                    let _ = clip_app.emit(
                        "tilegrab-clip-tiles",
                        TileFlashPayload {
                            task_id: clip_task_id.clone(),
                            tiles,
                        },
                    );
                },
            )
        })
        .await;

        match clip_result {
            Ok(Ok(())) => {
                app_db.add_log(Some(&task_id), "info", "瓦片裁剪完成").ok();
            }
            Ok(Err(e)) => {
                app_db
                    .add_log(Some(&task_id), "warn", &format!("瓦片裁剪异常: {}", e))
                    .ok();
            }
            Err(e) => {
                app_db
                    .add_log(Some(&task_id), "warn", &format!("裁剪任务异常: {}", e))
                    .ok();
            }
        }
    }

    // 8. 确定最终状态
    let final_status = if *ctrl_rx.borrow() == CtrlSignal::Cancel {
        "cancelled"
    } else {
        match tile_store.get_progress() {
            Ok(p) if p.pending == 0 && p.failed == 0 => "completed",
            Ok(p) if p.pending == 0 => "completed_with_errors",
            _ => "paused",
        }
    };

    app_db.update_task_status(&task_id, final_status).ok();

    let progress = tile_store.get_progress().unwrap_or_default();
    // 将最终精确进度持久化到 tasks 表（下载循环内只按 0.5s 节流更新，最后一次可能未到 100%）
    // 同步更新 total_tiles：GCJ02 合成后会删除 padding 瓦片，download_state 行数从 N+M 缩减到 N，
    // 但 init_download_state 时写入的 total_tiles = N+M 从未更新，导致进度分母偏大无法到 100%
    app_db
        .update_task_total(&task_id, progress.total)
        .ok();
    app_db
        .update_task_progress(&task_id, progress.downloaded, progress.failed)
        .ok();
    let summary = format!(
        "任务结束 [{}]：已下载 {}，失败 {}，共 {}",
        final_status, progress.downloaded, progress.failed, progress.total
    );
    app_db.add_log(Some(&task_id), "info", &summary).ok();

    // 下载完成后发送系统通知（仅在非取消状态且通知设置已启用时）
    let notify_enabled = app_db
        .get_setting("app.download_notification")
        .ok()
        .flatten()
        .map(|v| v != "false")
        .unwrap_or(true);
    if notify_enabled && final_status != "cancelled" {
        use tauri_plugin_notification::NotificationExt;
        let body = if progress.failed > 0 {
            format!(
                "已下载 {} 个瓦片，失败 {} 个",
                progress.downloaded, progress.failed
            )
        } else {
            format!("已成功下载 {} 个瓦片", progress.downloaded)
        };
        app.notification()
            .builder()
            .title(&task.name)
            .body(&body)
            .show()
            .ok();
    }

    emit_prog(ProgressPayload {
        task_id,
        total: progress.total,
        downloaded: progress.downloaded,
        failed: progress.failed,
        speed: 0.0,
        bytes_per_sec: 0.0,
        eta_secs: None,
        status: final_status.to_string(),
        retry_in_secs: None,
    });
}

// ─── 辅助函数 ─────────────────────────────────────────────────────────────────

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.0} KB", bytes as f64 / 1024.0)
    }
}

// ─── 下载后统一裁剪 ──────────────────────────────────────────────────────────

/// 对已下载的瓦片进行统一精确裁剪。
///
/// 遍历 .tiles 存储中的所有瓦片，将超出范围的像素设为透明（PNG），
/// 完成后在 metadata 表写入 `tiles.clipped=1` 标记，供导出时跳过二次裁剪。
///
/// # 实现要点
/// 两遍扫描策略：
/// 1. 第一遍：ROWID 游标流式读取坐标（无 BLOB），快速筛选出边界瓦片
/// 2. 第二遍：仅对边界瓦片按 `rowid IN (...)` 读取 tile_data 并裁剪
///
/// 内部瓦片完全跳过 BLOB 读取，避免无用的图像解码。
fn post_clip_tiles(
    store_path: &str,
    bounds: &Bounds,
    polygon: Option<&[[f64; 2]]>,
    crs: &crate::types::CrsType,
    progress_cb: impl Fn(u64, u64),
    tile_flash_cb: impl Fn(Vec<TileFlashBounds>),
) -> Result<()> {
    use rayon::prelude::*;
    use rusqlite::{params, Connection};

    let conn = Connection::open(store_path)?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;
         PRAGMA cache_size=-204800;
         PRAGMA mmap_size=536870912;
         PRAGMA temp_store=MEMORY;
         PRAGMA busy_timeout=10000;",
    )?;

    // 统计总瓦片数（进度条总量）
    let total: u64 =
        conn.query_row("SELECT COUNT(*) FROM tiles", [], |r| r.get::<_, i64>(0))? as u64;
    if total == 0 {
        return Ok(());
    }
    progress_cb(0, total);

    // ── 第一遍：仅读坐标，筛选边界瓦片的 ROWID ────────────────────────────
    // BLOB 不在此阶段读取，内存开销极小，即使百万瓦片也能秒级完成。
    // 每批 2000 条坐标（约 64 KB），游标向前推进不受 DELETE 影响。
    const COORD_BATCH: usize = 2000;
    let mut last_rowid: i64 = 0;
    let mut boundary_rowids: Vec<i64> = Vec::new();

    loop {
        let batch: Vec<(i64, i64, i64, i64)> = {
            let mut stmt = conn.prepare(
                "SELECT rowid, zoom_level, tile_column, tile_row
                 FROM tiles
                 WHERE rowid > ?1
                 ORDER BY rowid
                 LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![last_rowid, COORD_BATCH as i64], |row| {
                Ok((
                    row.get::<_, i64>(0)?, // rowid
                    row.get::<_, i64>(1)?, // zoom_level
                    row.get::<_, i64>(2)?, // tile_column (x)
                    row.get::<_, i64>(3)?, // tile_row    (y)
                ))
            })?;
            rows.collect::<std::result::Result<_, _>>()?
        };

        if batch.is_empty() {
            break;
        }
        last_rowid = batch.last().map(|(rowid, ..)| *rowid).unwrap_or(last_rowid);

        for (rowid, z, x, y) in &batch {
            let tb = crate::tile_math::tile_to_lonlat_bounds(*x as u32, *y as u32, *z as u8, crs);
            // 判断是否需要处理：
            // - 无多边形（矩形任务）：瓦片完全在 bbox 内则跳过
            // - 有多边形：必须确认瓦片四个角点都在多边形内才跳过；
            //   如果只靠 bbox 检查，位于 bbox 内但多边形外的瓦片会被错误跳过
            //   （高缩放级别时这类瓦片数量极多，导致高缩放裁剪失效）
            let needs_processing = if let Some(poly) = polygon {
                let corners = [
                    [tb.west, tb.north],
                    [tb.east, tb.north],
                    [tb.east, tb.south],
                    [tb.west, tb.south],
                ];
                !corners
                    .iter()
                    .all(|c| crate::tile_math::point_in_polygon(c[0], c[1], poly))
            } else {
                !(tb.west >= bounds.west
                    && tb.east <= bounds.east
                    && tb.south >= bounds.south
                    && tb.north <= bounds.north)
            };
            if needs_processing {
                boundary_rowids.push(*rowid);
            }
        }
    }

    let boundary_total = boundary_rowids.len() as u64;
    progress_cb(0, boundary_total.max(1));

    if boundary_rowids.is_empty() {
        // 所有瓦片都在范围内，无需任何像素级裁剪
        conn.execute(
            "INSERT OR REPLACE INTO metadata (name, value) VALUES ('tiles.clipped', '1')",
            [],
        )?;
        progress_cb(1, 1);
        return Ok(());
    }

    // ── 第二遍：仅对边界瓦片读 BLOB → 裁剪 → 写回 ───────────────────────
    // 使用 rowid IN (r1, r2, ...) 精确点查；rowid 是整数主键，每次查询 O(log n)，
    // 不存在多元组 IN 的索引失效问题。
    // DATA_BATCH 设为 1000：每批让 rayon 有足够多的瓦片充分并行；
    // 同时把 SQLite 事务次数从 N/50 降低到 N/1000，大幅减少 commit 开销。
    const DATA_BATCH: usize = 1000;
    let mut processed: u64 = 0;

    for chunk in boundary_rowids.chunks(DATA_BATCH) {
        // 构造 SELECT ... WHERE rowid IN (?, ?, ...)
        let placeholders: String = (1..=chunk.len())
            .map(|i| format!("?{i}"))
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT rowid, zoom_level, tile_column, tile_row, tile_data
             FROM tiles WHERE rowid IN ({placeholders})"
        );
        let batch_data: Vec<(i64, i64, i64, i64, Vec<u8>)> = {
            let mut stmt = conn.prepare(&sql)?;
            let param_refs: Vec<&dyn rusqlite::types::ToSql> = chunk
                .iter()
                .map(|r| r as &dyn rusqlite::types::ToSql)
                .collect();
            let rows = stmt.query_map(param_refs.as_slice(), |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Vec<u8>>(4)?,
                ))
            })?;
            rows.collect::<std::result::Result<_, _>>()?
        };

        // 并行裁剪
        let updates: Vec<(i64, Option<Vec<u8>>)> = batch_data
            .par_iter()
            .filter_map(|(rowid, z, x, y, data)| {
                let result = if let Some(poly) = polygon {
                    crate::export::tile_clip::clip_tile_to_polygon_crs(
                        data, *x as u32, *y as u32, *z as u8, poly, crs,
                    )
                } else {
                    crate::export::tile_clip::clip_tile_to_bounds_crs(
                        data, *x as u32, *y as u32, *z as u8, bounds, crs,
                    )
                };
                match result {
                    Ok(Some(d)) => Some((*rowid, Some(d))), // 写入裁剪后数据
                    Ok(None) => Some((*rowid, None)),       // 完全在范围外：删除
                    Err(_) => None,                         // 裁剪失败：保留原始
                }
            })
            .collect();

        // 写回（按 rowid 更新/删除，走主键索引）
        if !updates.is_empty() {
            let tx = conn.unchecked_transaction()?;
            for (rowid, data) in &updates {
                match data {
                    Some(d) => {
                        tx.execute(
                            "UPDATE tiles SET tile_data=?1 WHERE rowid=?2",
                            params![d, rowid],
                        )?;
                    }
                    None => {
                        tx.execute("DELETE FROM tiles WHERE rowid=?1", params![rowid])?;
                    }
                }
            }
            tx.commit()?;
        }

        // 触发地图 flash：将本批处理的瓦片边界发送给前端
        let flash_bounds: Vec<TileFlashBounds> = batch_data
            .iter()
            .map(|(_, z, x, y, _)| {
                let tb =
                    crate::tile_math::tile_to_lonlat_bounds(*x as u32, *y as u32, *z as u8, crs);
                TileFlashBounds {
                    west: tb.west,
                    east: tb.east,
                    south: tb.south,
                    north: tb.north,
                }
            })
            .collect();
        tile_flash_cb(flash_bounds);

        processed += chunk.len() as u64;
        progress_cb(processed, boundary_total);
    }

    // 写入裁剪完成标记
    conn.execute(
        "INSERT OR REPLACE INTO metadata (name, value) VALUES ('tiles.clipped', '1')",
        [],
    )?;

    Ok(())
}

// ─── GCJ02 纠偏合成 ────────────────────────────────────────────────────────────

/// 对已下载的 GCJ02（高德）瓦片进行纠偏合成，将纠偏结果原地写回存储。
///
/// # 原理
/// 高德等 GCJ02 来源的瓦片内容相对 WGS84 存在系统性偏移（约 100–700 m）。
/// 本函数对 `orig_bounds` 内的每块 WGS84 目标瓦片 (z, x, y)：
/// 1. 计算该瓦片中心的 GCJ02 像素偏移 `(dx, dy)`；
/// 2. 以偏移量确定 2×2 来源 Gaode 瓦片并读取（来源瓦片因下载时扩边已存在）；
/// 3. 合成为 256×256 WGS84 对齐图像，编码为 PNG 并原地覆写；
/// 4. 所有合成完成后删除扩边区（GCJ02_PAD）瓦片，更新格式元数据为 `png`。
///
/// # 处理顺序安全性
/// 在中国境内 GCJ02 偏移始终向东（dx ≥ 0）且向北（dy ≤ 0），故来源瓦片始终位于
/// 目标瓦片的东方和/或北方。按 (x 升序, y 降序) 批处理时，来源行列号均不小于
/// 当前批次的最大处理行列号，保证批间读写不发生污染。批内以"先全部读源、再全部写"
/// 模式执行，同样无污染。
fn post_gcj02_composite(
    store_path: &str,
    orig_bounds: &Bounds,
    min_zoom: u8,
    max_zoom: u8,
    crs: &crate::types::CrsType,
    progress_cb: impl Fn(u64, u64),
) -> Result<()> {
    use rayon::prelude::*;
    use rusqlite::{params, Connection};
    use std::collections::HashMap;

    use image::RgbaImage;

    let conn = Connection::open(store_path)?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;
         PRAGMA cache_size=-204800;
         PRAGMA mmap_size=536870912;
         PRAGMA temp_store=MEMORY;
         PRAGMA busy_timeout=10000;",
    )?;

    // 幂等保护：已合成过则直接跳过
    let already_done: bool = conn
        .query_row(
            "SELECT value FROM metadata WHERE name='tiles.gcj02_composited'",
            [],
            |r| r.get::<_, String>(0),
        )
        .ok()
        .map(|v| v == "1")
        .unwrap_or(false);
    if already_done {
        return Ok(());
    }

    // 枚举原始范围内的所有目标瓦片（逐层级，避免单次枚举超出上限）
    let mut target_tiles: Vec<(u8, u32, u32)> = Vec::new();
    for z in min_zoom..=max_zoom {
        let tiles =
            crate::tile_math::enumerate_tiles(orig_bounds, z, z, crs, Some(10_000_000));
        for tc in tiles {
            target_tiles.push((tc.z, tc.x, tc.y));
        }
    }

    // 按 (x 升序, y 降序) 排序保证合成顺序安全（见函数文档注释）
    target_tiles.sort_unstable_by(|a, b| a.1.cmp(&b.1).then(b.2.cmp(&a.2)));

    let total = target_tiles.len() as u64;
    if total == 0 {
        return Ok(());
    }
    progress_cb(0, total);

    const BATCH_SIZE: usize = 200;
    let mut processed: u64 = 0;

    for chunk in target_tiles.chunks(BATCH_SIZE) {
        // 1. 计算每块目标瓦片的偏移量，收集所需源瓦片坐标
        struct TileInfo {
            z: u8,
            x: u32,
            y: u32,
            tile_off_x: i32,
            tile_off_y: i32,
            sub_x: i32,
            sub_y: i32,
        }
        let tile_infos: Vec<TileInfo> = chunk
            .iter()
            .map(|&(z, x, y)| {
                let (dx, dy) = crate::gcj02::gcj02_pixel_delta(z, x, y);
                TileInfo {
                    z,
                    x,
                    y,
                    tile_off_x: dx.div_euclid(256),
                    tile_off_y: dy.div_euclid(256),
                    sub_x: dx.rem_euclid(256),
                    sub_y: dy.rem_euclid(256),
                }
            })
            .collect();

        let mut source_keys: Vec<(u8, i64, i64)> = Vec::new();
        for ti in &tile_infos {
            for j in 0i64..2 {
                for i in 0i64..2 {
                    let sx = ti.x as i64 + ti.tile_off_x as i64 + i;
                    let sy = ti.y as i64 + ti.tile_off_y as i64 + j;
                    if sx >= 0 && sy >= 0 {
                        source_keys.push((ti.z, sx, sy));
                    }
                }
            }
        }
        source_keys.sort_unstable();
        source_keys.dedup();

        // 2. 批量读取源瓦片 → HashMap
        let source_data: HashMap<(u8, i64, i64), Vec<u8>> = source_keys
            .iter()
            .filter_map(|&(z, sx, sy)| {
                conn.query_row(
                    "SELECT tile_data FROM tiles \
                     WHERE zoom_level=?1 AND tile_column=?2 AND tile_row=?3",
                    params![z as i64, sx, sy],
                    |r| r.get::<_, Vec<u8>>(0),
                )
                .ok()
                .map(|data| ((z, sx, sy), data))
            })
            .collect();

        // 3. 并行合成（source_data 为只读共享，rayon 安全）
        let results: Vec<(u8, u32, u32, Vec<u8>)> = tile_infos
            .par_iter()
            .filter_map(|ti| {
                let mut canvas = RgbaImage::new(256, 256);
                let mut has_data = false;

                for j in 0i32..2 {
                    for i in 0i32..2 {
                        let sx = ti.x as i64 + ti.tile_off_x as i64 + i as i64;
                        let sy = ti.y as i64 + ti.tile_off_y as i64 + j as i64;
                        if sx < 0 || sy < 0 {
                            continue;
                        }
                        if let Some(data) = source_data.get(&(ti.z, sx, sy)) {
                            if let Ok(img) = image::load_from_memory(data) {
                                let src = img.into_rgba8();
                                // 源瓦片在画布上的左上角（可为负值）
                                let cx = i * 256 - ti.sub_x;
                                let cy = j * 256 - ti.sub_y;
                                // 计算源与目标的有效重叠区域
                                let dst_x0 = cx.max(0) as u32;
                                let dst_y0 = cy.max(0) as u32;
                                let src_x0 = (-cx).max(0) as u32;
                                let src_y0 = (-cy).max(0) as u32;
                                let w = ((256 - src_x0 as i32).min(256 - cx)).max(0) as u32;
                                let h = ((256 - src_y0 as i32).min(256 - cy)).max(0) as u32;
                                for dp in 0..h {
                                    for dq in 0..w {
                                        let px = src.get_pixel(src_x0 + dq, src_y0 + dp);
                                        canvas.put_pixel(dst_x0 + dq, dst_y0 + dp, *px);
                                    }
                                }
                                has_data = true;
                            }
                        }
                    }
                }

                if !has_data {
                    return None;
                }

                crate::export::tile_clip::encode_png_fast(&canvas)
                    .ok()
                    .map(|encoded| (ti.z, ti.x, ti.y, encoded))
            })
            .collect();

        // 4. 写回（事务批量提交）
        if !results.is_empty() {
            let tx = conn.unchecked_transaction()?;
            for (z, x, y, data) in &results {
                tx.execute(
                    "UPDATE tiles SET tile_data=?1 \
                     WHERE zoom_level=?2 AND tile_column=?3 AND tile_row=?4",
                    params![data, *z as i64, *x as i64, *y as i64],
                )?;
            }
            tx.commit()?;
        }

        processed += chunk.len() as u64;
        progress_cb(processed, total);
    }

    // 5. 删除扩边区（GCJ02_PAD）中不属于原始范围的 padding 瓦片
    {
        let tx = conn.unchecked_transaction()?;
        for z in min_zoom..=max_zoom {
            let ((x_min, x_max), (y_min, y_max)) =
                crate::tile_math::bounds_to_tile_range_xyz(orig_bounds, z);
            let args = params![
                z as i64,
                x_min as i64,
                x_max as i64,
                y_min as i64,
                y_max as i64
            ];
            tx.execute(
                "DELETE FROM tiles WHERE zoom_level=?1 \
                 AND (tile_column<?2 OR tile_column>?3 OR tile_row<?4 OR tile_row>?5)",
                args,
            )?;
            tx.execute(
                "DELETE FROM download_state WHERE zoom_level=?1 \
                 AND (tile_column<?2 OR tile_column>?3 OR tile_row<?4 OR tile_row>?5)",
                params![
                    z as i64,
                    x_min as i64,
                    x_max as i64,
                    y_min as i64,
                    y_max as i64
                ],
            )?;
        }
        tx.commit()?;
    }

    // 6. 更新格式元数据（合成输出始终为 PNG）并写入完成标记
    conn.execute(
        "INSERT OR REPLACE INTO metadata (name, value) VALUES ('format', 'png')",
        [],
    )?;
    conn.execute(
        "INSERT OR REPLACE INTO metadata (name, value) VALUES ('tiles.gcj02_composited', '1')",
        [],
    )?;

    Ok(())
}

async fn run_mbtiles_import(
    task_id: String,
    app_db: AppDb,
    app: AppHandle,
    mbtiles_path: String,
    tile_store_path: String,
    ctrl_rx: watch::Receiver<CtrlSignal>,
) {
    app_db.update_task_status(&task_id, "downloading").ok();

    let result = tokio::task::spawn_blocking({
        let task_id = task_id.clone();
        let app_db = app_db.clone();
        let app = app.clone();
        move || -> anyhow::Result<()> {
            use rusqlite::{Connection, OpenFlags};

            let src = Connection::open_with_flags(
                &mbtiles_path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?;

            let total: i64 = src.query_row("SELECT COUNT(*) FROM tiles", [], |r| r.get(0))?;
            app_db.update_task_total(&task_id, total).ok();

            app.emit(
                "tilegrab-progress",
                ProgressPayload {
                    task_id: task_id.clone(),
                    total,
                    downloaded: 0,
                    failed: 0,
                    speed: 0.0,
                    bytes_per_sec: 0.0,
                    eta_secs: None,
                    status: "downloading".into(),
                    retry_in_secs: None,
                },
            )?;

            let mut stmt =
                src.prepare("SELECT zoom_level, tile_column, tile_row, tile_data FROM tiles")?;

            if let Some(parent) = std::path::Path::new(&tile_store_path).parent() {
                std::fs::create_dir_all(parent)?;
            }
            let tile_store = TileStore::open(std::path::Path::new(&tile_store_path), &task_id)?;
            let mut downloaded: i64 = 0;
            let mut failed: i64 = 0;
            let mut rows = stmt.query([])?;

            while let Some(row) = rows.next()? {
                let signal = ctrl_rx.borrow().clone();
                match signal {
                    CtrlSignal::Cancel => {
                        app_db
                            .update_task_progress(&task_id, downloaded, failed)
                            .ok();
                        app_db.update_task_status(&task_id, "cancelled").ok();
                        app.emit(
                            "tilegrab-progress",
                            ProgressPayload {
                                task_id: task_id.clone(),
                                total,
                                downloaded,
                                failed,
                                speed: 0.0,
                                bytes_per_sec: 0.0,
                                eta_secs: None,
                                status: "cancelled".into(),
                                retry_in_secs: None,
                            },
                        )?;
                        return Ok(());
                    }
                    CtrlSignal::Pause => {
                        for _ in 0..300 {
                            std::thread::sleep(std::time::Duration::from_millis(200));
                            if *ctrl_rx.borrow() != CtrlSignal::Pause {
                                break;
                            }
                        }
                        continue;
                    }
                    CtrlSignal::Run => {}
                }

                let z: u8 = u8::try_from(row.get::<_, i32>(0)?)
                    .map_err(|e| anyhow::anyhow!("zoom_level 越界: {e}"))?;
                let x: u32 = u32::try_from(row.get::<_, i32>(1)?)
                    .map_err(|e| anyhow::anyhow!("tile_column 越界: {e}"))?;
                let tms_y: u32 = u32::try_from(row.get::<_, i32>(2)?)
                    .map_err(|e| anyhow::anyhow!("tile_row 越界: {e}"))?;
                let tile_data: Vec<u8> = row.get(3)?;
                let y = (1u32 << z).wrapping_sub(1).wrapping_sub(tms_y);

                match tile_store.save_tile(&crate::tile_math::TileCoord { z, x, y }, &tile_data) {
                    Ok(_) => downloaded += 1,
                    Err(e) => {
                        tracing::warn!(task_id, error = %e, "[mbtiles-import] save tile failed");
                        failed += 1;
                    }
                }

                if (downloaded + failed) % 100 == 0 {
                    app.emit(
                        "tilegrab-progress",
                        ProgressPayload {
                            task_id: task_id.clone(),
                            total,
                            downloaded,
                            failed,
                            speed: 0.0,
                            bytes_per_sec: 0.0,
                            eta_secs: None,
                            status: "downloading".into(),
                            retry_in_secs: None,
                        },
                    )?;
                }
            }

            let final_status = if failed == 0 {
                "completed"
            } else {
                "completed_with_errors"
            };
            app_db.update_task_status(&task_id, final_status).ok();
            app_db
                .update_task_progress(&task_id, downloaded, failed)
                .ok();
            app.emit(
                "tilegrab-progress",
                ProgressPayload {
                    task_id: task_id.clone(),
                    total,
                    downloaded,
                    failed,
                    speed: 0.0,
                    bytes_per_sec: 0.0,
                    eta_secs: None,
                    status: final_status.into(),
                    retry_in_secs: None,
                },
            )?;

            Ok(())
        }
    })
    .await;

    match result {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            tracing::error!(task_id, error = %e, "[engine] mbtiles import error");
            app_db
                .add_log(Some(&task_id), "error", &format!("MBTiles 导入失败: {e}"))
                .ok();
            if let Err(e) = app_db.update_task_status(&task_id, "failed") {
                app_db.soft_err(Some(&task_id), "标记任务失败状态", e);
            }
            app.emit(
                "tilegrab-progress",
                ProgressPayload {
                    task_id: task_id.clone(),
                    total: 0,
                    downloaded: 0,
                    failed: 0,
                    speed: 0.0,
                    bytes_per_sec: 0.0,
                    eta_secs: None,
                    status: "failed".into(),
                    retry_in_secs: None,
                },
            )
            .ok();
        }
        Err(e) => {
            tracing::error!(task_id, error = %e, "[engine] spawn_blocking panicked");
            if let Err(e) = app_db.update_task_status(&task_id, "failed") {
                app_db.soft_err(Some(&task_id), "标记任务失败状态", e);
            }
        }
    }
}
