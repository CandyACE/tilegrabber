//! 流水线裁剪：下载过程中并发完成边界瓦片的像素级裁剪。
//!
//! 设计：
//! - 主下载循环通过 mpsc 通道把刚保存到 `tiles` 表的瓦片坐标发送给本模块的
//!   背景消费者；消费者用独立 SQLite 连接读取 BLOB、用 rayon 并行裁剪、批量
//!   写回 UPDATE/DELETE。
//! - 仅对"边界瓦片"做工作；完全在 bounds/polygon 内的瓦片跳过。
//! - 下载结束（sender 全部 drop）后消费者排空通道，写入 `tiles.clipped='1'`
//!   元数据，主循环可据此跳过后处理的 `post_clip_tiles` 全量扫描。
//! - 与 GCJ02 纠偏合成不兼容（合成必须先于裁剪），调用方负责门控。

use std::time::Duration;

use anyhow::Result;
use rayon::prelude::*;
use rusqlite::{params, Connection};
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::task::JoinHandle;

use super::engine::{ProgressPayload, TileFlashBounds, TileFlashPayload};
use crate::tile_math::TileCoord;
use crate::types::{Bounds, CrsType};

/// 通道消息：刚成功落盘的瓦片坐标（消费者按需读取 BLOB）。
pub type ClipMsg = Vec<TileCoord>;

/// 消费者运行结果（用于决定是否跳过后处理）。
#[derive(Debug, Clone, PartialEq)]
pub enum ClipOutcome {
    /// 流水线完整执行；`boundary_total` 是累计裁剪的边界瓦片数。
    Completed { boundary_total: u64 },
    /// 流水线被禁用或未启用（调用方应继续走后处理）。
    Disabled,
    /// 出错或被中止，未写入完成标记。
    Failed(String),
}

#[derive(Clone)]
pub struct ClipPipelineConfig {
    pub store_path: String,
    pub bounds: Bounds,
    pub polygon: Option<Vec<[f64; 2]>>,
    pub crs: CrsType,
    pub task_id: String,
}

/// 判断单个瓦片是否需要裁剪（边界瓦片）。
fn is_boundary(coord: &TileCoord, cfg: &ClipPipelineConfig) -> bool {
    let tb = crate::tile_math::tile_to_lonlat_bounds(coord.x, coord.y, coord.z, &cfg.crs);
    if let Some(poly) = &cfg.polygon {
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
        let b = &cfg.bounds;
        !(tb.west >= b.west && tb.east <= b.east && tb.south >= b.south && tb.north <= b.north)
    }
}

