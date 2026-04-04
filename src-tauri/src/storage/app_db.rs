//! TileGrabber — 主数据库 (app.db)
//!
//! 管理任务、日志、全局设置的 CRUD 操作。

use std::collections::HashMap;
use std::path::Path;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

// ─── 任务状态 ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    Downloading,
    Paused,
    Completed,
    CompletedWithErrors,
    Failed,
    Cancelled,
}

impl TaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Downloading => "downloading",
            Self::Paused => "paused",
            Self::Completed => "completed",
            Self::CompletedWithErrors => "completed_with_errors",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

impl FromStr for TaskStatus {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "pending" => Ok(Self::Pending),
            "downloading" => Ok(Self::Downloading),
            "paused" => Ok(Self::Paused),
            "completed" => Ok(Self::Completed),
            "completed_with_errors" => Ok(Self::CompletedWithErrors),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(anyhow::anyhow!("unknown task status: {}", s)),
        }
    }
}

// ─── 数据结构 ────────────────────────────────────────────────────────────────

/// 完整任务记录（从数据库读取）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub name: String,
    /// JSON 序列化的 TileSource
    pub source_config: String,
    pub status: String,
    pub bounds_west: f64,
    pub bounds_east: f64,
    pub bounds_south: f64,
    pub bounds_north: f64,
    pub min_zoom: u8,
    pub max_zoom: u8,
    pub total_tiles: i64,
    pub downloaded_tiles: i64,
    pub failed_tiles: i64,
    pub tile_store_path: Option<String>,
    /// 是否严格裁剪至选框范围（导出时精确裁剪边缘瓦片；下载阶段不影响哪些瓦片被下载）
    pub clip_to_bounds: bool,
    /// 多边形顶点坐标 JSON（[[lng, lat], ...]），非矩形框选时有值；None 表示矩形或导入 bbox
    pub polygon_wgs84: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// 创建任务时的参数
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewTask {
    pub name: String,
    /// JSON 序列化的 TileSource
    pub source_config: String,
    pub bounds_west: f64,
    pub bounds_east: f64,
    pub bounds_south: f64,
    pub bounds_north: f64,
    pub min_zoom: u8,
    pub max_zoom: u8,
    /// 是否严格裁剪至选框范围（仅影响导出阶段的精确裁剪，与下载瓦片范围无关）
    #[serde(default)]
    pub clip_to_bounds: bool,
    /// 多边形顶点坐标 JSON（[[lng, lat], ...]），None 表示矩形范围
    #[serde(default)]
    pub polygon_wgs84: Option<String>,
}

/// 图层记录（从数据库读取）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Layer {
    pub id: String,
    pub name: String,
    /// JSON 序列化的 TileSource（文件已解析，不存文件路径）
    pub source_config: String,
    pub sort_order: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// 创建图层时的参数
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewLayer {
    pub name: String,
    /// JSON 序列化的 TileSource
    pub source_config: String,
}

/// 日志条目
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub id: i64,
    pub task_id: Option<String>,
    pub level: String,
    pub message: String,
    pub timestamp: String,
}

// ─── AppDb ───────────────────────────────────────────────────────────────────

/// 主数据库连接（可 Clone，内部持有 Arc<Mutex<Connection>>）
#[derive(Clone)]
pub struct AppDb(Arc<Mutex<Connection>>);

impl AppDb {
    /// 打开（或创建）app.db
    pub fn open(data_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(data_dir)
            .with_context(|| format!("failed to create data dir: {:?}", data_dir))?;
        let db_path = data_dir.join("app.db");
        let conn = Connection::open(&db_path)
            .with_context(|| format!("failed to open app.db at {:?}", db_path))?;

        // WAL 模式：写时追加，读写互不阻塞
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;

        let db = AppDb(Arc::new(Mutex::new(conn)));
        db.init_tables()?;
        Ok(db)
    }

