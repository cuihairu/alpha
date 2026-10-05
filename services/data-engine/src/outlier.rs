//! 数据质量：单跳价格异常检测（architecture-review §5 P2「异常」维度）
//!
//! 每 symbol 一条价格基线，单 tick 相对上一成交价的涨跌幅超阈即判定
//! 异常（`Jump`）——A 股单日涨跌停上限 ±10%（主板）/ ±20%（科创/创业），
//! 单 tick 跨更大跳变只可能来自行情源污染（分/元单位混用、错位填充、
//! 截断数据），不是正常价格行为。缺省阈值 30%（跨出两大板涨跌停带）。
//!
//! 观察语义（每 symbol 一条基线）：
//! - **FirstSeen**：首见记基线，不告警（无从判跳变）
//! - **Within**：|涨跌幅| ≤ 阈值，更新基线
//! - **Jump**：单跳超阈 → 更新基线并返回详情——每条毒值只相对前值告警
//!   一次，不让一个卡死的坏价在后续每条消息上反复触发（回到正常价的
//!   反向跳变同样计一次，属预期形态）
//!
//! 状态有界：`TRACK_EVICT` 上限 + `TRACK_IDLE_MS` 惰性逐出（防 symbol
//! 面无限增长撑爆内存，同网关 shield 护栏口径）；非正价格不改基线
//! （调用方 `normalize_quote` 已挡，此处防御性保留）。
//!
//! 本模块只做判定（纯状态机可单测）；metrics/tracing 告警面由调用方在
//! `process_normalizer_message` 组装，与 `sequence_gap.rs` 同构。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// 单跳异常的详情
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PriceJump {
    /// 上一基线价
    pub last: f64,
    /// 本次价格
    pub got: f64,
    /// 涨跌幅百分比（带符号：正=涨、负=跌）
    pub pct: f64,
}

/// 观测结果
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OutlierObservation {
    /// 该 symbol 基线首次建立
    FirstSeen,
    /// 单跳在阈值内
    Within,
    /// 单跳超阈（异常）
    Jump(PriceJump),
}

/// 基线上限与闲置逐出（同网关 audit/shield 护栏口径）
const TRACK_EVICT: usize = 10_000;
const TRACK_IDLE_MS: i64 = 60_000;
/// 缺省阈值（%）：跨出 A 股两大板涨跌停带，只可能是源侧污染
const DEFAULT_THRESHOLD_PCT: f64 = 30.0;

/// per-symbol 价格基线；last=None 表示尚未建立
#[derive(Debug, Default)]
struct PriceBaseline {
    last: Option<f64>,
    last_ms: i64,
}

/// 线程安全的价格异常观察器
#[derive(Clone)]
pub struct PriceOutlierMonitor {
    threshold_pct: f64,
    baselines: Arc<Mutex<HashMap<String, PriceBaseline>>>,
}

