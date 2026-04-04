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
pub struct DownloadEngine(Arc<Mutex<HashMap<String, TaskHandle>>>);

impl DownloadEngine {
    pub fn new() -> Self {
        DownloadEngine(Arc::new(Mutex::new(HashMap::new())))
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
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("engine lock poisoned"))?;

        // 如果已存在（正在运行），直接返回
        if handles.contains_key(&task_id) {
            return Ok(());
        }

        let (ctrl_tx, ctrl_rx) = watch::channel(CtrlSignal::Run);
        handles.insert(task_id.clone(), TaskHandle { ctrl_tx });

        // 任务结束后自动清理句柄
        let engine_ref = self.0.clone();
        let tid = task_id.clone();

        tokio::spawn(async move {
            run_download(task_id, app_db, concurrency, ctrl_rx, app).await;
            if let Ok(mut h) = engine_ref.lock() {
                h.remove(&tid);
            }
        });

        Ok(())
    }

    /// 暂停指定任务
    pub fn pause(&self, task_id: &str) -> Result<()> {
        let handles = self
            .0
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
            .0
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
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("engine lock poisoned"))?;
        if let Some(h) = handles.get(task_id) {
            let _ = h.ctrl_tx.send(CtrlSignal::Cancel);
        }
        Ok(())
    }

    /// 查询任务是否正在运行
    pub fn is_active(&self, task_id: &str) -> bool {
        self.0
            .lock()
            .map(|h| h.contains_key(task_id))
            .unwrap_or(false)
    }
}

// ─── 下载主循环 ──────────────────────────────────────────────────────────────