    fn init_tables(&self) -> Result<()> {
        let conn = self.lock()?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS tasks (
                id                TEXT PRIMARY KEY,
                name              TEXT NOT NULL,
                source_config     TEXT NOT NULL DEFAULT '{}',
                status            TEXT NOT NULL DEFAULT 'pending',
                bounds_west       REAL NOT NULL DEFAULT -180,
                bounds_east       REAL NOT NULL DEFAULT  180,
                bounds_south      REAL NOT NULL DEFAULT  -90,
                bounds_north      REAL NOT NULL DEFAULT   90,
                min_zoom          INTEGER NOT NULL DEFAULT  0,
                max_zoom          INTEGER NOT NULL DEFAULT 18,
                total_tiles       INTEGER NOT NULL DEFAULT  0,
                downloaded_tiles  INTEGER NOT NULL DEFAULT  0,
                failed_tiles      INTEGER NOT NULL DEFAULT  0,
                tile_store_path   TEXT,
                clip_to_bounds    INTEGER NOT NULL DEFAULT 0,
                polygon_wgs84     TEXT,
                created_at        TEXT NOT NULL,
                updated_at        TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS logs (
                id        INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id   TEXT REFERENCES tasks(id) ON DELETE CASCADE,
                level     TEXT NOT NULL DEFAULT 'info',
                message   TEXT NOT NULL,
                timestamp TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS settings (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_logs_task_id ON logs(task_id);
            CREATE INDEX IF NOT EXISTS idx_tasks_status  ON tasks(status);
            CREATE TABLE IF NOT EXISTS layers (
                id            TEXT PRIMARY KEY,
                name          TEXT NOT NULL,
                source_config TEXT NOT NULL,
                sort_order    INTEGER NOT NULL DEFAULT 0,
                created_at    TEXT NOT NULL,
                updated_at    TEXT NOT NULL
            );
            "#,
        )?;
        // 迁移旧数据库：添加 clip_to_bounds 列（忽略已存在时的错误）
        conn.execute_batch(
            "ALTER TABLE tasks ADD COLUMN clip_to_bounds INTEGER NOT NULL DEFAULT 0;"
        ).ok();
        // 迁移旧数据库：添加 polygon_wgs84 列
        conn.execute_batch(
            "ALTER TABLE tasks ADD COLUMN polygon_wgs84 TEXT;"
        ).ok();
        Ok(())
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>> {
        self.0
            .lock()
            .map_err(|_| anyhow::anyhow!("app.db mutex poisoned"))
    }

    // ── 任务 CRUD ─────────────────────────────────────────────────────────────

    /// 创建任务（调用方负责生成 UUID）
    pub fn create_task(&self, id: &str, new_task: &NewTask, tile_store_path: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO tasks
             (id, name, source_config, status,
              bounds_west, bounds_east, bounds_south, bounds_north,
              min_zoom, max_zoom, clip_to_bounds, polygon_wgs84, tile_store_path, created_at, updated_at)
             VALUES (?1,?2,?3,'pending',?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?13)",
            params![
                id,
                new_task.name,
                new_task.source_config,
                new_task.bounds_west,
                new_task.bounds_east,
                new_task.bounds_south,
                new_task.bounds_north,
                new_task.min_zoom as i64,
                new_task.max_zoom as i64,
                new_task.clip_to_bounds as i32,
                new_task.polygon_wgs84,
                tile_store_path,
                now,
            ],
        )?;
        Ok(())
    }

    pub fn get_task(&self, id: &str) -> Result<Task> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT id,name,source_config,status,
                    bounds_west,bounds_east,bounds_south,bounds_north,
                    min_zoom,max_zoom,total_tiles,downloaded_tiles,failed_tiles,
                    tile_store_path,clip_to_bounds,polygon_wgs84,created_at,updated_at
             FROM tasks WHERE id=?1",
            params![id],
            row_to_task,
        )
        .with_context(|| format!("task not found: {}", id))
    }