impl PriceOutlierMonitor {
    /// 按给定阈值（%）构造；非正值回退缺省——配置面永不拒绝启动
    pub fn new(threshold_pct: f64) -> Self {
        Self {
            threshold_pct: if threshold_pct > 0.0 {
                threshold_pct
            } else {
                DEFAULT_THRESHOLD_PCT
            },
            baselines: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// env 装配：`ALPHA_DATAQUALITY_OUTLIER_PCT`，缺省/非法回退 30%
    pub fn from_env() -> Self {
        let threshold = std::env::var("ALPHA_DATAQUALITY_OUTLIER_PCT")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(DEFAULT_THRESHOLD_PCT);
        Self::new(threshold)
    }

    /// 观察一条价格；返回判定结果。基线更新规则见模块文档（Jump 也更新）。
    pub fn observe(&self, symbol: &str, price: f64, now_ms: i64) -> OutlierObservation {
        if !price.is_finite() || price <= 0.0 {
            // 调用方 normalize 已挡非正价；此处防御性不改基线
            return OutlierObservation::Within;
        }
        let mut baselines = self
            .baselines
            .lock()
            .expect("price baseline mutex poisoned");
        if baselines.len() > TRACK_EVICT {
            baselines.retain(|_, b| now_ms - b.last_ms < TRACK_IDLE_MS);
        }
        let baseline = baselines.entry(symbol.to_string()).or_default();

        let Some(last) = baseline.last else {
            baseline.last = Some(price);
            baseline.last_ms = now_ms;
            return OutlierObservation::FirstSeen;
        };

        baseline.last = Some(price);
        baseline.last_ms = now_ms;

        let pct = (price - last) / last * 100.0;
        if pct.abs() > self.threshold_pct {
            OutlierObservation::Jump(PriceJump {
                last,
                got: price,
                pct,
            })
        } else {
            OutlierObservation::Within
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_seen_then_within_then_jump() {
        let monitor = PriceOutlierMonitor::new(30.0);
        // 首见记基线，不告警
        assert_eq!(
            monitor.observe("600519", 100.0, 1),
            OutlierObservation::FirstSeen
        );
        // 阈值内（+10%，涨跌停带内）
        assert_eq!(
            monitor.observe("600519", 110.0, 2),
            OutlierObservation::Within
        );
        // 超阈 +45.5% → Jump，带详情
        match monitor.observe("600519", 160.0, 3) {
            OutlierObservation::Jump(jump) => {
                assert_eq!(jump.last, 110.0);
                assert_eq!(jump.got, 160.0);
                assert!((jump.pct - 45.454545).abs() < 1e-4);
            }
            other => panic!("应为 Jump，得到 {other:?}"),
        }
        // Jump 也更新基线：回落到 160 同价为 Within（毒值不反复触发）
        assert_eq!(
            monitor.observe("600519", 160.0, 4),
            OutlierObservation::Within
        );
        // 反向跳变（跌）同样计一次
        assert!(matches!(
            monitor.observe("600519", 60.0, 5),
            OutlierObservation::Jump(PriceJump { pct, .. }) if pct < 0.0
        ));
    }

    #[test]
    fn threshold_boundary_is_strict() {
        let monitor = PriceOutlierMonitor::new(30.0);
        // 每次 observe 都推进基线，两个判定各用独立 symbol 从 100 起跳
        monitor.observe("exact", 100.0, 1);
        // 恰好 30% 不算超阈（> 而非 >=）
        assert_eq!(
            monitor.observe("exact", 130.0, 2),
            OutlierObservation::Within
        );
        monitor.observe("over", 100.0, 1);
        // 超一点（30.1%）即 Jump
        assert!(matches!(
            monitor.observe("over", 130.1, 2),
            OutlierObservation::Jump(_)
        ));
    }

    #[test]
    fn symbols_have_independent_baselines() {
        let monitor = PriceOutlierMonitor::new(30.0);
        monitor.observe("a", 100.0, 1);
        monitor.observe("b", 100.0, 1);
        // b 首建后直接大跳不牵连 a
        assert!(matches!(
            monitor.observe("b", 500.0, 2),
            OutlierObservation::Jump(_)
        ));
        assert_eq!(monitor.observe("a", 105.0, 2), OutlierObservation::Within);
    }

    #[test]
    fn non_positive_price_does_not_change_baseline() {
        let monitor = PriceOutlierMonitor::new(30.0);
        assert_eq!(
            monitor.observe("a", 100.0, 1),
            OutlierObservation::FirstSeen
        );
        // 非正价防御性跳过（不建基线、不告警）
        assert_eq!(monitor.observe("a", 0.0, 2), OutlierObservation::Within);
        // 基线仍是 100：从 0 恢复的正常价不算跳变
        assert_eq!(monitor.observe("a", 101.0, 3), OutlierObservation::Within);
    }

    #[test]
    fn eviction_drops_idle_symbols_and_rebuilds_baseline() {
        let monitor = PriceOutlierMonitor::new(30.0);
        monitor.observe("old", 100.0, 1);
        // 撑过上限：新 symbol 触发逐出，60s 闲置的 old 被清
        for i in 0..TRACK_EVICT {
            monitor.observe(&format!("s{i}"), 100.0, 2);
        }
        // 逐出后 old 重新建立基线（FirstSeen 而非拿旧基线比跳变）
        assert_eq!(
            monitor.observe("old", 999.0, 1 + TRACK_IDLE_MS + 1),
            OutlierObservation::FirstSeen
        );
    }

    #[test]
    fn env_and_constructor_fallback() {
        // 非法阈值回退缺省（配置面永不拒绝启动）
        assert_eq!(
            PriceOutlierMonitor::new(-1.0).threshold_pct,
            DEFAULT_THRESHOLD_PCT
        );
        assert_eq!(DEFAULT_THRESHOLD_PCT, 30.0);
    }
}
