//! 后台同步决策（L337，docs/mobile-push-sync.md §3）
//!
//! 分层纪律：**核心库决定何时该同步（间隔闸门 + 状态 + 指纹），壳层负责唤醒
//! 与取数**（Android WorkManager / iOS BGTaskScheduler 调度，平台网络执行）。
//! 触发时机四种：`Periodic` 受间隔闸门约束，回前台/网络恢复/手动刷新无条件
//! 出计划——骨架期无真实远端（api_url 仍是配置槽，取数执行归 L339），本模块
//! 先把**计划 / 闸门 / 指纹**三件套的契约立住。指纹沿桌面 L116
//! `content_seq(price, volume)` 口径（同内容同指纹 → 同步可跳过）。

use alpha_core::models::MarketData;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// 默认同步间隔（秒）——骨架值；壳层实际周期 ≥ 它即兼容（系统钳制见假设③）
pub const DEFAULT_INTERVAL_SECS: u64 = 300;

/// 同步触发时机（字符串枚举经 FFI：`periodic` / `foreground` /
/// `connectivity_restored` / `manual`，未知值走 `Failed`）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncTrigger {
    /// 壳层定时器（唯一受间隔闸门约束的触发）
    Periodic,
    /// 应用回前台（线上串显式定为 `foreground`——比 snake_case 推导的
    /// `app_foreground` 短，且为文档 §3/§8⑥ 的既定口径）
    #[serde(rename = "foreground")]
    AppForeground,
    /// 网络恢复
    ConnectivityRestored,
    /// 用户手动刷新
    Manual,
}

/// 同步间隔配置（骨架期常量，动态下发归 L339）
#[derive(Debug, Clone)]
pub struct SyncConfig {
    /// 间隔秒数
    pub interval_secs: u64,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            interval_secs: DEFAULT_INTERVAL_SECS,
        }
    }
}

/// 同步状态（经 FFI 下行给壳层展示与调度）
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SyncStatus {
    /// 上次同步时刻（从未同步为 `null`）
    pub last_sync: Option<DateTime<Utc>>,
    /// 当前内容指纹（0 = 从未同步）
    pub fingerprint: u64,
    /// 间隔秒数
    pub interval_secs: u64,
    /// 按 `Periodic` 触发此刻是否到期
    pub due: bool,
}

/// 同步计划（壳层据此取数；`due=false` 时只带原因不带工作项）
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SyncPlan {
    /// 是否到期可同步
    pub due: bool,
    /// 触发来源
    pub trigger: SyncTrigger,
    /// 需要覆盖的观察列表（未到期为空）
    pub symbols: Vec<String>,
    /// 上次同步的内容指纹（增量语义起点）
    pub since_fingerprint: u64,
    /// 间隔秒数
    pub interval_secs: u64,
    /// 未到期原因（到期时缺省不序列化）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// 内容指纹：价格 + 成交量的确定性哈希（**与桌面 L116 `content_seq` 同算法**：
/// FNV-1a 64 位，价格 f64 位模式 + 成交量 u64 小端字节；行情域不出现 NaN）
pub fn content_seq(price: f64, volume: u64) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in price
        .to_le_bytes()
        .iter()
        .chain(volume.to_le_bytes().iter())
    {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    hash
}

/// 观察列表整体指纹：按 **symbol 排序**逐标的折入（与传入顺序无关，避免壳层
/// 排序差异造成指纹漂移——假设⑤）
pub fn fingerprint_of(quotes: &[MarketData]) -> u64 {
    let mut sorted: Vec<&MarketData> = quotes.iter().collect();
    sorted.sort_by(|a, b| a.symbol.cmp(&b.symbol));
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for quote in sorted {
        for byte in quote.symbol.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100_0000_01b3);
        }
        let seq = content_seq(quote.price, quote.volume);
        for byte in seq.to_le_bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100_0000_01b3);
        }
    }
    hash
}

/// 同步决策状态（挂 `Mutex` 于 [`crate::MobileCore`]，时间可注入便于单测）
#[derive(Debug, Default)]
pub struct BackgroundSync {
    config: SyncConfig,
    last_sync: Option<DateTime<Utc>>,
    fingerprint: u64,
}

impl BackgroundSync {
    /// 以给定配置构建（默认间隔）
    pub fn new(config: SyncConfig) -> Self {
        Self {
            config,
            last_sync: None,
            fingerprint: 0,
        }
    }

    /// `Periodic` 此刻是否到期：从未同步立即到期；否则距上次 ≥ 间隔才到期
    fn periodic_due(&self, now: DateTime<Utc>) -> bool {
        match self.last_sync {
            None => true,
            Some(last) => (now - last).num_seconds() >= self.config.interval_secs as i64,
        }
    }

