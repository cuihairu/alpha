//! 确定性演示行情
//!
//! 骨架阶段尚无真实数据源（接入 api-gateway 属 TODO「统一配置管理」之后的接线工作），
//! 但演示数据不能是 `rand` + `Utc::now()`：那样同一标的每次打开都不一样，命令行/单测
//! 都无法断言。改为 **种子由 symbol 派生的 LCG 伪随机游走 + 可注入结束时刻**，于是
//! demo、单测、前端快照三者同源。
//!
//! 真实数据接入时（TODO L113 本地数据导出等）只需替换本模块的取数实现，
//! 分析/导出链路无需改动。

use alpha_core::models::MarketData;
use chrono::{DateTime, Duration, Utc};

/// 默认 K 线根数（足够 alpha-core 的 SMA(50)/RSI 产出有效值）
pub const DEFAULT_BARS: usize = 100;
/// K 线间隔：1 分钟
pub const BAR_INTERVAL_MINUTES: i64 = 1;

/// symbol → 种子（FNV-1a 32 位，确定性、无依赖）
fn seed_for(symbol: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in symbol.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    hash
}

/// 线性同余伪随机（与 packages/core 流式基准同族，同一 seed 出同一序列）
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

/// 单个快照行情（由同一 LCG 派生，字段口径与序列一致）
pub fn synthetic_quote_at(symbol: &str, at: DateTime<Utc>) -> MarketData {
    let mut rng = Lcg::new(seed_for(symbol));
    // 与 synthetic_series_at 取同一段序列的起点/步长，保持口径一致
    let base_price = 50.0 + (seed_for(symbol) % 45_000) as f64 / 100.0;
    let mut price = base_price;
    for _ in 0..DEFAULT_BARS {
        price = (price * (1.0 + (rng.next_unit() - 0.5) * 0.02)).max(0.01);
    }
    let spread = (price * 0.001).max(0.01);
    MarketData {
        symbol: symbol.to_string(),
        timestamp: at,
        price,
        volume: 1_000 + rng.next_below(99_000),
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

    fn at() -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000, 0).expect("固定时间戳")
    }

    #[test]
    fn series_length_matches_request() {
        for bars in [0_usize, 1, 5, DEFAULT_BARS, 500] {
            let series = synthetic_series_at("600519", bars, at());
            assert_eq!(series.len(), bars, "应产出请求的根数");
        }
    }

    #[test]
    fn series_is_deterministic_for_same_symbol() {
        let a = synthetic_series_at("AAPL", 20, at());
        let b = synthetic_series_at("AAPL", 20, at());
        let prices: Vec<f64> = a.iter().map(|d| d.price).collect();
        let same: Vec<f64> = b.iter().map(|d| d.price).collect();
        assert_eq!(prices, same, "同 symbol 同末根时刻应完全一致");
    }

    #[test]
    fn different_symbols_produce_different_paths() {
        let a = synthetic_series_at("AAPL", 30, at());
        let b = synthetic_series_at("MSFT", 30, at());
        let a_prices: Vec<f64> = a.iter().map(|d| d.price).collect();
        let b_prices: Vec<f64> = b.iter().map(|d| d.price).collect();
        assert_ne!(a_prices, b_prices, "不同标的应有不同序列");
    }

    #[test]
    fn timestamps_are_strictly_increasing_by_interval() {
        let series = synthetic_series_at("AAPL", 10, at());
        assert_eq!(series[0].timestamp, at() - Duration::minutes(9));
        assert_eq!(series[9].timestamp, at(), "末根 bar 时间戳应等于注入的 end");
        for pair in series.windows(2) {
            assert_eq!(
                pair[1].timestamp - pair[0].timestamp,
                Duration::minutes(BAR_INTERVAL_MINUTES),
                "相邻 bar 间隔应恒定"
            );
        }
    }

    #[test]
    fn symbol_is_stamped_on_every_bar() {
        let series = synthetic_series_at("000001", 5, at());
        assert!(series.iter().all(|d| d.symbol == "000001"));
    }

    #[test]
    fn prices_and_ohlc_stay_positive_and_ordered() {
        let series = synthetic_series_at("GOOGL", 200, at());
        for d in &series {
            assert!(d.price > 0.0, "价格须为正: {}", d.price);
            assert!(d.volume >= 1_000, "成交量下限: {}", d.volume);
            let low = d.low.expect("low");
            let high = d.high.expect("high");
            assert!(low > 0.0 && high >= low, "high/low 关系错乱: {d:?}");
            let bid = d.bid.expect("bid");
            let ask = d.ask.expect("ask");
            assert!(ask >= bid, "买卖价倒挂: bid={bid} ask={ask}");
        }
    }

    #[test]
    fn series_is_not_constant() {
        let series = synthetic_series_at("AAPL", 50, at());
        let first = series[0].price;
        assert!(
            series.iter().any(|d| (d.price - first).abs() > 1e-9),
            "序列应体现波动"
        );
    }

    #[test]
    fn quote_is_deterministic_and_instant() {
        let a = synthetic_quote_at("AAPL", at());
        let b = synthetic_quote_at("AAPL", at());
        assert_eq!(a.price, b.price);
        assert_eq!(a.volume, b.volume);
        assert_eq!(a.symbol, "AAPL");
    }

    #[test]
    fn quote_matches_series_scale() {
        let quote = synthetic_quote_at("MSFT", at());
        let series = synthetic_series_at("MSFT", DEFAULT_BARS, at());
        let min = series.iter().map(|d| d.price).fold(f64::INFINITY, f64::min);
        let max = series
            .iter()
            .map(|d| d.price)
            .fold(f64::NEG_INFINITY, f64::max);
        assert!(
            quote.price >= min * 0.5 && quote.price <= max * 1.5,
            "快照价应与序列同量级: quote={} 区间=[{min}, {max}]",
            quote.price
        );
    }

    #[test]
    fn unicode_symbols_supported() {
        let series = synthetic_series_at("贵州茅台", 5, at());
        assert_eq!(series.len(), 5);
        assert_eq!(series[0].symbol, "贵州茅台");
    }

    #[test]
    fn seed_is_stable_across_calls() {
        // 防止无意改动种子派生导致历史快照/演示漂移
        assert_eq!(seed_for("AAPL"), seed_for("AAPL"));
        assert_ne!(seed_for("AAPL"), seed_for("AAPL "));
    }
}
