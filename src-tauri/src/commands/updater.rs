//! TileGrabber — 自动更新检查命令
//!
//! 功能：
//!   1. check_for_update   — 从 OSS 的 latest.json 检查版本，返回当前平台的下载直链
//!   2. download_and_install_update — 下载安装包并启动安装程序，然后退出应用
//!      下载进度通过 Tauri 事件 `update-download-progress` 推送到前端
//!
//! OSS latest.json 格式：
//! ```json
//! {
//!   "tag_name": "v1.2.0",
//!   "html_url": "https://your-bucket.oss-cn-hangzhou.aliyuncs.com/tilegrabber/releases/v1.2.0/",
//!   "body": "## 更新内容\n- 修复了某某问题",
//!   "assets": {
//!     "windows": "https://...TileGrabber_1.2.0_x64-setup.exe",
//!     "macos":   "https://...TileGrabber_1.2.0_x64.dmg",
//!     "linux":   "https://...TileGrabber_1.2.0_amd64.AppImage"
//!   }
//! }
//! ```

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tauri::Emitter;
use tokio::io::AsyncWriteExt;

/// latest.json 地址。
/// 优先使用编译时环境变量 TILEGRABBER_UPDATE_URL（CI 流水线注入），
/// 否则回退到 GitHub Releases 上的固定 latest.json 地址。
/// 格式：https://github.com/<owner>/<repo>/releases/latest/download/latest.json
const UPDATE_CHECK_URL: &str = match option_env!("TILEGRABBER_UPDATE_URL") {
    Some(url) => url,
    None => "http://oss.emapgis.com/soft/tiledownload/latest.json",
};

// ─── 公共类型 ─────────────────────────────────────────────────────────────────

/// 更新检查结果（返回给前端）
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheckResult {
    pub current_version: String,
    pub latest_version: Option<String>,
    pub has_update: bool,
    /// 更新说明页 URL（可选，浏览器打开）
    pub release_url: Option<String>,
    /// 当前平台的安装包 OSS 直链
    pub download_url: Option<String>,
    pub release_notes: Option<String>,
    pub error: Option<String>,
}

/// 下载进度事件 payload
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub downloaded: u64,
    pub total: u64,
    /// 0-100
    pub percent: u8,
}

// ─── 私有：OSS JSON 反序列化 ──────────────────────────────────────────────────