    /// 出同步计划（时间与观察列表注入；`Periodic` 过闸门，其余无条件）
    pub fn decide_at(
        &self,
        trigger: SyncTrigger,
        now: DateTime<Utc>,
        symbols: &[String],
    ) -> SyncPlan {
        let due = match trigger {
            SyncTrigger::Periodic => self.periodic_due(now),
            _ => true,
        };
        if due {
            SyncPlan {
                due: true,
                trigger,
                symbols: symbols.to_vec(),
                since_fingerprint: self.fingerprint,
                interval_secs: self.config.interval_secs,
                reason: None,
            }
        } else {
            SyncPlan {
                due: false,
                trigger,
                symbols: Vec::new(),
                since_fingerprint: self.fingerprint,
                interval_secs: self.config.interval_secs,
                reason: Some("interval_not_elapsed".to_string()),
            }
        }
    }

    /// 标记已同步：记时刻 + 以当前行情重算指纹（算法不外泄，壳层只透传状态）
    pub fn mark_synced_at(&mut self, now: DateTime<Utc>, quotes: &[MarketData]) -> SyncStatus {
        self.last_sync = Some(now);
        self.fingerprint = fingerprint_of(quotes);
        self.status_at(now)
    }

    /// 当前状态（`due` 按 `Periodic` 口径）
    pub fn status_at(&self, now: DateTime<Utc>) -> SyncStatus {
        SyncStatus {
            last_sync: self.last_sync,
            fingerprint: self.fingerprint,
            interval_secs: self.config.interval_secs,
            due: self.periodic_due(now),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_time() -> DateTime<Utc> {
        DateTime::from_timestamp(1_760_000_000, 0).expect("固定时刻")
    }

    fn quote(symbol: &str, price: f64) -> MarketData {
        MarketData::new(symbol.to_string(), price, 1_000)
    }

    fn watch() -> Vec<String> {
        vec!["600519".to_string(), "000001".to_string()]
    }

    /// Periodic 未到间隔不出计划，出原因
    #[test]
    fn periodic_is_gated_by_interval() {
        let mut state = BackgroundSync::default();
        let t0 = base_time();
        state.mark_synced_at(t0, &[quote("600519", 90.0)]);

        let before = state.decide_at(
            SyncTrigger::Periodic,
            t0 + chrono::Duration::seconds(299),
            &watch(),
        );
        assert!(!before.due, "299s < 300s 不到期");
        assert_eq!(before.reason.as_deref(), Some("interval_not_elapsed"));
        assert!(before.symbols.is_empty(), "未到期不带工作项");

        let after = state.decide_at(
            SyncTrigger::Periodic,
            t0 + chrono::Duration::seconds(300),
            &watch(),
        );
        assert!(after.due, "满 300s 到期");
        assert_eq!(after.reason, None);
        assert_eq!(after.symbols, watch(), "到期计划带观察列表");
    }

    /// 从未同步：Periodic 立即可同步
    #[test]
    fn first_sync_is_due_immediately() {
        let state = BackgroundSync::default();
        let plan = state.decide_at(SyncTrigger::Periodic, base_time(), &watch());
        assert!(plan.due, "无上次同步时刻立即到期");
        assert_eq!(plan.since_fingerprint, 0, "初始指纹为 0");
    }

    /// 非 Periodic 三种触发无条件出计划（回前台/网络恢复/手动没有理由拒绝）
    #[test]
    fn non_periodic_triggers_bypass_interval() {
        let mut state = BackgroundSync::default();
        let t0 = base_time();
        state.mark_synced_at(t0, &[quote("600519", 90.0)]);
        for trigger in [
            SyncTrigger::AppForeground,
            SyncTrigger::ConnectivityRestored,
            SyncTrigger::Manual,
        ] {
            let plan = state.decide_at(trigger, t0, &watch());
            assert!(plan.due, "{trigger:?} 不受间隔约束");
            assert_eq!(plan.trigger, trigger);
        }
    }

    /// 计划携带上次指纹（增量起点）与间隔
    #[test]
    fn plan_carries_fingerprint_and_interval() {
        let mut state = BackgroundSync::default();
        let t0 = base_time();
        let status = state.mark_synced_at(t0, &[quote("600519", 90.0)]);
        let plan = state.decide_at(SyncTrigger::Manual, t0, &watch());
        assert_eq!(
            plan.since_fingerprint, status.fingerprint,
            "增量起点=上次指纹"
        );
        assert_eq!(plan.interval_secs, DEFAULT_INTERVAL_SECS);
    }

    /// mark_synced 后状态刷新、Periodic 闸门重置
    #[test]
    fn mark_synced_resets_gate_and_updates_status() {
        let mut state = BackgroundSync::default();
        let t0 = base_time();
        let status = state.mark_synced_at(t0, &[quote("600519", 90.0)]);
        assert_eq!(status.last_sync, Some(t0));
        assert_ne!(status.fingerprint, 0, "有行情的指纹非 0");
        assert!(!status.due, "刚同步完不 due");

        let later = state.status_at(t0 + chrono::Duration::seconds(301));
        assert!(later.due, "过闸后重新 due");
    }

    /// 指纹确定性 + 与顺序无关（壳层传序不造成漂移）
    #[test]
    fn fingerprint_is_deterministic_and_order_independent() {
        let a = vec![quote("600519", 90.0), quote("000001", 10.0)];
        let mut b = a.clone();
        b.reverse();
        assert_eq!(fingerprint_of(&a), fingerprint_of(&a), "同数据同指纹");
        assert_eq!(fingerprint_of(&a), fingerprint_of(&b), "顺序无关");
        assert_ne!(fingerprint_of(&a), fingerprint_of(&[]), "空列表与非空不同");
    }

    /// 内容变 → 指纹变（价格/成交量任一变化均触发）
    #[test]
    fn fingerprint_tracks_content_changes() {
        let base = vec![quote("600519", 90.0)];
        let price_changed = vec![quote("600519", 90.5)];
        let mut volume_changed = vec![quote("600519", 90.0)];
        volume_changed[0].volume = 2_000;
        let fp = fingerprint_of(&base);
        assert_ne!(fp, fingerprint_of(&price_changed), "价格变化 → 指纹变化");
        assert_ne!(fp, fingerprint_of(&volume_changed), "成交量变化 → 指纹变化");
        // content_seq 与桌面 L116 同算法（同输入同输出可跨端断言的依据）
        assert_eq!(content_seq(100.0, 1_000), content_seq(100.0, 1_000));
        assert_ne!(content_seq(100.0, 1_000), content_seq(100.5, 1_000));
    }

    /// 触发字符串经 serde 往返（FFI 解析路径）
    #[test]
    fn trigger_strings_round_trip() {
        for (text, expected) in [
            ("\"periodic\"", SyncTrigger::Periodic),
            ("\"foreground\"", SyncTrigger::AppForeground),
            (
                "\"connectivity_restored\"",
                SyncTrigger::ConnectivityRestored,
            ),
            ("\"manual\"", SyncTrigger::Manual),
        ] {
            let parsed: SyncTrigger = serde_json::from_str(text).expect("解析触发串");
            assert_eq!(parsed, expected);
            assert_eq!(
                serde_json::to_string(&parsed).expect("序列化"),
                text,
                "往返一致"
            );
        }
        assert!(
            serde_json::from_str::<SyncTrigger>("\"nope\"").is_err(),
            "未知值拒绝"
        );
    }

    /// 状态载荷字段契约（§4：last_sync/fingerprint/interval_secs/due）
    #[test]
    fn status_keeps_payload_contract() {
        let state = BackgroundSync::default();
        let value = serde_json::to_value(state.status_at(base_time())).expect("序列化");
        for key in ["last_sync", "fingerprint", "interval_secs", "due"] {
            assert!(value.get(key).is_some(), "缺字段 {key}: {value}");
        }
        assert!(value["last_sync"].is_null(), "从未同步 last_sync 为 null");
        assert_eq!(value["interval_secs"], DEFAULT_INTERVAL_SECS);
    }

    /// 计划载荷字段契约（未到期带 reason，到期不带）
    #[test]
    fn plan_keeps_payload_contract() {
        let mut state = BackgroundSync::default();
        let t0 = base_time();
        state.mark_synced_at(t0, &[quote("600519", 90.0)]);

        let not_due = serde_json::to_value(state.decide_at(SyncTrigger::Periodic, t0, &watch()))
            .expect("序列化");
        for key in [
            "due",
            "trigger",
            "symbols",
            "since_fingerprint",
            "interval_secs",
            "reason",
        ] {
            assert!(not_due.get(key).is_some(), "未到期载荷缺 {key}: {not_due}");
        }
        assert_eq!(not_due["due"], false);
        assert_eq!(not_due["trigger"], "periodic");

        let due = serde_json::to_value(state.decide_at(SyncTrigger::Manual, t0, &watch()))
            .expect("序列化");
        assert!(due.get("reason").is_none(), "到期不带 reason");
        assert_eq!(due["due"], true);
    }
}
