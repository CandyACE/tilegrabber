//! 全局设置 Tauri 命令
//!
//! 设置以 key/value 形式存储在 app.db 的 settings 表。
//! 前端通过 `get_setting` / `set_setting` / `get_all_settings` 读写。

use std::collections::HashMap;

use tauri::State;

use crate::storage::app_db::AppDb;

// ─── 默认值 ──────────────────────────────────────────────────────────────────

/// 应用的全部可配置设置及其默认值
pub fn default_settings() -> HashMap<&'static str, &'static str> {
    [
        // 下载并发（默认 32，充分利用多核）
        ("download.concurrency", "32"),
        // 单个瓦片请求超时（秒）
        ("download.timeout_secs", "15"),
        // 首次重试前延迟（毫秒，指数退避基数）
        ("download.retry_delay_ms", "500"),
        // 最大重试次数
        ("download.max_retries", "3"),
        // 相邻两次瓦片请求的最小/最大随机延迟（毫秒）：设 0/150 可兼顾速度与反封禁
        ("download.delay_min_ms", "0"),
        ("download.delay_max_ms", "150"),
        // 瓦片存储目录（空字符串 = 使用安装目录下的 tiles 子目录）
        ("app.tiles_dir", ""),
        // 瓦片发布服务默认端口
        ("server.default_port", "8765"),
        // ── 下载规则 ────────────────────────────────────────────────────────
        // 是否启用时间窗口
        ("rules.time_window_enabled", "false"),
        // 允许下载的开始/结束小时（0-23）
        ("rules.time_window_start", "22"),
        ("rules.time_window_end", "8"),
        // 每秒最大瓦片数（0 = 不限制）
        ("rules.max_tiles_per_sec", "0"),
        // 批次间额外停顿（ms，0 = 使用内置随机停顿）
        ("rules.burst_pause_ms", "0"),
        // ── 应用行为 ────────────────────────────────────────────────────────
        // 免责声明是否已同意
        ("app.disclaimer_agreed", "false"),
        // 关闭窗口行为：""或"ask" = 每次询问，"quit" = 直接退出，"tray" = 最小化到托盘
        ("app.close_action", "ask"),
        // 是否开启悬浮速度窗口
        ("app.float_window", "false"),
    ]
    .into()
}

// ─── Tauri 命令 ──────────────────────────────────────────────────────────────

/// 读取单个设置（若不存在返回默认值，若默认值也无返回 null）
#[tauri::command]
pub fn get_setting(key: String, app_db: State<'_, AppDb>) -> Result<Option<String>, String> {
    let val = app_db
        .get_setting(&key)
        .map_err(|e| e.to_string())?;
    if val.is_none() {
        // 返回默认值
        let defaults = default_settings();
        return Ok(defaults.get(key.as_str()).map(|s| s.to_string()));
    }
    Ok(val)
}

/// 写入单个设置
#[tauri::command]
pub fn set_setting(key: String, value: String, app_db: State<'_, AppDb>) -> Result<(), String> {
    app_db.set_setting(&key, &value).map_err(|e| e.to_string())
}

/// 读取全部设置（合并默认值，数据库中的值优先）
#[tauri::command]
pub fn get_all_settings(app_db: State<'_, AppDb>) -> Result<HashMap<String, String>, String> {
    // 先填充默认值
    let mut map: HashMap<String, String> = default_settings()
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

    // 用数据库全部行覆盖（SELECT *，不依赖 default_settings 键列表）
    // 好处：即使某个 key 尚未加入 default_settings，已存入 DB 的值也能正确读回
    let db_rows = app_db.get_all_settings_raw().map_err(|e| e.to_string())?;
    for (k, v) in db_rows {
        map.insert(k, v);
    }
    Ok(map)
}

/// 批量写入设置
#[tauri::command]
pub fn set_all_settings(
    settings: HashMap<String, String>,
    app_db: State<'_, AppDb>,
) -> Result<(), String> {
    for (k, v) in &settings {
        app_db.set_setting(k, v).map_err(|e| e.to_string())?;
    }
    Ok(())
}
