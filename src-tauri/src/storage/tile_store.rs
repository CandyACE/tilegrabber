//! TileGrabber — 单任务瓦片存储 ({task_id}.tiles)
//!
//! 每个下载任务独立的 SQLite 文件，存储：
//! - 已下载的瓦片二进制数据 (tiles 表)
//! - 每块瓦片的下载状态 (download_state 表，支持断点续传)

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::tile_math::TileCoord;

// ─── 进度统计 ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TileProgress {
    pub total: i64,
    pub downloaded: i64,
    pub failed: i64,
    pub pending: i64,
}

// ─── TileStore ───────────────────────────────────────────────────────────────

/// 单任务瓦片存储（可 Clone，内部持有 Arc<Mutex<Connection>>）
#[derive(Clone)]
pub struct TileStore {
    conn: Arc<Mutex<Connection>>,
    pub task_id: String,
}

impl TileStore {
    /// 打开（或创建）一个任务的瓦片存储文件
    pub fn open(path: &Path, task_id: &str) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)
            .with_context(|| format!("failed to open tile store at {:?}", path))?;

        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=OFF;
             PRAGMA cache_size=-200000;
             PRAGMA mmap_size=268435456;
             PRAGMA temp_store=MEMORY;",
        )?;

        let store = TileStore {
            conn: Arc::new(Mutex::new(conn)),
            task_id: task_id.to_string(),
        };
        store.init_tables()?;
        Ok(store)
    }

    fn init_tables(&self) -> Result<()> {
        let conn = self.lock()?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS metadata (
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
                zoom_level    INTEGER NOT NULL,
                tile_column   INTEGER NOT NULL,
                tile_row      INTEGER NOT NULL,
                status        TEXT    NOT NULL DEFAULT 'pending',
                retry_count   INTEGER NOT NULL DEFAULT 0,
                error_message TEXT,
                PRIMARY KEY (zoom_level, tile_column, tile_row)
            );
            CREATE INDEX IF NOT EXISTS idx_ds_status
                ON download_state(status, zoom_level);
            "#,
        )?;
        Ok(())
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|_| anyhow::anyhow!("tile store mutex poisoned"))
    }

    // ── 下载状态管理 ──────────────────────────────────────────────────────────

    /// 批量初始化下载状态（INSERT OR IGNORE，幂等，支持断点续传）
    /// 返回当前 download_state 总行数
    pub fn init_download_state(&self, tiles: &[TileCoord]) -> Result<i64> {
        {
            let mut conn = self.lock()?;
            let tx = conn.transaction()?;
            {
                let mut stmt = tx.prepare(
                    "INSERT OR IGNORE INTO download_state
                     (zoom_level, tile_column, tile_row, status)
                     VALUES (?1, ?2, ?3, 'pending')",
                )?;
                for tile in tiles {
                    stmt.execute(params![
                        tile.z as i64,
                        tile.x as i64,
                        tile.y as i64
                    ])?;
                }
            }
            tx.commit()?;
        }
        let total: i64 = self
            .lock()?
            .query_row("SELECT COUNT(*) FROM download_state", [], |r| r.get(0))?;
        Ok(total)
    }

    /// 获取下一批待下载瓦片（无排序，最大化查询速度）
    pub fn get_pending_batch(&self, limit: usize) -> Result<Vec<TileCoord>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare_cached(
            "SELECT zoom_level, tile_column, tile_row
             FROM download_state
             WHERE status IN ('pending', 'downloading')
             LIMIT ?1",
        )?;
        let tiles = stmt
            .query_map(params![limit as i64], |row| {
                Ok(TileCoord {
                    z: row.get::<_, i64>(0)? as u8,
                    x: row.get::<_, i64>(1)? as u32,
                    y: row.get::<_, i64>(2)? as u32,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(tiles)
    }

    /// 标记一批瓦片为"下载中"（防止同一批次重复领取）
    pub fn mark_downloading(&self, tiles: &[TileCoord]) -> Result<()> {
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "UPDATE download_state SET status='downloading'
                 WHERE zoom_level=?1 AND tile_column=?2 AND tile_row=?3
                   AND status='pending'",
            )?;
            for tile in tiles {
                stmt.execute(params![tile.z as i64, tile.x as i64, tile.y as i64])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// 保存已下载的瓦片数据，同时将下载状态更新为 downloaded
    pub fn save_tile(&self, coord: &TileCoord, data: &[u8]) -> Result<()> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT OR REPLACE INTO tiles (zoom_level, tile_column, tile_row, tile_data)
             VALUES (?1, ?2, ?3, ?4)",
            params![coord.z as i64, coord.x as i64, coord.y as i64, data],
        )?;
        conn.execute(
            "UPDATE download_state SET status='downloaded', error_message=NULL
             WHERE zoom_level=?1 AND tile_column=?2 AND tile_row=?3",
            params![coord.z as i64, coord.x as i64, coord.y as i64],
        )?;
        Ok(())
    }

    /// 批量保存已下载的瓦片数据（单个事务，极大减少 I/O 开销）
    pub fn save_tiles_batch(&self, tiles: &[(TileCoord, Vec<u8>)]) -> Result<()> {
        if tiles.is_empty() {
            return Ok(());
        }
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        {
            let mut insert_stmt = tx.prepare_cached(
                "INSERT OR REPLACE INTO tiles (zoom_level, tile_column, tile_row, tile_data)
                 VALUES (?1, ?2, ?3, ?4)",
            )?;
            let mut update_stmt = tx.prepare_cached(
                "UPDATE download_state SET status='downloaded', error_message=NULL
                 WHERE zoom_level=?1 AND tile_column=?2 AND tile_row=?3",
            )?;
            for (coord, data) in tiles {
                insert_stmt.execute(params![coord.z as i64, coord.x as i64, coord.y as i64, data])?;
                update_stmt.execute(params![coord.z as i64, coord.x as i64, coord.y as i64])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// 批量标记瓦片为"已完成但被跳过"（裁剪后完全在范围外的瓦片）。
    /// 只更新 download_state，不向 tiles 表写入数据，避免重复下载。
    pub fn mark_skipped_batch(&self, coords: &[TileCoord]) -> Result<()> {
        if coords.is_empty() {
            return Ok(());
        }
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "UPDATE download_state SET status='downloaded', error_message=NULL
                 WHERE zoom_level=?1 AND tile_column=?2 AND tile_row=?3",
            )?;
            for coord in coords {
                stmt.execute(params![coord.z as i64, coord.x as i64, coord.y as i64])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// 批量标记瓦片下载失败（单个事务）
    pub fn mark_failed_batch(&self, failures: &[(TileCoord, String)]) -> Result<()> {
        if failures.is_empty() {
            return Ok(());
        }
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "UPDATE download_state
                 SET status='failed', retry_count=retry_count+1, error_message=?1
                 WHERE zoom_level=?2 AND tile_column=?3 AND tile_row=?4",
            )?;
            for (coord, error) in failures {
                stmt.execute(params![error, coord.z as i64, coord.x as i64, coord.y as i64])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// 标记瓦片下载失败
    pub fn mark_failed(&self, coord: &TileCoord, error: &str) -> Result<()> {
        let conn = self.lock()?;
        conn.execute(
            "UPDATE download_state
             SET status='failed', retry_count=retry_count+1, error_message=?1
             WHERE zoom_level=?2 AND tile_column=?3 AND tile_row=?4",
            params![error, coord.z as i64, coord.x as i64, coord.y as i64],
        )?;
        Ok(())
    }

    /// 将失败的瓦片重置为 pending（用于重试）
    pub fn reset_failed(&self) -> Result<i64> {
        let conn = self.lock()?;
        let count =
            conn.execute("UPDATE download_state SET status='pending' WHERE status='failed'", [])?;
        Ok(count as i64)
    }

    /// 将"下载中"状态回退为 pending（应用重启后恢复用）
    pub fn reset_stale_downloading(&self) -> Result<()> {
        let conn = self.lock()?;
        conn.execute(
            "UPDATE download_state SET status='pending' WHERE status='downloading'",
            [],
        )?;
        Ok(())
    }

    // ── metadata 读写（.tgr v2 任务元数据） ────────────────────────────────────

    /// 将 key/value 对批量写入 metadata 表
    pub fn write_meta(&self, pairs: &[(&str, &str)]) -> Result<()> {
        let conn = self.lock()?;
        for (k, v) in pairs {
            conn.execute(
                "INSERT OR REPLACE INTO metadata (name, value) VALUES (?1, ?2)",
                params![k, v],
            )?;
        }
        Ok(())
    }

    /// 读取 metadata 表中所有 key/value（返回 HashMap）
    pub fn read_meta(&self) -> Result<HashMap<String, String>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare("SELECT name, value FROM metadata")?;
        let pairs = stmt
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(pairs.into_iter().collect())
    }

    // ── 进度查询 ──────────────────────────────────────────────────────────────

    pub fn get_progress(&self) -> Result<TileProgress> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare_cached(
            "SELECT
                COUNT(*) AS total,
                SUM(CASE WHEN status='downloaded' THEN 1 ELSE 0 END) AS downloaded,
                SUM(CASE WHEN status='failed' THEN 1 ELSE 0 END) AS failed
             FROM download_state",
        )?;
        let progress = stmt.query_row([], |r| {
            let total: i64 = r.get(0)?;
            let downloaded: i64 = r.get(1)?;
            let failed: i64 = r.get(2)?;
            Ok(TileProgress {
                total,
                downloaded,
                failed,
                pending: total - downloaded - failed,
            })
        })?;
        Ok(progress)
    }
}
