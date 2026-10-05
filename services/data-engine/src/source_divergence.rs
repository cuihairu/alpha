//! 数据质量：多源价格背离检测（architecture-review §5 P2「Source Divergence」）
//!
//! 同 symbol 有两个及以上数据源在报时，比较同刻价格：相对差超容差即
//! 判背离（行情源污染/延迟/错价信号——两源各自独立抓取，正常盘中差应
//! 在容差内）。只有单一源在报的 symbol 无从比较，记基线不算背离。
//!
//! 观察语义（per-symbol 收各源最新价，按**事件时间**对齐）：
//! - **FirstSeen**：该 (symbol, source) 首见，记基线不告警
//! - **Agree**：有同刻兄弟源且差 ≤ 容差（或无兄弟源/兄弟源过期——过期
//!   不比较，隔代的盘中价差属正常波动，不是背离）
//! - **Diverge**：与任一同刻兄弟源差 > 容差 → 返回对端明细，由调用方打
//!   告警面（metrics + tracing，symbol 不进指标标签防无界基数）
//!
//! 时效窗口 `STALE_MS`（2 分钟）：兄弟源事件时间差超过即放弃比较——
//! 拿 5 分钟前的报价判「背离」只会制造误报。
//!
//! 状态有界：symbol 数上限 + 闲置逐出（同 outlier/网关护栏口径）。本模块
//! 只做判定（纯状态机可单测），metrics/tracing 由调用方在
//! `process_normalizer_message` 组装，与 `sequence_gap.rs`/`outlier.rs` 同构。
//!
//! 现状（诚实登记）：当前行情面单一源在报（collector 侧 EastMoney/Sina
//! 双实现已有但未并发同标的），本检测器挂上即位、单源期间恒
//! FirstSeen/Agree；第二源并发接入即自然生效。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// 兄弟源事件时间对齐窗口：超此差放弃比较（隔代盘中价差是正常波动）
const STALE_MS: i64 = 120_000;
/// symbol 数上限与闲置逐出（同 outlier 口径）
const TRACK_EVICT: usize = 10_000;
const TRACK_IDLE_MS: i64 = 60_000;
/// 缺省容差（%）：两独立源同刻报价的正常抓取差
const DEFAULT_TOLERANCE_PCT: f64 = 1.0;

/// 一次背离的详情（对端 = 被比较的兄弟源）
#[derive(Debug, Clone, PartialEq)]
pub struct Divergence {
    /// 本侧源
    pub source: String,
    /// 对端（背离的兄弟源）
    pub other_source: String,
    /// 本侧价
    pub price: f64,
    /// 对端价
    pub other_price: f64,
    /// 相对差百分比（带符号：正=本侧高）
    pub pct: f64,
}

/// 观测结果
#[derive(Debug, Clone, PartialEq)]
pub enum DivergenceObservation {
    /// 该 (symbol, source) 首见，记基线
    FirstSeen,
    /// 无背离（同刻兄弟源在容差内，或无/无新鲜兄弟源）
    Agree,
    /// 与同刻兄弟源超容差
    Diverge(Divergence),
}

/// per-symbol 各源最新价（价格 + 事件时间）
#[derive(Debug, Default, Clone)]
struct SymbolQuotes {
    /// (source → (price, event_ms))
    by_source: HashMap<String, (f64, i64)>,
    last_ms: i64,
}

/// 线程安全的多源背离观察器
#[derive(Clone)]
pub struct SourceDivergenceMonitor {
    tolerance_pct: f64,
    quotes: Arc<Mutex<HashMap<String, SymbolQuotes>>>,
}

