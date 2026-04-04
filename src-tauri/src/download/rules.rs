//! TileGrabber — 下载规则引擎
//!
//! 支持：
//! - **时间窗口**：只在指定时间段内（夜间/低峰期）运行下载
//! - **速率限制**：限制每秒最大瓦片下载数，防止被服务器封禁
//!
//! 规则以 key/value 形式存储在 app.db settings 表，通过 AppDb 读取。

use chrono::Local;
use tokio::time::{sleep, Duration};

use crate::storage::app_db::AppDb;

// ─── 规则结构体 ───────────────────────────────────────────────────────────────

/// 下载规则配置
#[derive(Debug, Clone)]
pub struct DownloadRules {
    /// 是否启用时间窗口限制
    pub time_window_enabled: bool,
    /// 允许下载的开始小时（0-23）
    pub time_window_start: u8,
    /// 允许下载的结束小时（0-23，可小于 start 表示跨午夜）
    pub time_window_end: u8,
    /// 每秒最大瓦片下载数（0 = 不限制）
    pub max_tiles_per_sec: u32,
    /// 批次间额外停顿时间（毫秒，0 = 使用 throttle 默认值）
    pub burst_pause_ms: u32,
}

impl Default for DownloadRules {
    fn default() -> Self {
        DownloadRules {
            time_window_enabled: false,
            time_window_start: 22,
            time_window_end: 8,
            max_tiles_per_sec: 0,
            burst_pause_ms: 0,
        }
    }
}

impl DownloadRules {
    /// 从数据库读取规则配置（若 key 不存在则使用默认值）
    pub fn load(app_db: &AppDb) -> Self {
        let get = |key: &str, default: &str| -> String {
            app_db
                .get_setting(key)
                .ok()
                .flatten()
                .unwrap_or_else(|| default.to_string())
        };

        let time_window_enabled = get("rules.time_window_enabled", "false") == "true";
        let time_window_start: u8 = get("rules.time_window_start", "22")
            .parse()
            .unwrap_or(22);
        let time_window_end: u8 = get("rules.time_window_end", "8").parse().unwrap_or(8);
        let max_tiles_per_sec: u32 = get("rules.max_tiles_per_sec", "0").parse().unwrap_or(0);
        let burst_pause_ms: u32 = get("rules.burst_pause_ms", "0").parse().unwrap_or(0);

        DownloadRules {
            time_window_enabled,
            time_window_start,
            time_window_end,
            max_tiles_per_sec,
            burst_pause_ms,
        }
    }

    // ─── 时间窗口 ─────────────────────────────────────────────────────────────

    /// 当前是否处于允许下载的时间窗口内
    pub fn is_in_window(&self) -> bool {
        if !self.time_window_enabled {
            return true;
        }
        let hour = Local::now().hour();
        let start = self.time_window_start as u32;
        let end = self.time_window_end as u32;

        if start <= end {
            // 不跨午夜：如 08:00–22:00
            hour >= start && hour < end
        } else {
            // 跨午夜：如 22:00–08:00
            hour >= start || hour < end
        }
    }

    /// 等待直到进入时间窗口。若已在窗口内，立即返回。
    /// 每分钟检查一次，直到窗口开始。
    pub async fn wait_for_window(&self) {
        loop {
            if self.is_in_window() {
                return;
            }
            // 等待 60 秒后再检查
            sleep(Duration::from_secs(60)).await;
        }
    }

    // ─── 速率控制 ─────────────────────────────────────────────────────────────

    /// 根据 max_tiles_per_sec 计算每个瓦片间需要的最小延迟（毫秒）
    /// 返回 0 表示不限速
    pub fn per_tile_delay_ms(&self) -> u64 {
        if self.max_tiles_per_sec == 0 {
            return 0;
        }
        1000 / self.max_tiles_per_sec.max(1) as u64
    }

    /// 批次间停顿时间（毫秒）。若配置了 burst_pause_ms 则覆盖默认值。
    pub fn batch_pause_ms(&self) -> Option<u64> {
        if self.burst_pause_ms > 0 {
            Some(self.burst_pause_ms as u64)
        } else {
            None // 使用 throttle 的默认随机停顿
        }
    }
}

// ─── 辅助 trait：chrono hour ─────────────────────────────────────────────────

use chrono::Timelike;
