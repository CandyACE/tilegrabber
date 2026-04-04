//! TileGrabber — 下载速率控制 + 反封禁
//!
//! 高级反封禁策略：
//! - User-Agent 轮换（7 种真实桌面浏览器）
//! - Accept-Language 轮换（6 种常见语言偏好）
//! - **空间局部性模拟**：模拟人类浏览地图时的视口范围，按空间聚类下载
//! - **自适应节流**：连续失败时自动降速，成功率恢复后自动提速
//! - 瓦片间随机微延迟（模拟网络抖动）
//! - 批次间人类节奏停顿（模拟地图拖拽后短暂停留）
//! - 对 429/403 响应自动放慢

use rand::Rng;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use tokio::time::{sleep, Duration};

// ─── User-Agent 池 ───────────────────────────────────────────────────────────

const USER_AGENTS: &[&str] = &[
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/123.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:125.0) Gecko/20100101 Firefox/125.0",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_4) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.4 Safari/605.1.15",
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36 Edg/124.0.0.0",
];

// ─── Accept-Language 池 ──────────────────────────────────────────────────────

const ACCEPT_LANGUAGES: &[&str] = &[
    "zh-CN,zh;q=0.9,en;q=0.8",
    "zh-CN,zh;q=0.9,en-US;q=0.8,en;q=0.7",
    "zh-TW,zh;q=0.9,en-US;q=0.8,en;q=0.7",
    "zh-CN,zh;q=0.8,zh-TW;q=0.7,zh-HK;q=0.5,en-US;q=0.3,en;q=0.2",
    "en-US,en;q=0.9,zh-CN;q=0.8,zh;q=0.7",
    "en-GB,en;q=0.9,zh-CN;q=0.8",
];

// ─── 自适应节流器 ────────────────────────────────────────────────────────────

/// 全局自适应节流状态
/// 连续失败达到阈值时自动增加延迟（模拟被检测后降速），
/// 成功率恢复后自动回到正常速度。
pub struct AdaptiveThrottle {
    /// 连续失败计数
    consecutive_failures: AtomicU32,
    /// 额外延迟因子（毫秒），由失败率动态调节
    extra_delay_ms: AtomicU64,
}

impl AdaptiveThrottle {
    pub const fn new() -> Self {
        Self {
            consecutive_failures: AtomicU32::new(0),
            extra_delay_ms: AtomicU64::new(0),
        }
    }

    /// 报告一次成功下载
    pub fn report_success(&self) {
        let prev = self.consecutive_failures.swap(0, Ordering::Relaxed);
        if prev > 0 {
            // 成功后逐步降低额外延迟（不是一次归零）
            let cur = self.extra_delay_ms.load(Ordering::Relaxed);
            let new_val = cur.saturating_sub(cur / 4 + 1); // 每次成功减少 25%
            self.extra_delay_ms.store(new_val, Ordering::Relaxed);
        }
    }

    /// 报告一次失败下载
    pub fn report_failure(&self) {
        let failures = self.consecutive_failures.fetch_add(1, Ordering::Relaxed) + 1;
        // 阶梯式增加延迟：
        // 3+ 连续失败 → +50ms
        // 6+ 连续失败 → +200ms
        // 10+ 连续失败 → +1000ms（严重限流，可能被检测）
        let delay = match failures {
            0..=2 => 0,
            3..=5 => 50,
            6..=9 => 200,
            _ => 1000,
        };
        self.extra_delay_ms.store(delay, Ordering::Relaxed);
    }

    /// 获取当前应附加的额外延迟
    pub fn extra_delay_ms(&self) -> u64 {
        self.extra_delay_ms.load(Ordering::Relaxed)
    }
}

/// 全局节流器实例
pub static ADAPTIVE: AdaptiveThrottle = AdaptiveThrottle::new();

// ─── 空间局部性排序 ──────────────────────────────────────────────────────────

use crate::tile_math::TileCoord;

/// 将一批瓦片按空间局部性重新排序，模拟人类浏览地图的模式。
///
/// 策略：先按 zoom level 分组，同一 zoom 内按 Hilbert 风格的空间填充曲线排序，
/// 使得连续下载的瓦片在空间上相邻（像人类拖动地图看邻近区域）。
/// 简化实现：使用 Z-order（Morton code）排序，计算成本低且效果接近。
pub fn sort_spatial_locality(tiles: &mut [TileCoord]) {
    tiles.sort_unstable_by(|a, b| {
        // 先按层级
        a.z.cmp(&b.z)
            // 同层级按 Z-order curve（Morton code）排序
            .then_with(|| morton_code(a.x, a.y).cmp(&morton_code(b.x, b.y)))
    });
}

/// 计算 2D Morton code（Z-order curve），将 (x, y) 交织为单个排序键
#[inline]
fn morton_code(x: u32, y: u32) -> u64 {
    fn spread(v: u32) -> u64 {
        let mut v = v as u64;
        v = (v | (v << 16)) & 0x0000_FFFF_0000_FFFF;
        v = (v | (v << 8)) & 0x00FF_00FF_00FF_00FF;
        v = (v | (v << 4)) & 0x0F0F_0F0F_0F0F_0F0F;
        v = (v | (v << 2)) & 0x3333_3333_3333_3333;
        v = (v | (v << 1)) & 0x5555_5555_5555_5555;
        v
    }
    spread(x) | (spread(y) << 1)
}

// ─── 公开函数 ────────────────────────────────────────────────────────────────

/// 瓦片间微延迟（含自适应额外延迟），在 spawn 内部调用
pub async fn random_delay(min_ms: u64, max_ms: u64) {
    let adaptive_extra = ADAPTIVE.extra_delay_ms();
    let base = if max_ms > min_ms {
        rand::thread_rng().gen_range(min_ms..=max_ms)
    } else {
        0
    };
    let total = base + adaptive_extra;
    if total > 0 {
        sleep(Duration::from_millis(total)).await;
    }
}

/// 批次间短暂停顿（10–40 ms + 自适应额外延迟）
///
/// 每完成一批瓦片后调用，给服务器极小的喘息空间。
pub async fn burst_pause() {
    let adaptive_extra = ADAPTIVE.extra_delay_ms();
    let base = rand::thread_rng().gen_range(10u64..=40u64);
    sleep(Duration::from_millis(base + adaptive_extra)).await;
}

/// 模拟人类浏览地图时的"视口加载"行为：
/// 在一组空间聚类的瓦片之间插入一个较短停顿（150–500ms），
/// 模拟用户停下来看地图然后拖拽到下一个区域。
pub async fn viewport_pause() {
    let ms = rand::thread_rng().gen_range(150u64..=500u64);
    sleep(Duration::from_millis(ms)).await;
}

/// 从池中随机选取一个 User-Agent
pub fn random_user_agent() -> &'static str {
    let idx = rand::thread_rng().gen_range(0..USER_AGENTS.len());
    USER_AGENTS[idx]
}

/// 从池中随机选取一个 Accept-Language 值
pub fn random_accept_language() -> &'static str {
    let idx = rand::thread_rng().gen_range(0..ACCEPT_LANGUAGES.len());
    ACCEPT_LANGUAGES[idx]
}
