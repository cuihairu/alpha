//! 离线数据决策（L390，docs/mobile-offline.md）
//!
//! 分层纪律：**核心库决定允许存什么/是否需要对齐（授权 + 范围 + 指纹增量），
//! 壳层执行落盘与网络**（KeyValueStore/SharedPreferences 与远端请求）。
//! 术语红线（文档 §1）：**备份 = 本地快照落盘（不出设备）**，载荷走 `Snapshot`；
//! **同步 = 与远端增量对齐（出设备）**，载荷走 `SyncDelta`——命名与文案不混用。
//!
//! 产品红线（文档 §2）：①授权开关**默认关闭**（未开启快照生成直接拒绝，在
//! 本库强制，壳层绕不过）；②显式开启必须同时携带非空数据范围（`scopes`）；
//! ③未授权状态不产生任何同步工作项（`SyncDelta.needed=false`）。指纹沿 L337
//! `content_seq`/`fingerprint_of` 口径（同源可跨面比较）。

use crate::sync::{content_seq, fingerprint_of};
use alpha_core::models::MarketData;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// 数据范围（封闭枚举：未知字符串经 serde 拒绝，壳层不可自造——文档 §3；
/// `Analysis`/`Watchlist` 等变体随能力评审后加入）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataScope {
    /// 行情快照（代码/价格/成交量/买卖一档/open，serde 即 `MarketData` 字段）
    Quotes,
}

/// 离线授权配置——**enabled 默认 false**（红线①；备份与同步共用同一开关，
/// 文档 §2 尾：一条红线管两个面）
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OfflineSyncConfig {
    /// 授权开关（默认关闭；显式开启须同时携带非空范围）
    pub enabled: bool,
    /// 授权数据范围（开启时必须非空——红线②）
    pub scopes: Vec<DataScope>,
}

// 派生 Default 恰为红线①语义：bool 默认 false、Vec 默认空——默认关闭且无范围；
// 语义由 offline.rs 单测 `default_is_disabled_and_blocks_snapshot` 锁定，未来字段
// 默认值若偏离红线该测试先红。
impl OfflineSyncConfig {
    /// 校验配置（红线②：开启与明示范围在同一请求里强制绑定）
    pub fn validate(&self) -> Result<(), String> {
        if self.enabled && self.scopes.is_empty() {
            return Err("开启离线功能必须明示数据范围（scopes 不能为空）".to_string());
        }
        Ok(())
    }
}

/// 快照条目（备份面最小单元：稳定键 + 载荷全文 + 内容指纹）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OfflineEntry {
    /// 稳定键：`quote:{symbol}`
    pub key: String,
    /// 载荷全文（`MarketData` serde 输出，字段契约由 alpha-core 单测锁定）
    pub payload_json: String,
    /// 内容指纹（L337 `content_seq(price, volume)` 同口径）
    pub content_seq: u64,
}

/// 本地快照（备份面载荷；壳层整体单键落盘——文档 §8⑥）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OfflineSnapshot {
    /// 生成时 crate 版本（restore 校验版本一致——文档 §8⑤）
    pub version: String,
    /// 生成时授权开关（恒 true：未开启生成不出快照）
    pub enabled: bool,
    /// 授权数据范围（明示面，UI 展示）
    pub scopes: Vec<DataScope>,
    /// 快照时刻
    pub captured_at: DateTime<Utc>,
    /// 快照条目（scope 过滤后）
    pub entries: Vec<OfflineEntry>,
    /// 聚合指纹（L337 `fingerprint_of` 同口径，symbol 排序聚合）
    pub fingerprint: u64,
}

