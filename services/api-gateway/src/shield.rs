//! 防爬虫与应用层 DDoS 护栏（TODO L486）
//!
//! 与既有限流（`RedisRateLimiter`，per-身份 per-分钟配额）互补的三件套，
//! 全部纯逻辑、时间显式入参（同输入序必同输出序），middleware 只做装配：
//!
//! 1. **UA 分类**（`classify_user_agent`）：空 UA / 已知脚本与爬虫特征
//!    识别。拒绝动作可关（`bot_deny`，默认关——curl/脚本类既有调用方
//!    与 e2e 不破坏），开启后命中 403；
//! 2. **路径扫描检测**（`ScanDetector`）：同身份滑动窗口内离散路径数
//!    超阈 → 爬虫广度扫描特征（正常用户会话不命中宽阈值），默认开；
//! 3. **秒级 burst 令牌桶**（`BurstGuard`）：补齐 per-分钟限流的秒级
//!    空隙（短洪峰在分钟配额内打满上游），默认开、宽阈值。
//!
//! 生效面与限流一致（只在 /api 子路由），健康检查 /metrics /ws 与
//! /auth/token 天然不受影响。内存界：detector/bucket 按身份分桶，
//! 超 1 万桶时清理闲置项（`TRACK_EVICT`，防身份伪造撑爆内存）。

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

/// 护栏运行态（GatewayState 成员；检测器/令桶 Arc 共享——网关状态各处
/// Clone 看到同一份推进状态）
#[derive(Debug, Clone)]
pub struct ShieldState {
    pub config: ShieldConfig,
    pub scan: Arc<Mutex<ScanDetector>>,
    pub burst: Arc<Mutex<BurstGuard>>,
}

impl ShieldState {
    pub fn from_config(config: ShieldConfig) -> Self {
        Self {
            scan: Arc::new(Mutex::new(ScanDetector::new(
                config.scan_window_ms,
                config.scan_max_distinct,
            ))),
            burst: Arc::new(Mutex::new(BurstGuard::new(
                config.burst_capacity,
                config.burst_refill_per_sec,
            ))),
            config,
        }
    }

    pub fn from_env() -> Self {
        Self::from_config(ShieldConfig::from_env())
    }
}

/// UA 三类：常规放行 / 空值 / 已知脚本与爬虫特征
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UaClass {
    Normal,
    Empty,
    KnownBot,
}

/// 已知脚本/爬虫/扫描器 UA 特征（小写子串匹配；`curl/` 带斜杠避免
/// 误伤含 curl 字样的常规 UA 词根）。`bot`/`spider`/`crawler` 同时
/// 覆盖搜索引擎与通用爬虫——本层不做白名单差异化（白名单是后续项）。
const BOT_MARKS: &[&str] = &[
    "scrapy",
    "wget",
    "curl/",
    "python-requests",
    "python-urllib",
    "go-http-client",
    "java/",
    "apache-httpclient",
    "httpclient",
    "libwww-perl",
    "okhttp",
    "nikto",
    "sqlmap",
    "masscan",
    "zgrab",
    "headlesschrome",
    "phantomjs",
    "bot",
    "spider",
    "crawler",
];

pub fn classify_user_agent(ua: Option<&str>) -> UaClass {
    match ua.map(str::trim) {
        None | Some("") => UaClass::Empty,
        Some(ua) => {
            let lower = ua.to_lowercase();
            if BOT_MARKS.iter().any(|mark| lower.contains(mark)) {
                UaClass::KnownBot
            } else {
                UaClass::Normal
            }
        }
    }
}

/// 空闲身份分桶的清理阈值（桶数超限时逐出闲置超 60s 的项）
const TRACK_EVICT: usize = 10_000;
const TRACK_IDLE_MS: i64 = 60_000;

/// 路径扫描检测：同身份滑动窗口内的**离散**路径计数（重复访问同一路径
/// 是热点行为不是扫描）。超阈返回 true（调用方决定拒绝动作）。
#[derive(Debug)]
pub struct ScanDetector {
    window_ms: i64,
    max_distinct: usize,
    tracks: HashMap<String, Vec<(i64, String)>>,
}