/// 从 DB 批量读取 (rowid, z, x, y, data)。
fn fetch_batch(
    conn: &Connection,
    coords: &[TileCoord],
) -> Result<Vec<(i64, u8, u32, u32, Vec<u8>)>> {
    if coords.is_empty() {
        return Ok(vec![]);
    }
    let placeholders: String = (1..=coords.len() * 3)
        .step_by(3)
        .map(|i| format!("(?{},?{},?{})", i, i + 1, i + 2))
        .collect::<Vec<_>>()
        .join(",");
    // SQLite 不直接支持 IN ((a,b,c), ...) 元组，也不支持 `(VALUES ...) AS k(z,x,y)`
    // 形式的列别名；改用 CTE：`WITH k(z,x,y) AS (VALUES (?,?,?), ...)`。
    let sql = format!(
        "WITH k(z,x,y) AS (VALUES {placeholders})
         SELECT t.rowid, t.zoom_level, t.tile_column, t.tile_row, t.tile_data
         FROM tiles t
         JOIN k ON t.zoom_level=k.z AND t.tile_column=k.x AND t.tile_row=k.y"
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut binds: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::with_capacity(coords.len() * 3);
    for c in coords {
        binds.push(Box::new(c.z as i64));
        binds.push(Box::new(c.x as i64));
        binds.push(Box::new(c.y as i64));
    }
    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        binds.iter().map(|b| b.as_ref() as &dyn rusqlite::types::ToSql).collect();
    let rows = stmt.query_map(param_refs.as_slice(), |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)? as u8,
            row.get::<_, i64>(2)? as u32,
            row.get::<_, i64>(3)? as u32,
            row.get::<_, Vec<u8>>(4)?,
        ))
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// 同步消费循环（运行在 spawn_blocking 中）。
fn run_consumer_blocking(
    cfg: ClipPipelineConfig,
    app: AppHandle,
    broadcast_tx: Option<tokio::sync::broadcast::Sender<ProgressPayload>>,
    mut rx: UnboundedReceiver<ClipMsg>,
) -> Result<u64> {
    let mut conn = Connection::open(&cfg.store_path)?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;
         PRAGMA cache_size=-65536;
         PRAGMA temp_store=MEMORY;
         PRAGMA busy_timeout=15000;",
    )?;

    // 累计待处理的边界瓦片
    let mut pending: Vec<TileCoord> = Vec::with_capacity(512);
    let mut total_boundary: u64 = 0;
    let mut processed: u64 = 0;
    const BATCH: usize = 256;

    let flush_batch = |conn: &mut Connection,
                       app: &AppHandle,
                       processed: &mut u64,
                       total_boundary: u64,
                       batch: &[TileCoord]|
     -> Result<()> {
        if batch.is_empty() {
            return Ok(());
        }
        let fetched = fetch_batch(conn, batch)?;
        if fetched.is_empty() {
            return Ok(());
        }
        let bounds = cfg.bounds.clone();
        let polygon = cfg.polygon.clone();
        let crs = cfg.crs.clone();
        let updates: Vec<(i64, Option<Vec<u8>>)> = fetched
            .par_iter()
            .filter_map(|(rowid, z, x, y, data)| {
                let r = if let Some(poly) = &polygon {
                    crate::export::tile_clip::clip_tile_to_polygon_crs(
                        data, *x, *y, *z, poly, &crs,
                    )
                } else {
                    crate::export::tile_clip::clip_tile_to_bounds_crs(
                        data, *x, *y, *z, &bounds, &crs,
                    )
                };
                match r {
                    Ok(Some(d)) => Some((*rowid, Some(d))),
                    Ok(None) => Some((*rowid, None)),
                    Err(_) => None,
                }
            })
            .collect();

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

        // flash 事件：通知前端"裁剪了哪些瓦片"
        let flash: Vec<TileFlashBounds> = fetched
            .iter()
            .map(|(_, z, x, y, _)| {
                let tb = crate::tile_math::tile_to_lonlat_bounds(*x, *y, *z, &cfg.crs);
                TileFlashBounds {
                    west: tb.west,
                    east: tb.east,
                    south: tb.south,
                    north: tb.north,
                }
            })
            .collect();
        if !flash.is_empty() {
            let _ = app.emit(
                "tilegrab-clip-tiles",
                TileFlashPayload {
                    task_id: cfg.task_id.clone(),
                    tiles: flash,
                },
            );
        }

        *processed += fetched.len() as u64;
        // 流水线裁剪进度事件（独立于主下载进度）
        let payload = ClipProgressPayload {
            task_id: cfg.task_id.clone(),
            clipped: *processed as i64,
            boundary_total: total_boundary as i64,
        };
        let _ = app.emit("tilegrab-clip-progress", payload);
        Ok(())
    };

    // 接收循环：tokio mpsc 在阻塞线程内用 blocking_recv()
    loop {
        let msg = match rx.blocking_recv() {
            Some(m) => m,
            None => break, // 所有 sender 已 drop
        };

        // 仅保留边界瓦片
        for coord in msg {
            if is_boundary(&coord, &cfg) {
                pending.push(coord);
                total_boundary += 1;
            }
        }

        // 达到批量阈值则刷写
        while pending.len() >= BATCH {
            let batch: Vec<TileCoord> = pending.drain(..BATCH.min(pending.len())).collect();
            if let Err(e) = flush_batch(&mut conn, &app, &mut processed, total_boundary, &batch) {
                eprintln!("[clip_pipeline] flush error: {}", e);
            }
        }

        // 让出 CPU 防止饱占；同时给主下载循环喘息时机
        std::thread::sleep(Duration::from_millis(0));
    }

    // 排空剩余
    if !pending.is_empty() {
        let batch: Vec<TileCoord> = pending.drain(..).collect();
        if let Err(e) = flush_batch(&mut conn, &app, &mut processed, total_boundary, &batch) {
            eprintln!("[clip_pipeline] final flush error: {}", e);
        }
    }

    // 通知前端流水线裁剪已结束（不在此处写 tiles.clipped 标记——
    // 由 engine.rs 在确认下载未被取消后写入，避免半途取消时遗留错误标记）
    let payload = ClipProgressPayload {
        task_id: cfg.task_id.clone(),
        clipped: processed as i64,
        boundary_total: total_boundary as i64,
    };
    let _ = app.emit("tilegrab-clip-progress-done", payload);
    let _ = broadcast_tx; // 当前实现不再借用主进度通道

    Ok(total_boundary)
}

/// 流水线裁剪进度事件 payload（`tilegrab-clip-progress`）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct ClipProgressPayload {
    pub task_id: String,
    pub clipped: i64,
    pub boundary_total: i64,
}

/// 启动流水线裁剪消费者。
///
/// 返回 `(sender, join_handle)`。调用方在每次成功落盘后通过 sender 发送坐标；
/// 下载循环结束时 drop sender，然后 `await join_handle` 得到最终结果。
pub fn spawn(
    cfg: ClipPipelineConfig,
    app: AppHandle,
    broadcast_tx: Option<tokio::sync::broadcast::Sender<ProgressPayload>>,
) -> (
    tokio::sync::mpsc::UnboundedSender<ClipMsg>,
    JoinHandle<ClipOutcome>,
) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<ClipMsg>();
    let handle = tokio::task::spawn_blocking(move || {
        match run_consumer_blocking(cfg, app, broadcast_tx, rx) {
            Ok(boundary_total) => ClipOutcome::Completed { boundary_total },
            Err(e) => {
                eprintln!("[clip_pipeline] consumer terminated with error: {}", e);
                ClipOutcome::Failed(e.to_string())
            }
        }
    });
    (tx, handle)
}
