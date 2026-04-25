// TileGrabber — Tauri v2 backend entry point

pub mod commands;
pub mod download;
pub mod export;
pub mod parser;
pub mod server;
pub mod storage;
pub mod tile_math;
pub mod types;

use commands::layer::{create_layer, delete_layer, list_layers, rename_layer, reorder_layers};
use commands::math::{calculate_tile_count, generate_tile_grid};
use commands::server::{get_server_status, get_service_stats, start_tile_server, stop_tile_server};
use commands::settings::{get_all_settings, get_setting, set_all_settings, set_setting};
use commands::source::{
    fetch_mbtiles_tile, parse_area_file, parse_mbtiles_source, parse_source_file, parse_tms_url,
    parse_wmts_url, validate_tile_url,
};
use commands::task::{
    cancel_download, check_disk_space, create_task, delete_task, estimate_download,
    export_directory, export_geotiff,
    export_mbtiles, export_task, get_download_progress_geojson, get_export_jobs, get_stored_tile,
    get_task, get_task_logs, get_task_thumbnail, import_mbtiles, import_task, list_tasks,
    pause_download, resume_download, retry_failed, reveal_in_explorer, start_download, ExportState,
};
use commands::tile_proxy::fetch_tile;
use commands::updater::{check_for_update, download_and_install_update, open_release_url};
use commands::web_capture::{
    clear_captured_tiles, close_capture_window, get_captured_tiles, open_capture_window,
    CaptureSession,
};
use download::engine::DownloadEngine;
use server::StatsMap;
use server::{TileServer, TileServerState};
use storage::app_db::AppDb;
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::Emitter;
use tauri::Manager;

// ─── 网络监控状态 ────────────────────────────────────────────────────────────

/// 记录因网络中断而被自动暂停的任务 ID（恢复时仅恢复这些任务）
type NetworkPausedSet = std::sync::Arc<std::sync::Mutex<std::collections::HashSet<String>>>;

/// 网络状态变更事件 payload
#[derive(Clone, serde::Serialize)]
struct NetworkStatusPayload {
    online: bool,
}

/// 使用 reqwest 探测网络可达性（走当前配置的代理）
/// 探测网络连通性：请求百度，能收到响应即认为在线。
async fn probe_network(proxy_url: Option<String>) -> bool {
    let mut builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(6))
        .connect_timeout(std::time::Duration::from_secs(5))
        .user_agent("Mozilla/5.0 TileGrabber-NetworkCheck/1.0");
    if let Some(url) = proxy_url.filter(|u| !u.is_empty()) {
        if let Ok(proxy) = reqwest::Proxy::all(&url) {
            builder = builder.proxy(proxy);
        }
    }
    let client = match builder.build() {
        Ok(c) => c,
        Err(_) => return false,
    };
    client
        .head("https://www.baidu.com/favicon.ico")
        .send()
        .await
        .is_ok()
}

/// 退出整个应用程序
#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