impl ScanDetector {
    pub fn new(window_ms: i64, max_distinct: usize) -> Self {
        Self {
            window_ms: window_ms.max(1),
            max_distinct: max_distinct.max(1),
            tracks: HashMap::new(),
        }
    }

    /// 记录一次访问并判定：true = 窗口内离散路径数超阈（疑似扫描）。
    /// 窗口过期项惰性清理（O(n) 线性扫，窗口长度有限）。
    pub fn observe(&mut self, identity: &str, path: &str, now_ms: i64) -> bool {
        let (window_ms, max_distinct) = (self.window_ms, self.max_distinct);
        let distinct_len = {
            let entry = self.tracks.entry(identity.to_string()).or_default();
            entry.retain(|(ts, _)| now_ms - *ts < window_ms);
            entry.push((now_ms, path.to_string()));
            let distinct: HashSet<&str> = entry.iter().map(|(_, p)| p.as_str()).collect();
            distinct.len()
        };
        if self.tracks.len() > TRACK_EVICT {
            self.evict_idle(now_ms);
        }
        distinct_len > max_distinct
    }

    fn evict_idle(&mut self, now_ms: i64) {
        self.tracks.retain(|_, entry| {
            entry
                .last()
                .map(|(ts, _)| now_ms - *ts < TRACK_IDLE_MS)
                .unwrap_or(false)
        });
    }
}

/// 秒级 burst 令牌桶（per-身份）：容量 `capacity`，匀速补充
/// `refill_per_sec`。短洪峰由桶深吸收，持续超速被拒。
#[derive(Debug)]
pub struct BurstGuard {
    capacity: f64,
    refill_per_sec: f64,
    buckets: HashMap<String, (f64, i64)>,
}

impl BurstGuard {
    pub fn new(capacity: u32, refill_per_sec: f64) -> Self {
        Self {
            capacity: (capacity as f64).max(1.0),
            refill_per_sec: refill_per_sec.max(0.001),
            buckets: HashMap::new(),
        }
    }

