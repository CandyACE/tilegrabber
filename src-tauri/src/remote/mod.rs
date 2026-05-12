//! 远程协作共享类型
//!
//! 服务端开放 `/remote/` API 后，其他御图实例可通过 Bearer Token 连接，
//! 提交下载任务并实时订阅 SSE 进度流。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::Emitter;
use tokio::sync::RwLock;

// ─── 配置缓存 ─────────────────────────────────────────────────────────────────

/// 远程服务配置快照（从 settings 表读取）
#[derive(Debug, Clone, Default)]
pub struct RemoteConfig {
    pub enabled: bool,
    pub token: String,
}

/// 线程安全远程配置缓存：`None` = 脏，下次请求时从 DB 重新加载
pub type RemoteConfigCache = Arc<RwLock<Option<RemoteConfig>>>;

// ─── 已连接客户端 ─────────────────────────────────────────────────────────────

/// 已连接 SSE 客户端的快照信息
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientInfo {
    pub id: String,
    pub ip: String,
    pub connected_at: i64,
}

/// 已连接 SSE 客户端注册表（Clone 廉价，内部持有 Arc）
#[derive(Clone)]
pub struct RemoteClients {
    pub inner: Arc<Mutex<HashMap<String, ClientInfo>>>,
}

impl RemoteClients {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn insert(&self, info: ClientInfo) {
        if let Ok(mut map) = self.inner.lock() {
            map.insert(info.id.clone(), info);
        }
    }

    pub fn remove(&self, id: &str) {
        if let Ok(mut map) = self.inner.lock() {
            map.remove(id);
        }
    }

    pub fn list(&self) -> Vec<ClientInfo> {
        self.inner
            .lock()
            .map(|m| m.values().cloned().collect())
            .unwrap_or_default()
    }

    pub fn count(&self) -> usize {
        self.inner.lock().map(|m| m.len()).unwrap_or(0)
    }
}

// ─── RAII 客户端守卫 ──────────────────────────────────────────────────────────

/// SSE 连接生命周期守卫：构造时注册客户端，Drop 时自动注销
pub struct ClientGuard {
    pub id: String,
    pub clients: RemoteClients,
    pub app: tauri::AppHandle,
}

impl Drop for ClientGuard {
    fn drop(&mut self) {
        self.clients.remove(&self.id);
        let count = self.clients.count();
        let _ = self.app.emit("remote:clients-changed", count);
    }
}