#[derive(Debug, Deserialize, Default)]
#[allow(dead_code)]
struct OssAssets {
    windows: Option<String>,
    /// macOS ARM64（Apple Silicon），也是 macos 字段的主下载地址
    macos: Option<String>,
    /// macOS x86_64（Intel），CI 额外写入，客户端按架构自动选择
    macos_x64: Option<String>,
    linux: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OssRelease {
    tag_name: String,
    html_url: Option<String>,
    body: Option<String>,
    #[serde(default)]
    assets: OssAssets,
}

// ─── Tauri 命令 ──────────────────────────────────────────────────────────────

/// 检查是否有新版本，返回当前平台的下载直链
#[tauri::command]
pub async fn check_for_update() -> Result<UpdateCheckResult, String> {
    let current_version = env!("CARGO_PKG_VERSION").to_string();


    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent(concat!("TileGrabber/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())?;

    let resp = match client.get(UPDATE_CHECK_URL).send().await {
        Ok(r) => r,
        Err(e) => {
            return Ok(UpdateCheckResult {
                current_version,
                latest_version: None,
                has_update: false,
                release_url: None,
                download_url: None,
                release_notes: None,
                error: Some(format!("网络请求失败: {}", e)),
            });
        }
    };

    if !resp.status().is_success() {
        return Ok(UpdateCheckResult {
            current_version,
            latest_version: None,
            has_update: false,
            release_url: None,
            download_url: None,
            release_notes: None,
            error: Some(format!("服务器返回 {}", resp.status())),
        });
    }

    let body_text = match resp.text().await {
        Ok(t) => t,
        Err(e) => {
            return Ok(UpdateCheckResult {
                current_version,
                latest_version: None,
                has_update: false,
                release_url: None,
                download_url: None,
                release_notes: None,
                error: Some(format!("读取响应失败: {}", e)),
            });
        }
    };

    let release: OssRelease = match serde_json::from_str(&body_text) {
        Ok(r) => r,
        Err(e) => {
            return Ok(UpdateCheckResult {
                current_version,
                latest_version: None,
                has_update: false,
                release_url: None,
                download_url: None,
                release_notes: None,
                error: Some(format!("解析响应失败: {}", e)),
            });
        }
    };

    let latest_version = release.tag_name.trim_start_matches('v').to_string();
    let has_update = is_newer(&latest_version, &current_version);

    // 根据当前编译平台和架构选择下载链接
    let download_url = {
        #[cfg(target_os = "windows")]
        { release.assets.windows.clone() }
        #[cfg(target_os = "macos")]
        {
            // M 系列（aarch64）优先用 macos 字段；Intel 优先用 macos_x64，回退 macos
            #[cfg(target_arch = "aarch64")]
            { release.assets.macos.clone() }
            #[cfg(not(target_arch = "aarch64"))]
            { release.assets.macos_x64.clone().or_else(|| release.assets.macos.clone()) }
        }
        #[cfg(target_os = "linux")]
        { release.assets.linux.clone() }
        #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
        { None::<String> }
    };

    let release_notes = release.body.as_deref().map(|s| {
        if s.len() > 500 { format!("{}…", &s[..500]) } else { s.to_string() }
    });

    Ok(UpdateCheckResult {
        current_version,
        latest_version: Some(latest_version),
        has_update,
        release_url: release.html_url,
        download_url,
        release_notes,
        error: None,
    })
}

/// 下载安装包并启动，退出应用（由安装程序完成替换和重启）
///
/// 下载过程中持续向前端推送 `update-download-progress` 事件。
/// 安全限制：URL 必须 https 开头。
#[tauri::command]
pub async fn download_and_install_update(
    app: tauri::AppHandle,
    url: String,
) -> Result<(), String> {
    // 安全校验：只允许 https
    if !url.starts_with("https://") {
        return Err("仅支持 HTTPS 下载地址".to_string());
    }

    // 从 URL 推断文件名
    let filename = url
        .split('/')
        .last()
        .filter(|s| !s.is_empty())
        .unwrap_or("TileGrabber_update_installer");

    let dest = std::env::temp_dir().join(filename);

    // ── 发起流式下载 ──────────────────────────────────────────────────────────
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .user_agent(concat!("TileGrabber/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())?;

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("下载失败: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("下载失败，服务器返回 {}", resp.status()));
    }

    let total = resp.content_length().unwrap_or(0);
    let mut file = tokio::fs::File::create(&dest)
        .await
        .map_err(|e| format!("无法创建临时文件: {}", e))?;

    let mut stream = resp.bytes_stream();
    let mut downloaded: u64 = 0;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("下载中断: {}", e))?;
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("写入失败: {}", e))?;
        downloaded += chunk.len() as u64;

        let percent = if total > 0 {
            ((downloaded * 100) / total).min(100) as u8
        } else {
            0
        };
        let _ = app.emit(
            "update-download-progress",
            DownloadProgress { downloaded, total, percent },
        );
    }

    file.flush().await.map_err(|e| e.to_string())?;
    drop(file);

    // ── 启动安装程序 ──────────────────────────────────────────────────────────
    #[cfg(target_os = "windows")]
    {
        // NSIS 安装包：直接运行，安装程序会提示用户关闭旧版本
        std::process::Command::new(&dest)
            .spawn()
            .map_err(|e| format!("启动安装程序失败: {}", e))?;
    }

    #[cfg(target_os = "macos")]
    {
        // 打开 .dmg 让用户拖拽安装
        std::process::Command::new("open")
            .arg(&dest)
            .spawn()
            .map_err(|e| format!("打开 DMG 失败: {}", e))?;
    }

    #[cfg(target_os = "linux")]
    {
        // 给 AppImage 加执行权限，然后运行
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("设置权限失败: {}", e))?;
        std::process::Command::new(&dest)
            .spawn()
            .map_err(|e| format!("启动 AppImage 失败: {}", e))?;
    }

    // 延迟 800ms 让安装程序窗口出现再退出
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    std::process::exit(0);
}

/// 用浏览器打开发布页（备用：无下载直链时使用）
#[tauri::command]
pub async fn open_release_url(url: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/c", "start", "", &url])
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&url)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&url)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ─── 版本比较 ─────────────────────────────────────────────────────────────────

fn is_newer(latest: &str, current: &str) -> bool {
    let parse = |v: &str| -> (u64, u64, u64) {
        let parts: Vec<u64> = v.split('.').map(|p| p.parse().unwrap_or(0)).collect();
        (
            parts.first().copied().unwrap_or(0),
            parts.get(1).copied().unwrap_or(0),
            parts.get(2).copied().unwrap_or(0),
        )
    };
    parse(latest) > parse(current)
}
