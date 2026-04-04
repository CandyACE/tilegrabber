// TileGrabber — Tauri v2 backend entry point

pub mod commands;
pub mod download;
pub mod export;
pub mod parser;
pub mod server;
pub mod storage;
pub mod tile_math;
pub mod types;

use tauri::Manager;
use tauri::tray::{TrayIconBuilder, MouseButton, MouseButtonState, TrayIconEvent};
use commands::math::{calculate_tile_count, generate_tile_grid};
use commands::source::{parse_area_file, parse_source_file, parse_tms_url, parse_wmts_url, validate_tile_url};
use commands::web_capture::{
    clear_captured_tiles, close_capture_window, get_captured_tiles, open_capture_window,
    CaptureSession,
};
use commands::task::{
    cancel_download, create_task, delete_task, export_directory, export_geotiff, export_mbtiles,
    export_task, get_download_progress_geojson, get_export_jobs, get_stored_tile, get_task,
    get_task_logs, get_task_thumbnail, import_task, list_tasks, pause_download, resume_download,
    retry_failed, reveal_in_explorer, start_download, ExportState,
};
use commands::server::{get_server_status, start_tile_server, stop_tile_server};
use commands::settings::{get_all_settings, get_setting, set_all_settings, set_setting};
use commands::tile_proxy::fetch_tile;
use commands::updater::{check_for_update, download_and_install_update, open_release_url};
use commands::layer::{create_layer, list_layers, delete_layer, reorder_layers, rename_layer};
use download::engine::DownloadEngine;
use server::{TileServer, TileServerState};
use storage::app_db::AppDb;

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
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            // 初始化应用数据目录
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;

            // 打开主数据库并注册为 Tauri 托管状态
            let app_db = AppDb::open(&data_dir)
                .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;
            // 应用重启后，将遗留的 downloading 任务重置为 paused，
            // 防止任务卡在"下载中"但引擎无句柄而无法操作的问题
            let _ = app_db.reset_downloading_to_paused();
            app.manage(app_db);

            // 初始化下载引擎
            app.manage(DownloadEngine::new());

            // 初始化瓦片发布服务状态
            let tile_server: TileServerState = std::sync::Arc::new(std::sync::Mutex::new(TileServer::new()));
            app.manage(tile_server);

            // 初始化网页抓取会话状态
            app.manage(std::sync::Arc::new(CaptureSession::new()));

            // 初始化导出任务状态
            let export_state: ExportState = std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
            app.manage(export_state);

            // 创建系统托盘图标
            let icon = app.default_window_icon()
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
                    } = event {
                        let app = tray.app_handle();
                        if let Some(win) = app.get_webview_window("main") {
                            let _ = win.show();
                            let _ = win.set_focus();
                        }
                    }
                })
                .build(app)?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // 数据源解析
            parse_source_file,
            parse_area_file,
            parse_tms_url,
            parse_wmts_url,
            validate_tile_url,
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
            // 下载进度可视化
            get_download_progress_geojson,
            // 瓦片代理
            fetch_tile,
            // 瓦片发布服务
            start_tile_server,
            stop_tile_server,
            get_server_status,
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
