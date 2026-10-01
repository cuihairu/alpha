//! 移动端核心状态与 FFI 导出（L118；L337 增推送/同步决策面）
//!
//! [`MobileCore`] 是平台壳经 uniffi 拿到的唯一对象：观察列表 + 配置槽 +
//! 分析引擎 + 演示数据源 + 推送/同步决策（`notify`/`sync` 模块）。FFI 载荷为
//! JSON 字符串（字段契约由单测锁定，见 docs/mobile-core-architecture.md §4 与
//! docs/mobile-push-sync.md §4）；错误经 [`MobileError`]（`uniffi::Error`）
//! 以类型化异常跨桥（§5）。观察列表是核心库唯一的业务判断：列表外标的统一
//! `InvalidSymbol`（§7），平台壳不重复实现。
//!
//! **只增不改**纪律：L118 构造器与三方法签名原样（L119 iOS 壳在用），L337
//! 追加六个推送/同步方法（docs/mobile-push-sync.md §4）。

use crate::notify::{AlertRule, LocalQueueChannel, Notifier};
use crate::offline::{OfflineManager, OfflineSnapshot, OfflineSyncConfig};
use crate::sync::{BackgroundSync, SyncTrigger};
use alpha_core::analytics::AnalysisEngine;
use alpha_core::errors::AlphaError;
use alpha_core::models::{AnalysisResult, MarketData};
use std::future::Future;
use std::sync::{Arc, Mutex};

/// 移动端 FFI 错误（跨桥为类型化异常；分类在 Rust 侧判定，平台壳只展示）
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum MobileError {
    /// 观察列表外的标的（含空 symbol、空列表退化——能力边界统一在核心库）
    #[error("标的 {symbol} 不在观察列表")]
    InvalidSymbol {
        /// 被拒绝的标的代码
        symbol: String,
    },
    /// 其余核心错误（`AlphaError` 等经 Display 全文透传，不猜不缩）
    ///
    /// 字段名不能叫 `message`：uniffi 生成的 Kotlin 会给错误类自动加
    /// `override val message` 展示属性，构造属性与之同名即冲突（K1/K2 都
    /// 编不过）——L301 实证，字段一律避开 `message`。
    #[error("{detail}")]
    Failed {
        /// 错误全文（`Display` 输出）
        detail: String,
    },
}

impl From<AlphaError> for MobileError {
    fn from(err: AlphaError) -> Self {
        MobileError::Failed {
            detail: err.to_string(),
        }
    }
}

/// 移动端核心状态（uniffi Object，`Arc` 共享、可跨线程）
///
/// 纯数据 + 引擎，无运行期句柄——`analyze` 按调用建 current-thread 运行时驱动
/// alpha-core 的 future（其 `analyze_symbol` 形为 async、体为零 await 纯计算），
/// 故本对象 `Send + Sync`，uniffi 侧多线程调用安全（压测属真机边界）。
/// 推送/同步决策状态各挂一把 `Mutex`（L337，文档 §6），保持锁粒度最小。
#[derive(Debug, uniffi::Object)]
pub struct MobileCore {
    /// 观察列表（能力边界，见模块文档）
    symbols: Vec<String>,
    /// 后端地址配置槽（骨架期只随 `status_json` 下行；探测/同步归后续 TODO）
    api_url: String,
    /// 分享给平台壳的分析引擎（与 web/desktop 同一份 alpha-core 计算）
    engine: AnalysisEngine,
    /// 推送决策（规则/已发集/本地队列通道，L337）
    notifier: Mutex<Notifier<LocalQueueChannel>>,
    /// 后台同步决策（闸门/状态/指纹，L337）
    sync: Mutex<BackgroundSync>,
    /// 离线授权与增量决策（默认关闭，L390 红线在核心库强制）
    offline: Mutex<OfflineManager>,
}

/// 驱动一个 future 到完成（仅骨架期使用：见模块文档与 §6 线程模型；
/// 复用 `Runtime` 的优化留到真机有实测数据后）
fn block_on<T>(future: impl Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("创建 current-thread 运行时")
        .block_on(future)
}