    pub fn list_tasks(&self) -> Result<Vec<Task>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT id,name,source_config,status,
                    bounds_west,bounds_east,bounds_south,bounds_north,
                    min_zoom,max_zoom,total_tiles,downloaded_tiles,failed_tiles,
                    tile_store_path,clip_to_bounds,polygon_wgs84,created_at,updated_at
             FROM tasks ORDER BY created_at DESC",
        )?;
        let tasks = stmt
            .query_map([], row_to_task)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(tasks)
    }

    pub fn update_task_status(&self, id: &str, status: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let conn = self.lock()?;
        conn.execute(
            "UPDATE tasks SET status=?1, updated_at=?2 WHERE id=?3",
            params![status, now, id],
        )?;
        Ok(())
    }

    pub fn update_task_total(&self, id: &str, total: i64) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let conn = self.lock()?;
        conn.execute(
            "UPDATE tasks SET total_tiles=?1, updated_at=?2 WHERE id=?3",
            params![total, now, id],
        )?;
        Ok(())
    }

    pub fn update_task_progress(&self, id: &str, downloaded: i64, failed: i64) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let conn = self.lock()?;
        conn.execute(
            "UPDATE tasks SET downloaded_tiles=?1, failed_tiles=?2, updated_at=?3 WHERE id=?4",
            params![downloaded, failed, now, id],
        )?;
        Ok(())
    }

    /// 应用重启时调用：将所有遗留的 downloading 状态重置为 paused，
    /// 避免任务卡在 downloading 却无法暂停/取消的问题
    pub fn reset_downloading_to_paused(&self) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let conn = self.lock()?;
        conn.execute(
            "UPDATE tasks SET status='paused', updated_at=?1 WHERE status='downloading'",
            params![now],
        )?;
        Ok(())
    }

    pub fn delete_task(&self, id: &str) -> Result<()> {
        let conn = self.lock()?;
        conn.execute("DELETE FROM tasks WHERE id=?1", params![id])?;
        Ok(())
    }

    // ── 日志 ──────────────────────────────────────────────────────────────────

    pub fn add_log(&self, task_id: Option<&str>, level: &str, message: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO logs (task_id,level,message,timestamp) VALUES (?1,?2,?3,?4)",
            params![task_id, level, message, now],
        )?;
        Ok(())
    }

    pub fn get_task_logs(&self, task_id: &str, limit: u32) -> Result<Vec<LogEntry>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT id,task_id,level,message,timestamp
             FROM logs WHERE task_id=?1 ORDER BY id DESC LIMIT ?2",
        )?;
        let logs = stmt
            .query_map(params![task_id, limit as i64], |row| {
                Ok(LogEntry {
                    id: row.get(0)?,
                    task_id: row.get(1)?,
                    level: row.get(2)?,
                    message: row.get(3)?,
                    timestamp: row.get(4)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(logs)
    }

    // ── 设置 ──────────────────────────────────────────────────────────────────

    /// 读取 settings 表中的全部行（key → value）
    pub fn get_all_settings_raw(&self) -> Result<HashMap<String, String>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare("SELECT key, value FROM settings")?;
        let pairs = stmt
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(pairs.into_iter().collect())
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let conn = self.lock()?;
        match conn.query_row(
            "SELECT value FROM settings WHERE key=?1",
            params![key],
            |row| row.get::<_, String>(0),
        ) {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT OR REPLACE INTO settings (key,value) VALUES (?1,?2)",
            params![key, value],
        )?;
        Ok(())
    }

    // ── 图层 CRUD ─────────────────────────────────────────────────────────────

    /// 创建图层（调用方负责生成 UUID）
    pub fn create_layer(&self, id: &str, new_layer: &NewLayer) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let conn = self.lock()?;
        // sort_order 取当前最大值 + 1
        let max_order: i64 = conn
            .query_row("SELECT COALESCE(MAX(sort_order), -1) FROM layers", [], |r| {
                r.get(0)
            })
            .unwrap_or(-1);
        conn.execute(
            "INSERT INTO layers (id, name, source_config, sort_order, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            params![id, new_layer.name, new_layer.source_config, max_order + 1, now],
        )?;
        Ok(())
    }

    pub fn list_layers(&self) -> Result<Vec<Layer>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, name, source_config, sort_order, created_at, updated_at
             FROM layers ORDER BY sort_order ASC, created_at ASC",
        )?;
        let layers = stmt
            .query_map([], |row| {
                Ok(Layer {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    source_config: row.get(2)?,
                    sort_order: row.get(3)?,
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(layers)
    }

    pub fn rename_layer(&self, id: &str, name: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let conn = self.lock()?;
        conn.execute(
            "UPDATE layers SET name=?1, updated_at=?2 WHERE id=?3",
            params![name, now, id],
        )?;
        Ok(())
    }

    pub fn delete_layer(&self, id: &str) -> Result<()> {
        let conn = self.lock()?;
        conn.execute("DELETE FROM layers WHERE id=?1", params![id])?;
        Ok(())
    }

    /// 批量更新图层排序（传入有序的 id 列表）
    pub fn reorder_layers(&self, ids: &[String]) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let conn = self.lock()?;
        for (i, id) in ids.iter().enumerate() {
            conn.execute(
                "UPDATE layers SET sort_order=?1, updated_at=?2 WHERE id=?3",
                params![i as i64, now, id],
            )?;
        }
        Ok(())
    }
}

// ─── 辅助函数 ────────────────────────────────────────────────────────────────

fn row_to_task(row: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
    Ok(Task {
        id: row.get(0)?,
        name: row.get(1)?,
        source_config: row.get(2)?,
        status: row.get(3)?,
        bounds_west: row.get(4)?,
        bounds_east: row.get(5)?,
        bounds_south: row.get(6)?,
        bounds_north: row.get(7)?,
        min_zoom: row.get::<_, i64>(8)? as u8,
        max_zoom: row.get::<_, i64>(9)? as u8,
        total_tiles: row.get(10)?,
        downloaded_tiles: row.get(11)?,
        failed_tiles: row.get(12)?,
        tile_store_path: row.get(13)?,
        clip_to_bounds: row.get::<_, i32>(14)? != 0,
        polygon_wgs84: row.get(15)?,
        created_at: row.get(16)?,
        updated_at: row.get(17)?,
    })
}