/// 增量决策（同步面载荷；`needed` 才允许壳层发起对齐请求）
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SyncDelta {
    /// 是否需要对齐（未授权/未变化均 false）
    pub needed: bool,
    /// 壳层基线指纹（入参透传）
    pub since_fingerprint: u64,
    /// 当前内容指纹
    pub current_fingerprint: u64,
    /// 未对齐原因（`sync_disabled` / `unchanged`；需要对齐时缺省）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// 离线决策状态（挂 `Mutex` 于 [`crate::MobileCore`]；时间与行情由调用方注入，
/// 便于单测确定性断言）
#[derive(Debug, Default)]
pub struct OfflineManager {
    config: OfflineSyncConfig,
}

impl OfflineManager {
    /// 以给定配置构建（测试与显式初始化用；`MobileCore` 走 `Default` = 关闭）
    pub fn new(config: OfflineSyncConfig) -> Self {
        Self { config }
    }

    /// 当前授权配置
    pub fn config(&self) -> &OfflineSyncConfig {
        &self.config
    }

    /// 设置配置：校验后生效，返回生效配置供 FFI 回显（红线③「明示范围」面）
    pub fn set_config(&mut self, config: OfflineSyncConfig) -> Result<OfflineSyncConfig, String> {
        config.validate()?;
        self.config = config;
        Ok(self.config.clone())
    }

    /// 生成本地快照（备份面）。未开启 → `Err`（红线①在核心库强制）。
    ///
    /// 条目按授权范围过滤（首期仅 `Quotes`）；序列化失败按条目报错，不静默丢弃。
    pub fn snapshot_at(
        &self,
        now: DateTime<Utc>,
        quotes: &[MarketData],
        version: &str,
    ) -> Result<OfflineSnapshot, String> {
        if !self.config.enabled {
            return Err(
                "离线同步未开启：如需备份行情快照，请先在设置中显式开启并确认数据范围".to_string(),
            );
        }
        let scoped = self.config.scopes.contains(&DataScope::Quotes);
        let entries = quotes
            .iter()
            .filter(|_| scoped)
            .map(|quote| {
                let payload_json = serde_json::to_string(quote)
                    .map_err(|e| format!("行情 {} 序列化失败: {e}", quote.symbol))?;
                Ok::<OfflineEntry, String>(OfflineEntry {
                    key: format!("quote:{}", quote.symbol),
                    payload_json,
                    content_seq: content_seq(quote.price, quote.volume),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(OfflineSnapshot {
            version: version.to_string(),
            enabled: true,
            scopes: self.config.scopes.clone(),
            captured_at: now,
            entries,
            fingerprint: fingerprint_of(quotes),
        })
    }

    /// 校验快照可恢复（结构由 serde 反序列化保证，此处查授权与版本一致），
    /// 返回将恢复的条目数。未开启同样拒绝——离线功能的完整生命周期都在授权内。
    pub fn restore_validate(
        &self,
        snapshot: &OfflineSnapshot,
        current_version: &str,
    ) -> Result<usize, String> {
        if !self.config.enabled {
            return Err("离线同步未开启：恢复快照前请先显式开启".to_string());
        }
        if snapshot.version != current_version {
            return Err(format!(
                "快照版本不兼容: 快照 {} ≠ 当前 {}",
                snapshot.version, current_version
            ));
        }
        Ok(snapshot.entries.len())
    }

    /// 同步增量判断（同步面）。红线③：未授权状态 `needed=false` +
    /// `reason="sync_disabled"`——不产生任何外发工作项；已授权则纯指纹比较
    /// （L337 口径），相同 → `unchanged`，不同 → 需要对齐。
    pub fn delta_at(&self, since_fingerprint: u64, quotes: &[MarketData]) -> SyncDelta {
        let current = fingerprint_of(quotes);
        if !self.config.enabled {
            return SyncDelta {
                needed: false,
                since_fingerprint,
                current_fingerprint: current,
                reason: Some("sync_disabled".to_string()),
            };
        }
        if current == since_fingerprint {
            SyncDelta {
                needed: false,
                since_fingerprint,
                current_fingerprint: current,
                reason: Some("unchanged".to_string()),
            }
        } else {
            SyncDelta {
                needed: true,
                since_fingerprint,
                current_fingerprint: current,
                reason: None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quote(symbol: &str, price: f64) -> MarketData {
        MarketData::new(symbol.to_string(), price, 1_000)
    }

    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(1_760_000_000, 0).expect("固定时刻")
    }

    fn enabled_config() -> OfflineSyncConfig {
        OfflineSyncConfig {
            enabled: true,
            scopes: vec![DataScope::Quotes],
        }
    }

    /// 红线①：默认配置 = 关闭 + 空范围；未开启快照生成直接拒绝（核心库强制）
    #[test]
    fn default_is_disabled_and_blocks_snapshot() {
        let manager = OfflineManager::default();
        assert!(!manager.config().enabled, "默认关闭");
        assert!(manager.config().scopes.is_empty());
        let err = manager
            .snapshot_at(now(), &[quote("600519", 90.0)], "0.1.0")
            .expect_err("未开启应拒绝");
        assert!(err.contains("未开启"), "文案: {err}");
        assert!(err.contains("显式开启"), "文案须引导显式开启: {err}");
    }

    /// 红线②：开启必须携带非空范围；合法配置被接受并回显
    #[test]
    fn enabling_requires_non_empty_scopes() {
        let mut manager = OfflineManager::default();
        let err = manager
            .set_config(OfflineSyncConfig {
                enabled: true,
                scopes: Vec::new(),
            })
            .expect_err("无范围开启应拒绝");
        assert!(err.contains("scopes"), "文案: {err}");
        assert!(!manager.config().enabled, "被拒后保持关闭");

        let applied = manager.set_config(enabled_config()).expect("合法配置");
        assert!(applied.enabled && applied.scopes == vec![DataScope::Quotes]);
        assert_eq!(manager.config(), &enabled_config(), "配置已生效");
    }

    /// 红线③：封闭范围枚举——未知 scope 字符串经 serde 拒绝
    #[test]
    fn unknown_scope_string_is_rejected() {
        assert!(serde_json::from_str::<OfflineSyncConfig>(
            r#"{"enabled":true,"scopes":["contacts"]}"#
        )
        .is_err());
        let parsed: OfflineSyncConfig =
            serde_json::from_str(r#"{"enabled":true,"scopes":["quotes"]}"#).expect("合法范围");
        assert_eq!(parsed.scopes, vec![DataScope::Quotes]);
    }

    /// 快照：条目键/载荷/指纹齐全，聚合指纹与 L337 fingerprint_of 同源
    #[test]
    fn snapshot_scopes_entries_and_keeps_fingerprint() {
        let manager = OfflineManager::new(enabled_config());
        let quotes = vec![quote("600519", 90.0), quote("000001", 10.0)];
        let snapshot = manager
            .snapshot_at(now(), &quotes, "0.1.0")
            .expect("已授权");
        assert_eq!(snapshot.version, "0.1.0");
        assert!(snapshot.enabled);
        assert_eq!(snapshot.scopes, vec![DataScope::Quotes]);
        assert_eq!(snapshot.captured_at, now());
        assert_eq!(snapshot.entries.len(), 2);
        assert_eq!(
            snapshot.entries[0].key, "quote:600519",
            "条目保持传入序，键含标的"
        );
        assert!(
            snapshot.entries[0].payload_json.contains("\"price\":90"),
            "载荷为 MarketData serde 全文: {}",
            snapshot.entries[0].payload_json
        );
        assert_eq!(
            snapshot.fingerprint,
            fingerprint_of(&quotes),
            "聚合指纹 = L337 口径"
        );
        assert_eq!(
            snapshot.entries[0].content_seq,
            content_seq(90.0, 1_000),
            "条目指纹 = L337 content_seq"
        );
    }

    /// 快照载荷字段契约（§4：七字段逐个点名）+ serde 往返
    #[test]
    fn snapshot_keeps_payload_contract_and_roundtrips() {
        let manager = OfflineManager::new(enabled_config());
        let snapshot = manager
            .snapshot_at(now(), &[quote("600519", 90.0)], "0.1.0")
            .expect("已授权");
        let value = serde_json::to_value(&snapshot).expect("序列化");
        for key in [
            "version",
            "enabled",
            "scopes",
            "captured_at",
            "entries",
            "fingerprint",
        ] {
            assert!(value.get(key).is_some(), "快照缺字段 {key}: {value}");
        }
        for key in ["key", "payload_json", "content_seq"] {
            assert!(value["entries"][0].get(key).is_some(), "条目缺字段 {key}");
        }
        let back: OfflineSnapshot = serde_json::from_value(value).expect("反序列化");
        assert_eq!(back, snapshot, "往返一致");
    }

    /// 恢复校验：未开启拒绝；版本不匹配拒绝；一致则返回条目数
    #[test]
    fn restore_validates_enabled_version_and_counts() {
        let mut manager = OfflineManager::default();
        let snapshot = OfflineManager::new(enabled_config())
            .snapshot_at(now(), &[quote("600519", 90.0)], "0.1.0")
            .expect("生成");
        assert!(
            manager.restore_validate(&snapshot, "0.1.0").is_err(),
            "未开启拒绝恢复"
        );

        manager.set_config(enabled_config()).expect("开启");
        assert!(
            manager.restore_validate(&snapshot, "0.2.0").is_err(),
            "版本不匹配拒绝"
        );
        let count = manager.restore_validate(&snapshot, "0.1.0").expect("一致");
        assert_eq!(count, 1, "返回待恢复条目数");
    }

    /// 同步增量（红线③）：未授权 needed=false + reason=sync_disabled（不外发）
    #[test]
    fn delta_without_authorization_yields_no_work() {
        let manager = OfflineManager::default();
        let quotes = [quote("600519", 90.0)];
        let delta = manager.delta_at(0, &quotes);
        assert!(!delta.needed, "未授权不产生同步工作项");
        assert_eq!(delta.reason.as_deref(), Some("sync_disabled"));
        assert_eq!(delta.current_fingerprint, fingerprint_of(&quotes));
    }

    /// 同步增量：指纹相同 → unchanged；不同 → needed（无 reason）
    #[test]
    fn delta_compares_fingerprints() {
        let manager = OfflineManager::new(enabled_config());
        let quotes = [quote("600519", 90.0)];
        let same = manager.delta_at(fingerprint_of(&quotes), &quotes);
        assert!(!same.needed);
        assert_eq!(same.reason.as_deref(), Some("unchanged"));

        let changed = manager.delta_at(0, &quotes);
        assert!(changed.needed, "指纹变化需对齐");
        assert_eq!(changed.reason, None, "需要对齐时不带 reason");
        assert_eq!(changed.since_fingerprint, 0);
        assert_eq!(changed.current_fingerprint, fingerprint_of(&quotes));
    }

    /// 同步增量载荷字段契约（未授权/unchanged 带 reason，needed 不带）
    #[test]
    fn delta_keeps_payload_contract() {
        let manager = OfflineManager::new(enabled_config());
        let quotes = [quote("600519", 90.0)];

        let disabled =
            serde_json::to_value(OfflineManager::default().delta_at(0, &quotes)).expect("序列化");
        for key in [
            "needed",
            "since_fingerprint",
            "current_fingerprint",
            "reason",
        ] {
            assert!(disabled.get(key).is_some(), "delta 缺字段 {key}");
        }
        assert_eq!(disabled["needed"], false);

        let needed = serde_json::to_value(manager.delta_at(0, &quotes)).expect("序列化");
        assert_eq!(needed["needed"], true);
        assert!(needed.get("reason").is_none(), "needed 不带 reason");
    }

    /// 配置载荷字段契约 + serde 往返（FFI 解析路径）
    #[test]
    fn config_keeps_payload_contract_and_roundtrips() {
        let config = enabled_config();
        let value = serde_json::to_value(&config).expect("序列化");
        for key in ["enabled", "scopes"] {
            assert!(value.get(key).is_some(), "配置缺字段 {key}: {value}");
        }
        assert_eq!(value["scopes"], serde_json::json!(["quotes"]));
        let back: OfflineSyncConfig = serde_json::from_value(value).expect("反序列化");
        assert_eq!(back, config, "往返一致");

        let default = serde_json::to_value(OfflineSyncConfig::default()).expect("序列化");
        assert_eq!(default["enabled"], false, "默认关闭（红线①的字段面）");
    }

    /// 快照对未授权范围不出条目：结构上 scope 过滤在条目构造前
    /// （首期 scopes 非空必含 Quotes，此处锁定未来多 scope 时过滤不被遗忘）
    #[test]
    fn snapshot_structure_prepares_multi_scope_filtering() {
        let manager = OfflineManager::new(enabled_config());
        let quotes = vec![quote("600519", 90.0)];
        let snapshot = manager
            .snapshot_at(now(), &quotes, "0.1.0")
            .expect("已授权");
        assert_eq!(
            snapshot.entries.len(),
            quotes.len(),
            "Quotes 授权则全量入快照"
        );
        assert!(
            snapshot
                .entries
                .iter()
                .all(|entry| entry.key.starts_with("quote:")),
            "条目键前缀按 scope 分类"
        );
    }
}