/// 显示主窗口并恢复焦点（供浮窗/托盘调用）
#[tauri::command]
fn show_main_window(app: tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.set_focus();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_single_instance::Builder::new()
                .callback(|app, _argv, _cwd| {
                    // 第二个实例启动时，聚焦已有主窗口
                    if let Some(win) = app.get_webview_window("main") {
                        let _ = win.show();
                        let _ = win.set_focus();
                    }
                })
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            // 初始化应用数据目录
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;

            // 打开主数据库并注册为 Tauri 托管状态
            let app_db = AppDb::open(&data_dir).map_err(|e| {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                ))
            })?;
            // 应用重启后，将遗留的 downloading 任务重置为 paused，
            // 防止任务卡在"下载中"但引擎无句柄而无法操作的问题
            // 先获取"崩溃中断"的任务 ID，用于后续自动续传
            let interrupted_ids = app_db
                .get_downloading_task_ids()
                .unwrap_or_default();
            let _ = app_db.reset_downloading_to_paused();
            app.manage(app_db);

            // 初始化下载引擎
            app.manage(DownloadEngine::new());

            // 初始化瓦片发布服务状态
            let tile_server: TileServerState =
                std::sync::Arc::new(std::sync::Mutex::new(TileServer::new()));
            app.manage(tile_server);

            // 初始化服务请求统计（在 axum 服务器和 Tauri 命令之间共享）
            let stats_map: StatsMap =
                std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
            app.manage(stats_map);

            // 初始化网页抓取会话状态
            app.manage(std::sync::Arc::new(CaptureSession::new()));

            // 初始化导出任务状态
            let export_state: ExportState =
                std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
            app.manage(export_state);

            // 初始化网络暂停任务集合
            let net_paused: NetworkPausedSet = std::sync::Arc::new(std::sync::Mutex::new(
                std::collections::HashSet::new(),
            ));
            app.manage(net_paused.clone());

            // 启动网络监控后台任务：每 15 秒探测一次，断网时自动暂停正在下载的任务，
            // 网络恢复后自动恢复它们。
            {
                let app_handle = app.handle().clone();
                let engine_ref = app.state::<DownloadEngine>().inner().clone();
                let db_ref = app.state::<AppDb>().inner().clone();
                let paused_ref = net_paused.clone();
                tauri::async_runtime::spawn(async move {
                    // None = 尚未确认状态；Some(true) = 在线；Some(false) = 离线
                    let mut was_online: Option<bool> = None;
                    // 连续失败计数：需要连续 2 次失败才声明离线，防止瞬时抖动
                    let mut consecutive_failures: u32 = 0;
                    const FAILURES_BEFORE_OFFLINE: u32 = 2;
                    // 首次延迟 20 秒，避免干扰启动阶段的正常网络请求
                    tokio::time::sleep(std::time::Duration::from_secs(20)).await;
                    loop {
                        // 读取当前代理配置（运行时可能变更）
                        let proxy_url =
                            commands::settings::get_active_proxy_url(&db_ref);
                        let online = probe_network(proxy_url).await;

                        if online {
                            consecutive_failures = 0;
                        } else {
                            consecutive_failures += 1;
                        }

                        // 实际使用的在线状态：失败次数未达阈值时仍视为在线
                        let effective_online =
                            online || consecutive_failures < FAILURES_BEFORE_OFFLINE;

                        match was_online {
                            Some(prev) if prev && !effective_online => {
                                // 网络刚断开：暂停所有正在下载的任务并记录 ID
                                let running = engine_ref.running_task_ids();
                                for id in &running {
                                    engine_ref.pause(id).ok();
                                }
                                if let Ok(mut set) = paused_ref.lock() {
                                    set.extend(running);
                                }
                                let _ = app_handle.emit(
                                    "tilegrab-network-status",
                                    NetworkStatusPayload { online: false },
                                );
                            }
                            Some(prev) if !prev && effective_online => {
                                // 网络刚恢复：只恢复当初被网络中断自动暂停的任务
                                let to_resume: Vec<String> = paused_ref
                                    .lock()
                                    .map(|mut set| set.drain().collect())
                                    .unwrap_or_default();
                                for id in to_resume {
                                    if engine_ref.is_active(&id) {
                                        engine_ref.resume(&id).ok();
                                    } else {
                                        let c = db_ref
                                            .get_setting("download.concurrency")
                                            .ok()
                                            .flatten()
                                            .and_then(|s| s.parse::<usize>().ok())
                                            .filter(|&n| n > 0)
                                            .unwrap_or(16);
                                        engine_ref
                                            .start(id, db_ref.clone(), c, app_handle.clone())
                                            .ok();
                                    }
                                }
                                let _ = app_handle.emit(
                                    "tilegrab-network-status",
                                    NetworkStatusPayload { online: true },
                                );
                            }
                            _ => {}
                        }

                        was_online = Some(effective_online);
                        tokio::time::sleep(std::time::Duration::from_secs(15)).await;
                    }
                });
            }

            // 创建系统托盘图标
            let icon = app
                .default_window_icon()
                .cloned()
                .expect("no window icon configured");
            TrayIconBuilder::new()
                .icon(icon)
                .tooltip("御图 — 点击显示主界面")
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(win) = app.get_webview_window("main") {
                            let _ = win.show();
                            let _ = win.set_focus();
                        }
                    }
                })
                .build(app)?;

            // 崩溃自动续传：延迟 5 秒后自动恢复被中断的任务（如设置允许）
            if !interrupted_ids.is_empty() {
                let auto_resume_enabled = app
                    .state::<AppDb>()
                    .get_setting("app.auto_resume_on_startup")
                    .ok()
                    .flatten()
                    .map(|v| v != "false")
                    .unwrap_or(true);
                if auto_resume_enabled {
                    let engine_ref = app.state::<DownloadEngine>().inner().clone();
                    let db_ref = app.state::<AppDb>().inner().clone();
                    let app_handle = app.handle().clone();
                    tauri::async_runtime::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        let c = db_ref
                            .get_setting("download.concurrency")
                            .ok()
                            .flatten()
                            .and_then(|s| s.parse::<usize>().ok())
                            .filter(|&n| n > 0)
                            .unwrap_or(16);
                        for id in interrupted_ids {
                            if let Ok(task) = db_ref.get_task(&id) {
                                if task.status == "paused" {
                                    engine_ref.start(id, db_ref.clone(), c, app_handle.clone()).ok();
                                }
                            }
                        }
                    });
                }
            }

            // 启动后静默检查更新（延迟 12 秒，避免干扰应用启动）
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(12)).await;
                if let Ok(result) = check_for_update().await {
                    if result.has_update {
                        let _ = app_handle.emit("update-available", &result);
                    }
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // 数据源解析
            parse_source_file,
            parse_area_file,
            parse_tms_url,
            parse_wmts_url,
            validate_tile_url,
            parse_mbtiles_source,
            fetch_mbtiles_tile,
            // 网页抓取
            open_capture_window,
            get_captured_tiles,
            clear_captured_tiles,
            close_capture_window,
            // 瓦片数学计算
            calculate_tile_count,
            generate_tile_grid,
            // 任务管理
            create_task,
            list_tasks,
            get_task,
            delete_task,
            // 下载控制
            start_download,
            pause_download,
            resume_download,
            cancel_download,
            retry_failed,
            get_task_logs,
            check_disk_space,
            estimate_download,
            // 本地瓦片读取（地图预览）
            get_stored_tile,
            get_task_thumbnail,
            // 导出
            export_mbtiles,
            export_directory,
            export_geotiff,
            get_export_jobs,
            reveal_in_explorer,
            // 任务包导入/导出
            export_task,
            import_task,
            import_mbtiles,
            // 下载进度可视化
            get_download_progress_geojson,
            // 瓦片代理
            fetch_tile,
            // 瓦片发布服务
            start_tile_server,
            stop_tile_server,
            get_server_status,
            get_service_stats,
            // 设置
            get_setting,
            set_setting,
            get_all_settings,
            set_all_settings,
            // 自动更新
            check_for_update,
            open_release_url,
            download_and_install_update,
            // 图层管理
            create_layer,
            list_layers,
            delete_layer,
            reorder_layers,
            rename_layer,
            // 应用控制
            quit_app,
            show_main_window,
        ])
        .run(tauri::generate_context!())
        .expect("error while running TileGrabber")
}
