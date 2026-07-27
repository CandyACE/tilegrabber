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

const CURRENT_TILE_STORE_VERSION: i32 = 1;

const TILE_STORE_MIGRATIONS: &[&str] = &[
    // v1: 基线表结构。fetched_at 由幂等迁移补齐。
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
];

// ─── 进度统计 ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TileProgress {
    pub total: i64,
    pub downloaded: i64,
    pub failed: i64,
    pub pending: i64,
}

// ─── TileStore ───────────────────────────────────────────────────────────────

fn convert_overflow(idx: usize, msg: impl std::fmt::Display) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        idx,
        rusqlite::types::Type::Integer,
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            msg.to_string(),
        )),
    )
}

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
        let tx = conn.unchecked_transaction()?;
        Self::run_migrations(&tx, CURRENT_TILE_STORE_VERSION, TILE_STORE_MIGRATIONS)?;
        Self::ensure_fetched_at(&tx)?;
        tx.commit()?;
        Ok(())
    }

    fn ensure_fetched_at(conn: &Connection) -> Result<()> {
        let has_fetched_at: bool = conn
            .prepare("PRAGMA table_info(tiles)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .iter()
            .any(|name| name == "fetched_at");

        if !has_fetched_at {
            conn.execute_batch(
                "ALTER TABLE tiles ADD COLUMN fetched_at INTEGER NOT NULL DEFAULT 0;",
            )?;
        }
        conn.execute_batch(
            "UPDATE tiles SET fetched_at = strftime('%s','now') WHERE fetched_at = 0;
             CREATE INDEX IF NOT EXISTS idx_tiles_fetched_at ON tiles(fetched_at);",
        )?;
        Ok(())
    }

    fn run_migrations(
        conn: &Connection,
        target_version: i32,
        migrations: &[&str],
    ) -> Result<()> {
        let current: i32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        for version in current..target_version {
            let idx = version as usize;
            if idx >= migrations.len() {
                anyhow::bail!("缺少版本 {} 的迁移脚本", version + 1);
            }
            conn.execute_batch(migrations[idx])
                .with_context(|| format!("tile store 迁移到版本 {} 失败", version + 1))?;
        }
        conn.execute(
            &format!("PRAGMA user_version = {}", target_version),
            [],
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
                    stmt.execute(params![tile.z as i64, tile.x as i64, tile.y as i64])?;
                }
            }
            tx.commit()?;
        }
        let total: i64 =
            self.lock()?
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
                let z = u8::try_from(row.get::<_, i64>(0)?)
                    .map_err(|e| convert_overflow(0, format!("zoom_level 越界: {e}")))?;
                let x = u32::try_from(row.get::<_, i64>(1)?)
                    .map_err(|e| convert_overflow(1, format!("tile_column 越界: {e}")))?;
                let y = u32::try_from(row.get::<_, i64>(2)?)
                    .map_err(|e| convert_overflow(2, format!("tile_row 越界: {e}")))?;
                Ok(TileCoord { z, x, y })
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
            "INSERT OR REPLACE INTO tiles (zoom_level, tile_column, tile_row, tile_data, fetched_at)
             VALUES (?1, ?2, ?3, ?4, strftime('%s','now'))",
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
                "INSERT OR REPLACE INTO tiles (zoom_level, tile_column, tile_row, tile_data, fetched_at)
                 VALUES (?1, ?2, ?3, ?4, strftime('%s','now'))",
            )?;
            let mut update_stmt = tx.prepare_cached(
                "UPDATE download_state SET status='downloaded', error_message=NULL
                 WHERE zoom_level=?1 AND tile_column=?2 AND tile_row=?3",
            )?;
            for (coord, data) in tiles {
                insert_stmt.execute(params![
                    coord.z as i64,
                    coord.x as i64,
                    coord.y as i64,
                    data
                ])?;
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
                stmt.execute(params![
                    error,
                    coord.z as i64,
                    coord.x as i64,
                    coord.y as i64
                ])?;
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
        let count = conn.execute(
            "UPDATE download_state SET status='pending' WHERE status='failed'",
            [],
        )?;
        Ok(count as i64)
    }

    /// E 套件：仅重置指定 zoom 的失败瓦片（用于"重试某一层"）。
    pub fn reset_failed_at_zoom(&self, zoom: i32) -> Result<i64> {
        let conn = self.lock()?;
        let count = conn.execute(
            "UPDATE download_state SET status='pending'
             WHERE status='failed' AND zoom_level=?1",
            params![zoom],
        )?;
        Ok(count as i64)
    }

    /// E 套件：按 zoom_level 统计失败瓦片数量（仅返回 count > 0 的层级）。
    pub fn failed_summary_by_zoom(&self) -> Result<Vec<(i32, i64)>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT zoom_level, COUNT(*) FROM download_state
             WHERE status='failed'
             GROUP BY zoom_level
             ORDER BY zoom_level ASC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                i32::try_from(r.get::<_, i64>(0)?)
                    .map_err(|e| convert_overflow(0, format!("zoom_level 越界: {e}")))?,
                r.get::<_, i64>(1)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// E 套件：列出指定 zoom 的所有失败瓦片坐标。
    /// 返回 (x, y) 列表，按行优先排序。
    pub fn list_failed_tiles(&self, zoom: i32) -> Result<Vec<(u32, u32)>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT tile_column, tile_row FROM download_state
             WHERE status='failed' AND zoom_level=?1
             ORDER BY tile_row ASC, tile_column ASC",
        )?;
        let rows = stmt.query_map(params![zoom], |r| {
            Ok((
                u32::try_from(r.get::<_, i64>(0)?)
                    .map_err(|e| convert_overflow(0, format!("tile_column 越界: {e}")))?,
                u32::try_from(r.get::<_, i64>(1)?)
                    .map_err(|e| convert_overflow(1, format!("tile_row 越界: {e}")))?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// 将早于 `cutoff_unix_secs` 的瓦片对应 download_state 重置为 pending（保留供未来 F3 复用）。
    /// 返回被重置的瓦片数量。
    #[allow(dead_code)]
    pub fn reset_expired(&self, cutoff_unix_secs: i64) -> Result<i64> {
        let conn = self.lock()?;
        let count = conn.execute(
            "UPDATE download_state
             SET status='pending', retry_count=0, error_message=NULL
             WHERE (zoom_level, tile_column, tile_row) IN (
                 SELECT zoom_level, tile_column, tile_row FROM tiles
                 WHERE fetched_at < ?1
             )",
            params![cutoff_unix_secs],
        )?;
        Ok(count as i64)
    }

    /// F4 借数据预检：统计源 .tiles 库中可被目标当前 pending/failed 列表复用的瓦片数。
    pub fn count_reusable_from(&self, source_path: &Path) -> Result<i64> {
        let conn = self.lock()?;
        let src_path_str = source_path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("源路径包含非法字符"))?;
        conn.execute(
            &format!(
                "ATTACH DATABASE '{}' AS src",
                src_path_str.replace('\'', "''")
            ),
            [],
        )?;
        let result: rusqlite::Result<i64> = conn.query_row(
            "SELECT COUNT(*) FROM src.tiles s
             JOIN download_state d
               ON d.zoom_level = s.zoom_level
              AND d.tile_column = s.tile_column
              AND d.tile_row = s.tile_row
             WHERE d.status IN ('pending','failed')",
            [],
            |r| r.get(0),
        );
        let _ = conn.execute("DETACH DATABASE src", []);
        Ok(result?)
    }

    /// 从另一个 .tiles 数据库导入瓦片（F4 借数据）。
    /// 仅导入目标当前 download_state 中状态为 pending / failed 的瓦片，且源端已有数据的。
    /// 导入后对应 download_state 置为 'downloaded'。
    /// 返回 (imported_count, total_pending_before)。
    pub fn import_from_external(&self, source_path: &Path) -> Result<(i64, i64)> {
        let conn = self.lock()?;

        let pending_before: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM download_state WHERE status IN ('pending','failed')",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);

        let src_path_str = source_path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("源路径包含非法字符"))?;
        // ATTACH 源库为 src
        conn.execute(&format!("ATTACH DATABASE '{}' AS src", src_path_str.replace('\'', "''")), [])?;

        let result: rusqlite::Result<i64> = (|| {
            // 1) 把源库的瓦片 INSERT OR REPLACE 进目标 tiles 表（仅那些目标当前是 pending/failed 的）
            let copied = conn.execute(
                "INSERT OR REPLACE INTO tiles (zoom_level, tile_column, tile_row, tile_data, fetched_at)
                 SELECT s.zoom_level, s.tile_column, s.tile_row, s.tile_data,
                        COALESCE(s.fetched_at, strftime('%s','now'))
                 FROM src.tiles s
                 JOIN download_state d
                   ON d.zoom_level = s.zoom_level
                  AND d.tile_column = s.tile_column
                  AND d.tile_row = s.tile_row
                 WHERE d.status IN ('pending','failed')",
                [],
            )?;
            // 2) 对应 download_state 置 downloaded
            conn.execute(
                "UPDATE download_state SET status='downloaded', error_message=NULL, retry_count=0
                 WHERE status IN ('pending','failed')
                   AND (zoom_level, tile_column, tile_row) IN (
                       SELECT zoom_level, tile_column, tile_row FROM src.tiles
                   )",
                [],
            )?;
            Ok(copied as i64)
        })();

        // 无论成功失败都尝试 DETACH
        let _ = conn.execute("DETACH DATABASE src", []);
        let imported = result?;
        Ok((imported, pending_before))
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
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
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

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn store_in_temp() -> (TileStore, PathBuf) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.tiles");
        let store = TileStore::open(&path, "test-task").unwrap();
        (store, path)
    }

    #[test]
    fn lifecycle_pending_to_downloaded() {
        let (store, _path) = store_in_temp();
        let tiles = vec![
            TileCoord { z: 1, x: 0, y: 0 },
            TileCoord { z: 1, x: 1, y: 0 },
            TileCoord { z: 1, x: 0, y: 1 },
            TileCoord { z: 1, x: 1, y: 1 },
        ];

        let total = store.init_download_state(&tiles).unwrap();
        assert_eq!(total, 4);

        let batch = store.get_pending_batch(2).unwrap();
        assert_eq!(batch.len(), 2);

        store.mark_downloading(&batch).unwrap();
        // get_pending_batch 同时包含 pending 与 downloading，因此总数仍是 4
        let still_claimable = store.get_pending_batch(10).unwrap();
        assert_eq!(still_claimable.len(), 4);

        for coord in &batch {
            store.save_tile(coord, b"data").unwrap();
        }

        let progress = store.get_progress().unwrap();
        assert_eq!(progress.total, 4);
        assert_eq!(progress.downloaded, 2);
        assert_eq!(progress.pending, 2);
    }

    #[test]
    fn opens_legacy_store_with_existing_fetched_at() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("legacy.tiles");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE tiles (
                 zoom_level INTEGER NOT NULL,
                 tile_column INTEGER NOT NULL,
                 tile_row INTEGER NOT NULL,
                 tile_data BLOB NOT NULL,
                 fetched_at INTEGER NOT NULL DEFAULT 0,
                 PRIMARY KEY (zoom_level, tile_column, tile_row)
             );",
        )
        .unwrap();
        drop(conn);

        let store = TileStore::open(&path, "legacy-task").unwrap();
        store.save_tile(&TileCoord { z: 1, x: 0, y: 0 }, b"tile").unwrap();
    }

    #[test]
    fn failed_tile_can_be_retried() {
        let (store, _path) = store_in_temp();
        let coord = TileCoord { z: 2, x: 1, y: 1 };
        store.init_download_state(&[coord]).unwrap();

        store.mark_failed(&coord, "timeout").unwrap();
        let progress = store.get_progress().unwrap();
        assert_eq!(progress.failed, 1);
        assert_eq!(progress.pending, 0);

        let reset = store.reset_failed().unwrap();
        assert_eq!(reset, 1);

        let progress = store.get_progress().unwrap();
        assert_eq!(progress.pending, 1);
        assert_eq!(progress.failed, 0);
    }

    #[test]
    fn reset_downloading_on_startup() {
        let (store, _path) = store_in_temp();
        let tiles = vec![TileCoord { z: 3, x: 2, y: 2 }];
        store.init_download_state(&tiles).unwrap();
        store.mark_downloading(&tiles).unwrap();
        // downloading 状态的瓦片仍会被 get_pending_batch 返回
        assert_eq!(store.get_pending_batch(10).unwrap().len(), 1);

        store.reset_stale_downloading().unwrap();
        let pending = store.get_pending_batch(10).unwrap();
        assert_eq!(pending.len(), 1);
    }

    #[test]
    fn mark_skipped_does_not_write_tile() {
        let (store, _path) = store_in_temp();
        let coord = TileCoord { z: 4, x: 0, y: 0 };
        store.init_download_state(&[coord]).unwrap();

        store.mark_skipped_batch(&[coord]).unwrap();
        let progress = store.get_progress().unwrap();
        assert_eq!(progress.downloaded, 1);
        assert_eq!(progress.pending, 0);
    }

    #[test]
    fn import_from_external_only_imports_pending() {
        let dir = tempdir().unwrap();
        let src_path = dir.path().join("src.tiles");
        let dst_path = dir.path().join("dst.tiles");

        let src = TileStore::open(&src_path, "src-task").unwrap();
        let shared = TileCoord { z: 5, x: 1, y: 1 };
        let only_src = TileCoord { z: 5, x: 2, y: 2 };
        src.init_download_state(&[shared, only_src]).unwrap();
        src.save_tile(&shared, b"shared").unwrap();
        src.save_tile(&only_src, b"only_src").unwrap();

        let dst = TileStore::open(&dst_path, "dst-task").unwrap();
        let dst_pending = TileCoord { z: 5, x: 1, y: 1 };
        let dst_missing = TileCoord { z: 5, x: 3, y: 3 };
        dst.init_download_state(&[dst_pending, dst_missing]).unwrap();

        let (imported, pending_before) = dst.import_from_external(&src_path).unwrap();
        assert_eq!(pending_before, 2);
        assert_eq!(imported, 1);

        let progress = dst.get_progress().unwrap();
        assert_eq!(progress.downloaded, 1);
        assert_eq!(progress.pending, 1);
    }
}
