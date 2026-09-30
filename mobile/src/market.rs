//! 确定性演示行情（移动端，与 `desktop/src/market.rs` 同口径）
//!
//! 骨架阶段尚无真实数据源（接入 api-gateway 属后续 TODO），但演示数据不能是
//! `rand` + `Utc::now()`：那样同一标的每次调用都不一样，单测与两端演示都无法
//! 断言。改为 **种子由 symbol 派生的 LCG 伪随机游走 + 可注入结束时刻**——demo、
//! 单测、FFI 载荷三者同源。价格与时间无关（只有 timestamp 随 `at` 变）。
//!
//! 真实数据接入时只替换本模块的取数实现，`MobileCore` 与 FFI 契约不动。
//! 与桌面端暂为两处同算法实现（两端生命周期不同，过早上提到 alpha-core 会把
//! 占位口径焊进共享层；真实数据源落地时一并收敛——见 docs/
//! mobile-core-architecture.md §8）。

use alpha_core::models::MarketData;
use chrono::{DateTime, Duration, Utc};

/// 默认 K 线根数（足够 alpha-core 的 SMA(50)/RSI 产出有效值）
pub const DEFAULT_BARS: usize = 100;
/// K 线间隔：1 分钟
pub const BAR_INTERVAL_MINUTES: i64 = 1;

/// symbol → 种子（FNV-1a 64 位，确定性、无依赖）
fn seed_for(symbol: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in symbol.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    hash
}

/// 线性同余伪随机（同一 seed 出同一序列）
struct Lcg {
    state: u64,
}

impl Lcg {
    fn new(seed: u64) -> Self {
        Self {
            state: seed.wrapping_add(1),
        }
    }

    /// [0, 1) 均匀分布
    fn next_unit(&mut self) -> f64 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.state >> 33) as f64) / (u32::MAX as f64)
    }

    /// [0, n) 整数
    fn next_below(&mut self, n: u64) -> u64 {
        (self.next_unit() * n as f64) as u64
    }
}

/// 生成序列，末根 bar 的时间戳为 `end`（向前每根一个 [`BAR_INTERVAL_MINUTES`]）
pub fn synthetic_series_at(symbol: &str, bars: usize, end: DateTime<Utc>) -> Vec<MarketData> {
    if bars == 0 {
        return Vec::new();
    }
    let mut rng = Lcg::new(seed_for(symbol));
    // 起点价由 symbol 决定（不同标的不同量级，便于演示时区分）
    let base_price = 50.0 + (seed_for(symbol) % 45_000) as f64 / 100.0;
    let mut price = base_price;
    let mut series = Vec::with_capacity(bars);

    for i in 0..bars {
        // 步进 ±1%（几何游走，始终为正）
        let drift = (rng.next_unit() - 0.5) * 0.02;
        price = (price * (1.0 + drift)).max(0.01);
        let spread = (price * 0.001).max(0.01);
        series.push(MarketData {
            symbol: symbol.to_string(),
            timestamp: end - Duration::minutes(BAR_INTERVAL_MINUTES * (bars - 1 - i) as i64),
            price,
            volume: 1_000 + rng.next_below(99_000),
            bid: Some((price - spread).max(0.0)),
            ask: Some(price + spread),
            open: Some((price - spread).max(0.0)),
            high: Some(price + spread * 2.0),
            low: Some((price - spread * 2.0).max(0.0)),
        });
    }
    series
}

/// 生成序列，末根 bar 为当前时刻
pub fn synthetic_series(symbol: &str, bars: usize) -> Vec<MarketData> {
    synthetic_series_at(symbol, bars, Utc::now())
}

/// 单个快照行情（**与序列末根逐 draw 同序**：每根先 drift 后 volume 的
/// 抽取顺序与 [`synthetic_series_at`] 完全一致，故快照 ≡ 末根 bar，同符号
/// 同序列下价格/成交量可跨接口断言——单测锁定此性质）
pub fn synthetic_quote_at(symbol: &str, at: DateTime<Utc>) -> MarketData {
    let mut rng = Lcg::new(seed_for(symbol));
    let base_price = 50.0 + (seed_for(symbol) % 45_000) as f64 / 100.0;
    let mut price = base_price;
    let mut volume = 1_000u64;
    for _ in 0..DEFAULT_BARS {
        price = (price * (1.0 + (rng.next_unit() - 0.5) * 0.02)).max(0.01);
        volume = 1_000 + rng.next_below(99_000);
    }
    let spread = (price * 0.001).max(0.01);
    MarketData {
        symbol: symbol.to_string(),
        timestamp: at,
        price,
        volume,
        bid: Some((price - spread).max(0.0)),
        ask: Some(price + spread),
        open: Some((price - spread).max(0.0)),
        high: Some(price + spread * 2.0),
        low: Some((price - spread * 2.0).max(0.0)),
    }
}

/// 单个快照行情（当前时刻）
pub fn synthetic_quote(symbol: &str) -> MarketData {
    synthetic_quote_at(symbol, Utc::now())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 同种子同序列：行情价格与时间无关（只 timestamp 随注入时刻变）
    #[test]
    fn quote_is_deterministic_per_symbol() {
        let at = Utc::now();
        let a = synthetic_quote_at("600519", at);
        let b = synthetic_quote_at("600519", at);
        assert_eq!(a.price, b.price, "同符号同时刻价格应一致");
        assert_eq!(a.volume, b.volume);
        assert_eq!(
            synthetic_quote("600519").price,
            synthetic_quote("600519").price,
            "价格与调用时刻无关"
        );
    }

    #[test]
    fn different_symbols_get_different_base_prices() {
        let at = Utc::now();
        assert_ne!(
            synthetic_quote_at("600519", at).price,
            synthetic_quote_at("000001", at).price,
            "不同符号起点价不同"
        );
    }

    #[test]
    fn series_shape_and_timestamps() {
        let end = Utc::now();
        let series = synthetic_series_at("600519", 10, end);
        assert_eq!(series.len(), 10);
        assert_eq!(series.last().unwrap().timestamp, end, "末根为注入时刻");
        assert!(
            series.windows(2).all(|w| w[0].timestamp < w[1].timestamp),
            "时间戳递增"
        );
        assert!(
            synthetic_series_at("600519", 0, end).is_empty(),
            "bars=0 空序列"
        );
    }

    #[test]
    fn series_is_deterministic() {
        let end = Utc::now();
        let a = synthetic_series_at("000001", 20, end);
        let b = synthetic_series_at("000001", 20, end);
        assert_eq!(a, b, "同符号同时刻整段序列逐字段一致");
    }

    #[test]
    fn series_quotes_share_consistent_ohlcv() {
        let series = synthetic_series("600519", DEFAULT_BARS);
        let last = series.last().expect("非空");
        assert!(last.price > 0.0 && last.volume > 0);
        let bid = last.bid.expect("bid");
        let ask = last.ask.expect("ask");
        assert!(bid < ask, "买价应低于卖价");
        // 与快照口径一致：同符号快照价格 == 以当前时刻生成的序列末根价格
        let at = Utc::now();
        let quote = synthetic_quote_at("600519", at);
        let tail = synthetic_series_at("600519", DEFAULT_BARS, at);
        assert_eq!(
            quote.price,
            tail.last().unwrap().price,
            "快照与序列同段取数"
        );
    }
}