impl MobileCore {
    /// 观察列表
    pub fn symbols(&self) -> &[String] {
        &self.symbols
    }

    /// 后端地址配置槽
    pub fn api_url(&self) -> &str {
        &self.api_url
    }

    /// 观察列表判定（核心库唯一业务判断；列表外一律 `InvalidSymbol`）
    fn ensure_watched(&self, symbol: &str) -> Result<(), MobileError> {
        if self.symbols.iter().any(|watched| watched == symbol) {
            Ok(())
        } else {
            Err(MobileError::InvalidSymbol {
                symbol: symbol.to_string(),
            })
        }
    }

    /// 快照行情（观察列表内；演示数据源，见 `market` 模块）
    pub fn quote(&self, symbol: &str) -> Result<MarketData, MobileError> {
        self.ensure_watched(symbol)?;
        Ok(crate::market::synthetic_quote(symbol))
    }

    /// 技术分析（观察列表内；走 alpha-core `AnalysisEngine`）
    pub fn analyze(&self, symbol: &str) -> Result<AnalysisResult, MobileError> {
        self.ensure_watched(symbol)?;
        let series = crate::market::synthetic_series(symbol, crate::market::DEFAULT_BARS);
        Ok(block_on(self.engine.analyze_symbol(&series, None))?)
    }

    /// 观察列表当前行情（告警评估与指纹重算共用；演示数据源，文档 §8⑦）
    fn current_quotes(&self) -> Vec<MarketData> {
        self.symbols
            .iter()
            .map(|symbol| crate::market::synthetic_quote(symbol))
            .collect()
    }

    /// JSON 序列化失败 → `Failed{detail}`（与 L118 三方法同一口径）
    fn to_json<T: serde::Serialize>(value: &T) -> Result<String, MobileError> {
        serde_json::to_string(value).map_err(|e| MobileError::Failed {
            detail: e.to_string(),
        })
    }
}

/// FFI 导出面：构造器 + 三个 JSON 载荷方法（`setup_scaffolding!` 在 lib.rs，
/// proc-macro-only、无 UDL 副本）
#[uniffi::export]
impl MobileCore {
    /// 构造核心状态（uniffi 主构造器，Kotlin/Swift 侧为 `MobileCore(...)`）
    ///
    /// 签名自 L118 原样——**只增不改**纪律（L119 iOS 壳在用）；L337 新状态
    /// （推送/同步）在构造内以默认值初始化，不扩构造参数。
    #[uniffi::constructor]
    pub fn new(symbols: Vec<String>, api_url: String) -> Arc<Self> {
        Arc::new(Self {
            symbols,
            api_url,
            engine: AnalysisEngine::new(),
            notifier: Mutex::new(Notifier::new(LocalQueueChannel::default())),
            sync: Mutex::new(BackgroundSync::default()),
            offline: Mutex::new(OfflineManager::default()),
        })
    }

    /// 快照行情的 JSON 载荷（`MarketData` serde 字段即契约）
    ///
    /// FFI 参数用 owned `String`：uniffi 0.25 的 proc-macro 路径不支持 `&str`
    /// 提升（`LiftRef` 只对已导出类型与 owned 内建类型实现）
    pub fn quote_json(&self, symbol: String) -> Result<String, MobileError> {
        serde_json::to_string(&self.quote(&symbol)?).map_err(|e| MobileError::Failed {
            detail: e.to_string(),
        })
    }

    /// 分析结果的 JSON 载荷（`AnalysisResult` serde 字段即契约）
    pub fn analyze_json(&self, symbol: String) -> Result<String, MobileError> {
        serde_json::to_string(&self.analyze(&symbol)?).map_err(|e| MobileError::Failed {
            detail: e.to_string(),
        })
    }