async fn run_download(
    task_id: String,
    app_db: AppDb,
    concurrency: usize,
    mut ctrl_rx: watch::Receiver<CtrlSignal>,
    app: AppHandle,
) {
    // 1. 从数据库加载任务信息
    app_db.add_log(Some(&task_id), "info", "开始下载任务").ok();
    let task = match app_db.get_task(&task_id) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("[engine] cannot load task {}: {}", task_id, e);
            app_db.add_log(Some(&task_id), "error", &format!("加载任务失败: {}", e)).ok();
            return;
        }
    };

    // 2. 解析 TileSource
    let source: TileSource = match serde_json::from_str(&task.source_config) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[engine] invalid source_config: {}", e);
            app_db.add_log(Some(&task_id), "error", &format!("数据源配置解析失败: {}", e)).ok();
            app_db.update_task_status(&task_id, "failed").ok();
            return;
        }
    };

    // 3. 打开瓦片存储（路径在任务创建时已持久化到 DB）
    let tile_store_path = match task.tile_store_path.as_deref() {
        Some(p) => p.to_owned(),
        None => {
            app_db.add_log(Some(&task_id), "error", "任务缺少瓦片存储路径").ok();
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
            eprintln!("[engine] cannot open tile store: {}", e);
            app_db.update_task_status(&task_id, "failed").ok();
            return;
        }
    };

    // 4. 将"下载中"回退为 pending（断点续传支持）
    tile_store.reset_stale_downloading().ok();

    // 5. 首次运行：枚举瓦片并写入 download_state
    let init_total = tile_store
        .get_progress()
        .map(|p| p.total)
        .unwrap_or(0);

    if init_total == 0 {
        let bounds = Bounds {
            west: task.bounds_west,
            east: task.bounds_east,
            south: task.bounds_south,
            north: task.bounds_north,
        };

        // 若任务附带多边形范围，仅枚举与多边形相交的瓦片，跳过外包围矩形中多余的瓦片
        let polygon: Option<Vec<[f64; 2]>> = task.polygon_wgs84
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok());

        let tiles = if let Some(ref poly) = polygon {
            app_db.add_log(
                Some(&task_id), "info",
                "已检测到多边形范围，将按多边形过滤下载瓦片",
            ).ok();
            enumerate_tiles_with_polygon(
                &bounds, task.min_zoom, task.max_zoom, &source.crs, poly, Some(2_000_000),
            )
        } else {
            enumerate_tiles(
                &bounds, task.min_zoom, task.max_zoom, &source.crs, Some(2_000_000),
            )
        };
        match tile_store.init_download_state(&tiles) {
            Ok(total) => {
                app_db.update_task_total(&task_id, total).ok();
                app_db.add_log(Some(&task_id), "info", &format!("共枚举 {} 个瓦片，z{}-z{}", total, task.min_zoom, task.max_zoom)).ok();
            }
            Err(e) => {
                eprintln!("[engine] init_download_state failed: {}", e);
                app_db.add_log(Some(&task_id), "error", &format!("初始化瓦片列表失败: {}", e)).ok();
                app_db.update_task_status(&task_id, "failed").ok();
                return;
            }
        }
    }

    // 6. 加载下载规则（时间窗口 + 速率限制）
    let rules = DownloadRules::load(&app_db);

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
    let client = match worker::build_client(&source.headers) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[engine] cannot build http client: {}", e);
            return;
        }
    };

    app_db.update_task_status(&task_id, "downloading").ok();

    // 速率限制：每瓦片最小间隔（由规则计算得出，0 = 不限速）
    let tile_delay_ms = rules.per_tile_delay_ms();

    // 立即发送初始进度事件，前端立刻将任务状态改为 "downloading"
    let init_prog = tile_store.get_progress().unwrap_or_default();
    let _ = app.emit(
        "tilegrab-progress",
        ProgressPayload {
            task_id: task_id.clone(),
            total: init_prog.total,
            downloaded: init_prog.downloaded,
            failed: init_prog.failed,
            speed: 0.0,
            bytes_per_sec: 0.0,
            eta_secs: None,
            status: "downloading".to_string(),
        },
    );

    let sem = Arc::new(Semaphore::new(concurrency.max(1)));
    let mut last_downloaded: i64 = 0;
    let mut last_tick = Instant::now();
    let mut last_bytes: u64 = 0; // 每个计时周期内下载的总字节数
    let mut batch_counter: u32 = 0; // 批次计数器，用于视口停顿模拟

    // === 流水线写入通道 ===
    // 单独的 tokio 任务负责 DB 写入 + 事件发送，主循环只管下载
    let (write_tx, mut write_rx) = tokio::sync::mpsc::channel::<WriteBatchMsg>(4);
    let write_store = tile_store.clone();
    let write_app_db = app_db.clone();
    let write_app = app.clone();
    let write_task_id = task_id.clone();
    let write_handle = tokio::spawn(async move {
        while let Some(msg) = write_rx.recv().await {
            let WriteBatchMsg { success_tiles, failed_tiles, flash_tiles } = msg;
            // 批量写入成功的瓦片（单事务）——先保存原始数据，下载完成后再统一裁剪
            if !success_tiles.is_empty() {
                let store = write_store.clone();
                tokio::task::spawn_blocking(move || {
                    store.save_tiles_batch(&success_tiles).ok();
                }).await.ok();
            }
            // 批量标记失败（单事务）
            if !failed_tiles.is_empty() {
                for (coord, err) in &failed_tiles {
                    let log_msg = format!("瓦片 z{}/x{}/y{} 下载失败: {}", coord.z, coord.x, coord.y, err);
                    write_app_db.add_log(Some(&write_task_id), "warn", &log_msg).ok();
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
                        let _ = app.emit(
                            "tilegrab-progress",
                            ProgressPayload {
                                task_id: task_id.clone(),
                                total: p.total,
                                downloaded: p.downloaded,
                                failed: p.failed,
                                speed: 0.0,
                                bytes_per_sec: 0.0,
                                eta_secs: None,
                                status: "paused".to_string(),
                            },
                        );
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
                eprintln!("[engine] get_pending_batch error: {}", e);
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
            let permit = sem.clone().acquire_owned().await.unwrap();
            let client = client.clone();
            let source = source.clone();
            let ctrl = ctrl_rx.clone();

            join_set.spawn(async move {
                let _permit = permit;
                if *ctrl.borrow() == CtrlSignal::Cancel {
                    return (tile, Err::<Vec<u8>, anyhow::Error>(anyhow::anyhow!("cancelled")));
                }
                // 瓦片间随机微延迟（防封禁）
                throttle::random_delay(0, 8).await;
                // 速率限制额外延迟
                if tile_delay_ms > 0 {
                    tokio::time::sleep(tokio::time::Duration::from_millis(tile_delay_ms)).await;
                }
                (tile, worker::download_tile(&client, tile, &source).await)
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
                    let b = crate::tile_math::tile_to_lonlat_bounds(coord.x, coord.y, coord.z, &source.crs);
                    flash_tiles.push(TileFlashBounds { west: b.west, east: b.east, south: b.south, north: b.north });
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
        let batch_downloaded = success_tiles.len() as i64;
        let _batch_failed = failed_tiles.len() as i64;
        let _ = write_tx.send(WriteBatchMsg {
            success_tiles,
            failed_tiles,
            flash_tiles,
        }).await;

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
                let speed = if elapsed > 0.1 { delta as f64 / elapsed } else { 0.0 };
                let remaining = progress.pending + progress.failed;
                let eta_secs = if speed > 0.1 {
                    Some(remaining as f64 / speed)
                } else {
                    None
                };

                let bytes_per_sec = if elapsed > 0.1 { last_bytes as f64 / elapsed } else { 0.0 };
                last_downloaded = progress.downloaded;
                last_tick = Instant::now();
                last_bytes = 0;

                app_db
                    .update_task_progress(&task_id, progress.downloaded, progress.failed)
                    .ok();

                let _ = app.emit(
                    "tilegrab-progress",
                    ProgressPayload {
                        task_id: task_id.clone(),
                        total: progress.total,
                        downloaded: progress.downloaded,
                        failed: progress.failed,
                        speed,
                        bytes_per_sec,
                        eta_secs,
                        status: "downloading".to_string(),
                    },
                );
            }
        }
    }

    // 关闭写入通道，等待后台写入完成
    drop(write_tx);
    write_handle.await.ok();

    // 补发下载完成（100%）事件：下载循环的最后一次 tick 可能还未到 100%，
    // 若紧接着进入裁剪阶段，进度条会直接从未满跳到 0%。
    // 这里先让进度条填满再切换状态，用户体验更连贯。
    {
        let done_progress = tile_store.get_progress().unwrap_or_default();
        let _ = app.emit(
            "tilegrab-progress",
            ProgressPayload {
                task_id: task_id.clone(),
                total: done_progress.total,
                downloaded: done_progress.downloaded,
                failed: done_progress.failed,
                speed: 0.0,
                bytes_per_sec: 0.0,
                eta_secs: None,
                status: "downloading".to_string(),
            },
        );
    }

    // ── 下载完成后的精确裁剪 ─────────────────────────────────────────────────
    // 先保存原始瓦片，确保数据完整；下载全部结束后再统一做像素级裁剪，
    // 这样既不影响下载速度，又能保证裁剪结果一致。
    if task.clip_to_bounds && *ctrl_rx.borrow() != CtrlSignal::Cancel {
        app_db.add_log(Some(&task_id), "info", "开始精确裁剪瓦片…").ok();
        app_db.update_task_status(&task_id, "processing").ok();

        // 查询边界瓦片总数，用于裁剪进度（第一遍扫描完后才知道，这里先发 0/0 的占位事件）
        // 立即发送裁剪启动事件，让前端切换到"裁剪中"状态；
        // downloaded 保持 total 使进度条暂时停在 100%（等第一遍扫描完毕后再重置）
        let download_total: i64 = tile_store
            .get_progress()
            .map(|p| p.total)
            .unwrap_or(0);
        let _ = app.emit(
            "tilegrab-progress",
            ProgressPayload {
                task_id: task_id.clone(),
                total: download_total,
                downloaded: download_total,
                failed: 0,
                speed: 0.0,
                bytes_per_sec: 0.0,
                eta_secs: None,
                status: "processing".to_string(),
            },
        );

        let clip_bounds = Bounds {
            west:  task.bounds_west,
            east:  task.bounds_east,
            south: task.bounds_south,
            north: task.bounds_north,
        };
        let clip_polygon: Option<Vec<[f64; 2]>> = task.polygon_wgs84
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok());
        let clip_crs = source.crs.clone();
        let clip_store_path = tile_store_path.clone();
        let clip_app = app.clone();
        let clip_task_id = task_id.clone();

        let clip_result = tokio::task::spawn_blocking(move || {
            post_clip_tiles(
                &clip_store_path,
                &clip_bounds,
                clip_polygon.as_deref(),
                &clip_crs,
                |done, total| {
                    let _ = clip_app.emit(
                        "tilegrab-progress",
                        ProgressPayload {
                            task_id: clip_task_id.clone(),
                            total: total as i64,
                            downloaded: done as i64,
                            failed: 0,
                            speed: 0.0,
                            bytes_per_sec: 0.0,
                            eta_secs: None,
                            status: "processing".to_string(),
                        },
                    );
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
    let summary = format!(
        "任务结束 [{}]：已下载 {}，失败 {}，共 {}",
        final_status, progress.downloaded, progress.failed, progress.total
    );
    app_db.add_log(Some(&task_id), "info", &summary).ok();
    let _ = app.emit(
        "tilegrab-progress",
        ProgressPayload {
            task_id,
            total: progress.total,
            downloaded: progress.downloaded,
            failed: progress.failed,
            speed: 0.0,
            bytes_per_sec: 0.0,
            eta_secs: None,
            status: final_status.to_string(),
        },
    );
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
    let total: u64 = conn
        .query_row("SELECT COUNT(*) FROM tiles", [], |r| r.get::<_, i64>(0))?
        as u64;
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
            let tb = crate::tile_math::tile_to_lonlat_bounds(
                *x as u32, *y as u32, *z as u8, crs,
            );
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
                !(tb.west  >= bounds.west
                    && tb.east  <= bounds.east
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
            let param_refs: Vec<&dyn rusqlite::types::ToSql> =
                chunk.iter().map(|r| r as &dyn rusqlite::types::ToSql).collect();
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
                    Ok(None)    => Some((*rowid, None)),      // 完全在范围外：删除
                    Err(_)      => None,                       // 裁剪失败：保留原始
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
                        tx.execute(
                            "DELETE FROM tiles WHERE rowid=?1",
                            params![rowid],
                        )?;
                    }
                }
            }
            tx.commit()?;
        }

        // 触发地图 flash：将本批处理的瓦片边界发送给前端
        let flash_bounds: Vec<TileFlashBounds> = batch_data
            .iter()
            .map(|(_, z, x, y, _)| {
                let tb = crate::tile_math::tile_to_lonlat_bounds(
                    *x as u32, *y as u32, *z as u8, crs,
                );
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