    /// 尝试取一枚令牌：true = 放行。惰性补充（按距上次的时长）。
    pub fn try_acquire(&mut self, identity: &str, now_ms: i64) -> bool {
        if self.buckets.len() > TRACK_EVICT {
            self.buckets
                .retain(|_, (_, last)| now_ms - *last < TRACK_IDLE_MS);
        }
        let (tokens, last) = self
            .buckets
            .entry(identity.to_string())
            .or_insert((self.capacity, now_ms));
        let elapsed_ms = (now_ms - *last).max(0) as f64;
        *tokens = (*tokens + elapsed_ms / 1000.0 * self.refill_per_sec).min(self.capacity);
        *last = now_ms;
        if *tokens >= 1.0 {
            *tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

/// 护栏配置（env 注入，非法值回退默认——配置面永不拒绝启动）
#[derive(Debug, Clone)]
pub struct ShieldConfig {
    /// UA 拒绝开关（ALPHA_GATEWAY_BOT_DENY=1 开启；默认关）
    pub bot_deny: bool,
    /// 扫描检测开关（ALPHA_GATEWAY_SCAN_DETECT=0 关闭；默认开）
    pub scan_enabled: bool,
    pub scan_window_ms: i64,
    pub scan_max_distinct: usize,
    /// burst 护栏开关（ALPHA_GATEWAY_BURST_GUARD=0 关闭；默认开）
    pub burst_enabled: bool,
    pub burst_capacity: u32,
    pub burst_refill_per_sec: f64,
}

impl Default for ShieldConfig {
    fn default() -> Self {
        Self {
            bot_deny: false,
            scan_enabled: true,
            scan_window_ms: 60_000,
            scan_max_distinct: 240,
            burst_enabled: true,
            burst_capacity: 100,
            burst_refill_per_sec: 50.0,
        }
    }
}

impl ShieldConfig {
    pub fn from_env() -> Self {
        let defaults = Self::default();
        let flag = |key: &str, default: bool| -> bool {
            std::env::var(key)
                .ok()
                .map(|v| v.trim() == "1")
                .unwrap_or(default)
        };
        Self {
            bot_deny: flag("ALPHA_GATEWAY_BOT_DENY", defaults.bot_deny),
            scan_enabled: flag("ALPHA_GATEWAY_SCAN_DETECT", defaults.scan_enabled),
            scan_window_ms: std::env::var("ALPHA_GATEWAY_SCAN_WINDOW_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.scan_window_ms),
            scan_max_distinct: std::env::var("ALPHA_GATEWAY_SCAN_MAX_DISTINCT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.scan_max_distinct),
            burst_enabled: flag("ALPHA_GATEWAY_BURST_GUARD", defaults.burst_enabled),
            burst_capacity: std::env::var("ALPHA_GATEWAY_BURST_CAPACITY")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.burst_capacity),
            burst_refill_per_sec: std::env::var("ALPHA_GATEWAY_BURST_REFILL_PER_SEC")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(defaults.burst_refill_per_sec),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_agent_classification_table() {
        // 常规浏览器 UA 放行
        assert_eq!(
            classify_user_agent(Some("Mozilla/5.0 (X11; Linux x86_64) Chrome/126.0")),
            UaClass::Normal
        );
        // 缺头 / 空白 UA：空值类（开启 bot_deny 时一并拒绝）
        assert_eq!(classify_user_agent(None), UaClass::Empty);
        assert_eq!(classify_user_agent(Some("   ")), UaClass::Empty);
        // 已知脚本与爬虫特征（大小写不敏感）
        for ua in [
            "curl/8.5.0",
            "Wget/1.21",
            "python-requests/2.31",
            "Scrapy/2.11",
            "Go-http-client/2.0",
            "Googlebot/2.1",
            "Mozilla/5.0 (compatible; bingbot/2.0)",
            "HeadlessChrome/126",
        ] {
            assert_eq!(classify_user_agent(Some(ua)), UaClass::KnownBot, "{ua}");
        }
        // 词根误伤防护：含 curl 字样但非 curl 签名的不命中
        assert_eq!(
            classify_user_agent(Some("CurlicueApp/1.0")),
            UaClass::Normal
        );
    }

    #[test]
    fn scan_detector_flags_distinct_paths_not_repeats() {
        let mut detector = ScanDetector::new(60_000, 3);
        // 重复打同一路径：热点行为，不触发
        for ts in [1, 2, 3, 4, 5] {
            assert!(!detector.observe("a", "/quote", ts));
        }
        // 离散路径累计（/quote 仍在窗口内计入）：第 4 条离散路径越阈
        assert!(!detector.observe("a", "/a", 10));
        assert!(!detector.observe("a", "/b", 11));
        assert!(detector.observe("a", "/c", 12));
        // 不同身份独立分桶
        assert!(!detector.observe("b", "/a", 14));
        // 窗口过期：整窗清出后重新计数
        assert!(!detector.observe("a", "/x", 60_050));
        assert!(!detector.observe("a", "/y", 60_051));
        assert!(!detector.observe("a", "/z", 60_052));
    }

    #[test]
    fn burst_guard_absorbs_burst_then_refills() {
        let mut guard = BurstGuard::new(3, 2.0);
        let mut now = 1_000;
        // 桶深 3：第 4 发被拒
        assert!(guard.try_acquire("a", now));
        assert!(guard.try_acquire("a", now));
        assert!(guard.try_acquire("a", now));
        assert!(!guard.try_acquire("a", now));
        // 500ms 补 1 枚（2/s）→ 放行，随后又空
        now += 500;
        assert!(guard.try_acquire("a", now));
        assert!(!guard.try_acquire("a", now));
        // 长闲后满桶恢复；身份间互不影响
        now += 10_000;
        assert!(guard.try_acquire("b", now));
        for _ in 0..3 {
            assert!(guard.try_acquire("a", now));
        }
        assert!(!guard.try_acquire("a", now));
    }

    #[test]
    fn config_defaults_keep_scripts_alive() {
        let defaults = ShieldConfig::default();
        // 默认形态：bot 拒绝关（curl/脚本既有调用方不破坏），扫描与
        // burst 开（宽阈值，正常客户端与 e2e 流量不命中）
        assert!(!defaults.bot_deny);
        assert!(defaults.scan_enabled);
        assert!(defaults.burst_enabled);
        assert!(defaults.scan_max_distinct >= 100);
        assert!(defaults.burst_capacity >= 50);
    }
}