impl SourceDivergenceMonitor {
    /// 按给定容差（%）构造；非正值回退缺省——配置面永不拒绝启动
    pub fn new(tolerance_pct: f64) -> Self {
        Self {
            tolerance_pct: if tolerance_pct > 0.0 {
                tolerance_pct
            } else {
                DEFAULT_TOLERANCE_PCT
            },
            quotes: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// env 装配：`ALPHA_DATAQUALITY_DIVERGENCE_PCT`，缺省/非法回退 1%
    pub fn from_env() -> Self {
        let tolerance = std::env::var("ALPHA_DATAQUALITY_DIVERGENCE_PCT")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(DEFAULT_TOLERANCE_PCT);
        Self::new(tolerance)
    }

    /// 观察一条报价：记录本侧最新价，与同刻（事件时间差 ≤ STALE_MS）
    /// 兄弟源逐一比较，任一超容差即 Diverge。
    pub fn observe(
        &self,
        symbol: &str,
        source: &str,
        price: f64,
        event_ms: i64,
        now_ms: i64,
    ) -> DivergenceObservation {
        if !price.is_finite() || price <= 0.0 {
            // 调用方 normalize 已挡；防御性不记不比
            return DivergenceObservation::Agree;
        }
        let mut quotes = self.quotes.lock().expect("divergence state mutex poisoned");
        if quotes.len() > TRACK_EVICT {
            quotes.retain(|_, q| now_ms - q.last_ms < TRACK_IDLE_MS);
        }
        let symbol_quotes = quotes.entry(symbol.to_string()).or_default();
        symbol_quotes.last_ms = now_ms;

        let first_seen = !symbol_quotes.by_source.contains_key(source);
        // 先取同刻兄弟快照（含过期过滤），再写入本侧——自比不产生背离
        let siblings: Vec<(String, f64)> = symbol_quotes
            .by_source
            .iter()
            .filter(|(src, (_, s_ms))| *src != source && (event_ms - *s_ms).abs() <= STALE_MS)
            .map(|(src, (p, _))| (src.clone(), *p))
            .collect();
        symbol_quotes
            .by_source
            .insert(source.to_string(), (price, event_ms));

        if first_seen {
            return DivergenceObservation::FirstSeen;
        }

        for (other_source, other_price) in siblings {
            let pct = (price - other_price) / other_price * 100.0;
            if pct.abs() > self.tolerance_pct {
                return DivergenceObservation::Diverge(Divergence {
                    source: source.to_string(),
                    other_source,
                    price,
                    other_price,
                    pct,
                });
            }
        }
        DivergenceObservation::Agree
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_source_first_seen_then_single_source_agrees() {
        let monitor = SourceDivergenceMonitor::new(1.0);
        assert_eq!(
            monitor.observe("600519", "eastmoney", 100.0, 1_000, 1_000),
            DivergenceObservation::FirstSeen
        );
        // 单源无从比较
        assert_eq!(
            monitor.observe("600519", "eastmoney", 101.0, 2_000, 2_000),
            DivergenceObservation::Agree
        );
    }

    #[test]
    fn two_sources_within_tolerance_agree() {
        let monitor = SourceDivergenceMonitor::new(1.0);
        monitor.observe("600519", "eastmoney", 100.0, 1_000, 1_000);
        // 第二源首见仍 FirstSeen（先建基线，下一条再比）
        assert_eq!(
            monitor.observe("600519", "sina", 100.5, 1_100, 1_100),
            DivergenceObservation::FirstSeen
        );
        // 同刻差 0.5% ≤ 1% → Agree
        assert_eq!(
            monitor.observe("600519", "sina", 101.0, 1_200, 1_200),
            DivergenceObservation::Agree
        );
    }

    #[test]
    fn two_sources_diverge_beyond_tolerance() {
        let monitor = SourceDivergenceMonitor::new(1.0);
        monitor.observe("600519", "eastmoney", 100.0, 1_000, 1_000);
        monitor.observe("600519", "sina", 100.0, 1_100, 1_100);
        // 同刻差 5% > 1% → Diverge，明细带对端
        match monitor.observe("600519", "eastmoney", 105.0, 1_200, 1_200) {
            DivergenceObservation::Diverge(d) => {
                assert_eq!(d.source, "eastmoney");
                assert_eq!(d.other_source, "sina");
                assert_eq!(d.price, 105.0);
                assert_eq!(d.other_price, 100.0);
                assert!((d.pct - 5.0).abs() < 1e-9);
            }
            other => panic!("应为 Diverge，得到 {other:?}"),
        }
    }

    #[test]
    fn stale_sibling_is_not_compared() {
        let monitor = SourceDivergenceMonitor::new(1.0);
        monitor.observe("600519", "eastmoney", 100.0, 1_000, 1_000);
        // 兄弟源事件时间落后超 STALE_MS：放弃比较（隔代盘中差是正常波动）
        assert_eq!(
            monitor.observe(
                "600519",
                "sina",
                120.0,
                1_000 + STALE_MS + 1,
                1_000 + STALE_MS + 1
            ),
            DivergenceObservation::FirstSeen
        );
        // 本侧（sina）新价 vs eastmoney 同刻快照：eastmoney 已过期 → Agree
        assert_eq!(
            monitor.observe(
                "600519",
                "sina",
                150.0,
                1_000 + STALE_MS + 2,
                1_000 + STALE_MS + 2
            ),
            DivergenceObservation::Agree
        );
    }

    #[test]
    fn symbols_have_independent_state() {
        let monitor = SourceDivergenceMonitor::new(1.0);
        monitor.observe("a", "s1", 100.0, 1_000, 1_000);
        monitor.observe("b", "s1", 100.0, 1_000, 1_000);
        // b 侧第二源背离不牵连 a
        monitor.observe("b", "s2", 100.0, 1_100, 1_100);
        assert!(matches!(
            monitor.observe("b", "s1", 110.0, 1_200, 1_200),
            DivergenceObservation::Diverge(_)
        ));
        assert_eq!(
            monitor.observe("a", "s2", 100.5, 1_100, 1_100),
            DivergenceObservation::FirstSeen
        );
    }

    #[test]
    fn non_positive_price_is_defensive() {
        let monitor = SourceDivergenceMonitor::new(1.0);
        // 非正/非有限价：不记不比
        assert_eq!(
            monitor.observe("a", "s1", 0.0, 1_000, 1_000),
            DivergenceObservation::Agree
        );
        assert_eq!(
            monitor.observe("a", "s1", f64::NAN, 1_001, 1_001),
            DivergenceObservation::Agree
        );
        // 基线未被污染：正常首见
        assert_eq!(
            monitor.observe("a", "s1", 100.0, 1_002, 1_002),
            DivergenceObservation::FirstSeen
        );
    }

    #[test]
    fn invalid_tolerance_falls_back_to_default() {
        assert_eq!(
            SourceDivergenceMonitor::new(0.0).tolerance_pct,
            DEFAULT_TOLERANCE_PCT
        );
        assert_eq!(DEFAULT_TOLERANCE_PCT, 1.0);
    }
}