    /// 状态快照：版本 + 观察列表 + 配置槽（平台壳启动时读一次）
    pub fn status_json(&self) -> String {
        serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "symbols": self.symbols,
            "api_url": self.api_url,
        })
        .to_string()
    }

    // ── L337 推送/同步决策面（六方法，只增不改——docs/mobile-push-sync.md §4）──

    /// 设置价格告警规则（整体替换；返回规则条数）
    ///
    /// 列表外标的 → `InvalidSymbol`；目标价非法（非有限或 ≤0）/JSON 解析
    /// 失败 → `Failed{detail}`。替换后已发集重武装（`Notifier::set_rules`）。
    pub fn set_alert_rules_json(&self, rules_json: String) -> Result<u64, MobileError> {
        let rules: Vec<AlertRule> =
            serde_json::from_str(&rules_json).map_err(|e| MobileError::Failed {
                detail: format!("告警规则解析失败: {e}"),
            })?;
        for rule in &rules {
            self.ensure_watched(&rule.symbol)?;
            if !rule.target_price.is_finite() || rule.target_price <= 0.0 {
                return Err(MobileError::Failed {
                    detail: format!("目标价非法（须为有限正数）: {}", rule.target_price),
                });
            }
        }
        let count = rules.len() as u64;
        self.notifier.lock().expect("通知决策锁").set_rules(rules);
        Ok(count)
    }

    /// 评估一轮告警（当前演示行情 + 当前时刻）：新触发入本地队列
    ///
    /// 载荷 `{fired:[NotificationSpec], pending:usize}`——`fired` 为本轮新触发，
    /// `pending` 为队列中待壳层取走的总条数（评估即入队，`take_pending_json` 取走）。
    /// 穿越去重语义见文档 §5。
    pub fn check_alerts_json(&self) -> String {
        let quotes = self.current_quotes();
        let fired_now = {
            let mut notifier = self.notifier.lock().expect("通知决策锁");
            notifier.evaluate_at(&quotes, chrono::Utc::now())
        };
        let pending = self
            .notifier
            .lock()
            .expect("通知决策锁")
            .channel()
            .pending();
        serde_json::json!({
            "fired": fired_now,
            "pending": pending,
        })
        .to_string()
    }

    /// 取走待送达通知（取走即空；壳层负责平台侧送达）
    ///
    /// 载荷 `{taken:[NotificationSpec]}`——壳层拿到后交
    /// NotificationManager / UNUserNotificationCenter 弹出。
    pub fn take_pending_json(&self) -> String {
        let taken = {
            let notifier = self.notifier.lock().expect("通知决策锁");
            notifier.channel().take_all()
        };
        serde_json::json!({ "taken": taken }).to_string()
    }

    /// 同步状态（`{last_sync,fingerprint,interval_secs,due}`，`due` 按 Periodic 口径）
    pub fn sync_status_json(&self) -> String {
        let status = self
            .sync
            .lock()
            .expect("同步决策锁")
            .status_at(chrono::Utc::now());
        serde_json::to_string(&status).unwrap_or_else(|_| "{}".to_string())
    }

    /// 出同步计划（入参 trigger 字符串：`periodic`/`foreground`/
    /// `connectivity_restored`/`manual`；未知值 → `Failed{detail}`）
    ///
    /// 载荷 `{due,trigger,symbols,since_fingerprint,interval_secs,reason?}`——
    /// `Periodic` 受间隔闸门（文档 §3），其余三种无条件 `due=true`。
    pub fn sync_plan_json(&self, trigger: String) -> Result<String, MobileError> {
        let parsed: SyncTrigger =
            serde_json::from_str(&format!("\"{trigger}\"")).map_err(|_| MobileError::Failed {
                detail: format!("未知同步触发: {trigger}"),
            })?;
        let plan = self.sync.lock().expect("同步决策锁").decide_at(
            parsed,
            chrono::Utc::now(),
            &self.symbols,
        );
        Self::to_json(&plan)
    }

    /// 标记已同步（壳层取数完成后调用）：记 `last_sync=now` + 以当前行情
    /// 重算指纹；返回更新后的状态（载荷同 `sync_status_json`）
    pub fn mark_synced_json(&self) -> String {
        let quotes = self.current_quotes();
        let status = self
            .sync
            .lock()
            .expect("同步决策锁")
            .mark_synced_at(chrono::Utc::now(), &quotes);
        serde_json::to_string(&status).unwrap_or_else(|_| "{}".to_string())
    }

    // ── L390 离线数据决策面（五方法，只增不改——docs/mobile-offline.md §4）──
    // 产品红线：授权开关默认关闭、显式开启须带数据范围、未授权不产生同步
    // 工作项（三条款在核心库强制，壳层绕不过）。

    /// 当前离线授权配置（`{enabled, scopes}`——UI 读取开关与数据范围，红线③明示面）
    pub fn offline_sync_config_json(&self) -> String {
        serde_json::to_string(self.offline.lock().expect("离线决策锁").config())
            .unwrap_or_else(|_| "{}".to_string())
    }

    /// 设置离线授权配置（**唯一开启路径**）：`enabled=true` 须同时携带非空
    /// `scopes`（红线②），未知 scope 字符串拒绝；成功返回生效配置回显
    pub fn set_offline_sync_config_json(&self, config_json: String) -> Result<String, MobileError> {
        let config: OfflineSyncConfig =
            serde_json::from_str(&config_json).map_err(|e| MobileError::Failed {
                detail: format!("离线配置解析失败（scopes 为封闭枚举）: {e}"),
            })?;
        let applied = self
            .offline
            .lock()
            .expect("离线决策锁")
            .set_config(config)
            .map_err(|detail| MobileError::Failed { detail })?;
        Self::to_json(&applied)
    }

    /// 生成本地快照（**备份面**：落盘供断网恢复，不出设备）。未开启 →
    /// `Failed`（红线①在核心库强制，文案引导显式开启）
    pub fn offline_snapshot_json(&self) -> Result<String, MobileError> {
        let quotes = self.current_quotes();
        let snapshot = self
            .offline
            .lock()
            .expect("离线决策锁")
            .snapshot_at(chrono::Utc::now(), &quotes, env!("CARGO_PKG_VERSION"))
            .map_err(|detail| MobileError::Failed { detail })?;
        Self::to_json(&snapshot)
    }

    /// 校验快照可恢复（授权 + 版本一致），返回待恢复条目数；落盘/读回由
    /// 壳层 KeyValueStore 执行（后写覆盖语义沿桌面 L116 口径）
    pub fn restore_offline_snapshot_json(&self, snapshot_json: String) -> Result<u64, MobileError> {
        let snapshot: OfflineSnapshot =
            serde_json::from_str(&snapshot_json).map_err(|e| MobileError::Failed {
                detail: format!("快照结构非法: {e}"),
            })?;
        let count = self
            .offline
            .lock()
            .expect("离线决策锁")
            .restore_validate(&snapshot, env!("CARGO_PKG_VERSION"))
            .map_err(|detail| MobileError::Failed { detail })?;
        Ok(count as u64)
    }

    /// 同步增量判断（**同步面**：与远端对齐是否需要）。未授权 →
    /// `needed=false, reason="sync_disabled"`（红线③：不产生外发工作项）；
    /// 已授权则纯指纹比较（L337 口径，相同 `unchanged` / 不同才需要对齐）
    pub fn offline_sync_delta_json(&self, since_fingerprint: u64) -> String {
        let quotes = self.current_quotes();
        let delta = self
            .offline
            .lock()
            .expect("离线决策锁")
            .delta_at(since_fingerprint, &quotes);
        serde_json::to_string(&delta).unwrap_or_else(|_| "{}".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn core() -> Arc<MobileCore> {
        MobileCore::new(
            vec!["600519".to_string(), "000001".to_string()],
            "http://localhost:8080".to_string(),
        )
    }

    #[test]
    fn new_stores_watch_list_and_api_url() {
        let core = core();
        assert_eq!(core.symbols(), ["600519", "000001"]);
        assert_eq!(core.api_url(), "http://localhost:8080");
    }

    #[test]
    fn quote_within_watch_list_is_deterministic() {
        let core = core();
        let a = core.quote("600519").expect("列表内");
        let b = core.quote("600519").expect("列表内");
        assert_eq!(a.price, b.price, "演示行情与调用时刻无关");
        assert_eq!(a.symbol, "600519");
    }

    #[test]
    fn quote_outside_watch_list_is_invalid_symbol() {
        let core = core();
        let err = core.quote("300750").expect_err("列表外应拒绝");
        match &err {
            MobileError::InvalidSymbol { symbol } => assert_eq!(symbol, "300750"),
            other => panic!("应为 InvalidSymbol，实际 {other:?}"),
        }
        // 错误文案带标的（平台壳展示即用）
        assert!(err.to_string().contains("300750"));
    }

    /// 空 symbol 与空观察列表的退化：一律 InvalidSymbol，不 panic 不编造
    #[test]
    fn empty_cases_degrade_to_invalid_symbol() {
        let empty = MobileCore::new(Vec::new(), "http://localhost:8080".to_string());
        assert!(matches!(
            empty.quote("600519"),
            Err(MobileError::InvalidSymbol { .. })
        ));
        assert!(matches!(
            core().quote(""),
            Err(MobileError::InvalidSymbol { .. })
        ));
    }

    #[test]
    fn analyze_within_watch_list_returns_indicators() {
        let core = core();
        let result = core.analyze("600519").expect("列表内");
        assert_eq!(result.symbol, "600519");
        assert!(!result.indicators.is_empty(), "应有指标（RSI/SMA/MACD）");
    }

    #[test]
    fn analyze_outside_watch_list_is_invalid_symbol() {
        let core = core();
        assert!(matches!(
            core.analyze("300750"),
            Err(MobileError::InvalidSymbol { .. })
        ));
    }

    /// FFI 载荷字段契约：quote_json = MarketData serde 字段（§4）
    #[test]
    fn quote_json_keeps_market_data_field_contract() {
        let core = core();
        let value: serde_json::Value =
            serde_json::from_str(&core.quote_json("600519".to_string()).expect("JSON"))
                .expect("解析");
        for key in [
            "symbol",
            "timestamp",
            "price",
            "volume",
            "bid",
            "ask",
            "open",
        ] {
            assert!(value.get(key).is_some(), "quote_json 缺字段 {key}: {value}");
        }
        assert_eq!(value["symbol"], "600519");
    }

    /// FFI 载荷字段契约：analyze_json = AnalysisResult serde 字段（§4）
    #[test]
    fn analyze_json_keeps_analysis_result_field_contract() {
        let core = core();
        let value: serde_json::Value =
            serde_json::from_str(&core.analyze_json("600519".to_string()).expect("JSON"))
                .expect("解析");
        for key in [
            "symbol",
            "indicators",
            "recommendation",
            "confidence",
            "risk_metrics",
        ] {
            assert!(
                value.get(key).is_some(),
                "analyze_json 缺字段 {key}: {value}"
            );
        }
        let indicators = value["indicators"].as_array().expect("指标数组");
        assert!(!indicators.is_empty());
        assert!(indicators[0].get("name").is_some() && indicators[0].get("values").is_some());
    }

    #[test]
    fn status_json_reports_config_slot() {
        let core = core();
        let value: serde_json::Value =
            serde_json::from_str(&core.status_json()).expect("解析为 JSON");
        assert_eq!(value["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(value["api_url"], "http://localhost:8080");
        assert_eq!(value["symbols"], serde_json::json!(["600519", "000001"]));
    }

    /// 列表外标的在 FFI 侧同样抛 InvalidSymbol（不因走 JSON 方法而绕过边界）
    #[test]
    fn json_methods_enforce_watch_list_too() {
        let core = core();
        assert!(matches!(
            core.quote_json("300750".to_string()),
            Err(MobileError::InvalidSymbol { .. })
        ));
        assert!(matches!(
            core.analyze_json("300750".to_string()),
            Err(MobileError::InvalidSymbol { .. })
        ));
    }

    /// From<AlphaError>：分类不变、Display 全文进 Failed.detail（§5）
    #[test]
    fn alpha_error_maps_to_failed_with_display_text() {
        let source = AlphaError::invalid_input("bad input");
        let mapped: MobileError = source.clone().into();
        assert_eq!(mapped.to_string(), source.to_string());
        assert!(matches!(mapped, MobileError::Failed { .. }));
    }

    // ── L337 FFI 决策面（规则校验 / 触发-取走回路 / 同步三件套）──

    fn rules_json(rules: serde_json::Value) -> String {
        rules.to_string()
    }

    /// 规则校验：列表外 → InvalidSymbol；非法目标价/坏 JSON → Failed
    #[test]
    fn alert_rules_validate_watch_list_target_and_json() {
        let core = core();
        assert!(matches!(
            core.set_alert_rules_json(rules_json(serde_json::json!([
                {"symbol": "300750", "target_price": 90.0, "above": true}
            ]))),
            Err(MobileError::InvalidSymbol { .. })
        ));
        for bad_target in [0.0, -1.0, f64::NAN] {
            assert!(
                matches!(
                    core.set_alert_rules_json(rules_json(serde_json::json!([
                        {"symbol": "600519", "target_price": bad_target, "above": true}
                    ]))),
                    Err(MobileError::Failed { .. })
                ),
                "目标价 {bad_target} 应拒绝"
            );
        }
        assert!(matches!(
            core.set_alert_rules_json("not json".to_string()),
            Err(MobileError::Failed { .. })
        ));
        assert!(
            core.notifier.lock().expect("锁").rules().is_empty(),
            "被拒的设置不改既有规则"
        );
    }

    /// 触发→取走回路：评估即入队；同穿越不重发（去重跨 FFI 调用成立）；
    /// 取走即空
    #[test]
    fn check_take_round_trip_dedups_and_drains() {
        let core = core();
        let price = core.quote("600519").expect("列表内").price;
        let count = core
            .set_alert_rules_json(rules_json(serde_json::json!([
                {"symbol": "600519", "target_price": price - 1.0, "above": true}
            ])))
            .expect("合法规则");
        assert_eq!(count, 1);

        let first: serde_json::Value =
            serde_json::from_str(&core.check_alerts_json()).expect("解析");
        assert_eq!(
            first["fired"].as_array().expect("数组").len(),
            1,
            "上穿触发"
        );
        assert_eq!(first["pending"], 1, "评估即入队");

        let second: serde_json::Value =
            serde_json::from_str(&core.check_alerts_json()).expect("解析");
        assert!(
            second["fired"].as_array().expect("数组").is_empty(),
            "仍成立不重发（文档 §5）"
        );
        assert_eq!(second["pending"], 1, "未取走仍在队列");

        let taken: serde_json::Value =
            serde_json::from_str(&core.take_pending_json()).expect("解析");
        let taken = taken["taken"].as_array().expect("数组");
        assert_eq!(taken.len(), 1);
        assert_eq!(taken[0]["kind"], "PriceAlert");
        assert_eq!(taken[0]["symbol"], "600519");
        for key in ["id", "title", "body", "created_at"] {
            assert!(taken[0].get(key).is_some(), "载荷缺 {key}");
        }

        assert!(
            core.notifier.lock().expect("锁").channel().pending() == 0,
            "取走即空"
        );
        let again: serde_json::Value =
            serde_json::from_str(&core.check_alerts_json()).expect("解析");
        assert!(
            again["fired"].as_array().expect("数组").is_empty(),
            "已发不重发"
        );
    }

    /// 条件回落再穿越 → 经 FFI 再发（重装语义跨桥成立）
    #[test]
    fn rearm_semantics_flow_through_ffi() {
        let core = core();
        let price = core.quote("600519").expect("列表内").price;
        let above = rules_json(serde_json::json!([
            {"symbol": "600519", "target_price": price - 1.0, "above": true}
        ]));
        core.set_alert_rules_json(above.clone()).expect("规则");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&core.check_alerts_json()).expect("解析")
                ["fired"]
                .as_array()
                .expect("数组")
                .len(),
            1
        );
        // 换成不可能触发的规则（回落）→ 重武装；再设回 → 再发
        core.set_alert_rules_json(rules_json(serde_json::json!([
            {"symbol": "600519", "target_price": price + 100.0, "above": true}
        ])))
        .expect("规则");
        assert!(
            serde_json::from_str::<serde_json::Value>(&core.check_alerts_json()).expect("解析")
                ["fired"]
                .as_array()
                .expect("数组")
                .is_empty(),
            "回落评估不发"
        );
        core.set_alert_rules_json(above).expect("规则");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&core.check_alerts_json()).expect("解析")
                ["fired"]
                .as_array()
                .expect("数组")
                .len(),
            1,
            "规则重设后重新武装"
        );
    }

    /// 同步三件套：初始 Periodic 立即到期 → mark 后受闸门 → 手动不受闸门；
    /// 未知触发字符串走 Failed
    #[test]
    fn sync_plan_gating_and_unknown_trigger_flow_through_ffi() {
        let core = core();
        let initial: serde_json::Value =
            serde_json::from_str(&core.sync_plan_json("periodic".into()).expect("计划"))
                .expect("解析");
        assert_eq!(initial["due"], true, "从未同步立即到期");
        assert_eq!(initial["since_fingerprint"], 0);
        assert_eq!(initial["symbols"], serde_json::json!(["600519", "000001"]));

        let marked: serde_json::Value =
            serde_json::from_str(&core.mark_synced_json()).expect("解析");
        assert_ne!(marked["fingerprint"], 0, "指纹已重算");
        assert_eq!(marked["due"], false, "刚同步不 due");
        assert!(!marked["last_sync"].is_null(), "last_sync 已记");

        let gated: serde_json::Value =
            serde_json::from_str(&core.sync_plan_json("periodic".into()).expect("计划"))
                .expect("解析");
        assert_eq!(gated["due"], false, "间隔闸门生效");
        assert_eq!(gated["reason"], "interval_not_elapsed");
        assert!(gated["symbols"].as_array().expect("数组").is_empty());

        let manual: serde_json::Value =
            serde_json::from_str(&core.sync_plan_json("manual".into()).expect("计划"))
                .expect("解析");
        assert_eq!(manual["due"], true, "手动刷新不受闸门");

        let err = core
            .sync_plan_json("nope".into())
            .expect_err("未知触发应拒绝");
        assert!(matches!(err, MobileError::Failed { .. }));
        assert!(err.to_string().contains("未知同步触发"), "文案: {err}");
    }

    /// status 载荷契约：四字段；从未同步 last_sync=null、指纹 0
    #[test]
    fn sync_status_json_keeps_payload_contract() {
        let core = core();
        let value: serde_json::Value =
            serde_json::from_str(&core.sync_status_json()).expect("解析");
        for key in ["last_sync", "fingerprint", "interval_secs", "due"] {
            assert!(value.get(key).is_some(), "缺字段 {key}: {value}");
        }
        assert!(value["last_sync"].is_null(), "从未同步 last_sync 为 null");
        assert_eq!(value["fingerprint"], 0);
        assert_eq!(value["interval_secs"], crate::sync::DEFAULT_INTERVAL_SECS);
        assert_eq!(value["due"], true, "从未同步 Periodic 立即到期");
    }

    /// 规则整体替换 + 返回条数（经 FFI 语义同 `Notifier::set_rules`）
    #[test]
    fn set_alert_rules_returns_count_and_replaces() {
        let core = core();
        let count = core
            .set_alert_rules_json(rules_json(serde_json::json!([
                {"symbol": "600519", "target_price": 100.0, "above": true},
                {"symbol": "000001", "target_price": 9.0, "above": false}
            ])))
            .expect("合法规则");
        assert_eq!(count, 2);
        assert_eq!(core.notifier.lock().expect("锁").rules().len(), 2);
        // 整体替换：后写覆盖前写
        core.set_alert_rules_json(rules_json(serde_json::json!([
            {"symbol": "600519", "target_price": 100.0, "above": true}
        ])))
        .expect("合法规则");
        assert_eq!(core.notifier.lock().expect("锁").rules().len(), 1);
    }

    // ── L390 离线数据 FFI（红线在核心库强制的端到端面）──

    /// 红线①：默认关闭可读、未开启快照拒绝且文案引导显式开启
    #[test]
    fn offline_defaults_disabled_and_blocks_snapshot_via_ffi() {
        let core = core();
        let config: serde_json::Value =
            serde_json::from_str(&core.offline_sync_config_json()).expect("解析");
        assert_eq!(config["enabled"], false, "默认关闭");
        assert_eq!(config["scopes"], serde_json::json!([]));

        let err = core.offline_snapshot_json().expect_err("未开启应拒绝");
        assert!(matches!(err, MobileError::Failed { .. }));
        assert!(err.to_string().contains("未开启"), "文案: {err}");
        assert!(
            err.to_string().contains("显式开启"),
            "文案须引导显式开启: {err}"
        );
    }

    /// 红线②③：开启须带范围；开启后快照/恢复/增量全链路走通
    #[test]
    fn offline_enable_snapshot_restore_via_ffi() {
        let core = core();
        // 无范围开启 → 拒绝（文案含 scopes）
        let err = core
            .set_offline_sync_config_json(r#"{"enabled":true,"scopes":[]}"#.to_string())
            .expect_err("无范围开启应拒绝");
        assert!(err.to_string().contains("scopes"), "文案: {err}");

        // 未知 scope → 拒绝（封闭枚举）
        assert!(core
            .set_offline_sync_config_json(r#"{"enabled":true,"scopes":["contacts"]}"#.to_string())
            .is_err());

        // 合法开启 → 生效配置回显（明示面）
        let applied: serde_json::Value = serde_json::from_str(
            &core
                .set_offline_sync_config_json(r#"{"enabled":true,"scopes":["quotes"]}"#.to_string())
                .expect("合法配置"),
        )
        .expect("解析");
        assert_eq!(applied["enabled"], true);
        assert_eq!(applied["scopes"], serde_json::json!(["quotes"]));

        // 备份面：快照含观察列表全量条目 + 版本
        let snapshot: serde_json::Value =
            serde_json::from_str(&core.offline_snapshot_json().expect("已授权")).expect("解析");
        assert_eq!(snapshot["entries"].as_array().expect("数组").len(), 2);
        assert_eq!(snapshot["version"], env!("CARGO_PKG_VERSION"));
        let snapshot_json = snapshot.to_string();

        // 恢复：结构非法/一致各走各路；篡改版本拒绝
        assert!(core
            .restore_offline_snapshot_json("not json".to_string())
            .is_err());
        let mut tampered = snapshot.clone();
        tampered["version"] = serde_json::json!("9.9.9");
        assert!(
            core.restore_offline_snapshot_json(tampered.to_string())
                .is_err(),
            "版本不匹配拒绝"
        );
        let count = core
            .restore_offline_snapshot_json(snapshot_json)
            .expect("一致");
        assert_eq!(count, 2, "返回待恢复条目数");
    }

    /// 同步面增量：未授权 sync_disabled；授权后指纹比较（unchanged/needed）
    #[test]
    fn offline_delta_gates_on_authorization_and_fingerprint_via_ffi() {
        let core = core();
        let disabled: serde_json::Value =
            serde_json::from_str(&core.offline_sync_delta_json(0)).expect("解析");
        assert_eq!(disabled["needed"], false, "红线③：未授权不产生工作项");
        assert_eq!(disabled["reason"], "sync_disabled");

        core.set_offline_sync_config_json(r#"{"enabled":true,"scopes":["quotes"]}"#.to_string())
            .expect("开启");
        let snapshot: serde_json::Value =
            serde_json::from_str(&core.offline_snapshot_json().expect("快照")).expect("解析");
        let fingerprint = snapshot["fingerprint"].as_u64().expect("指纹");

        let unchanged: serde_json::Value =
            serde_json::from_str(&core.offline_sync_delta_json(fingerprint)).expect("解析");
        assert_eq!(unchanged["needed"], false, "指纹相同无需对齐");
        assert_eq!(unchanged["reason"], "unchanged");

        let needed: serde_json::Value =
            serde_json::from_str(&core.offline_sync_delta_json(0)).expect("解析");
        assert_eq!(needed["needed"], true, "指纹变化需对齐");
        assert!(needed.get("reason").is_none());
        assert_eq!(needed["current_fingerprint"], fingerprint);
    }
}
