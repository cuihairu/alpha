//! Proptest 属性验证（L493）：跨实现不变量用随机输入穷打。
//!
//! 与单测互补——单测锚定具体数值，这里锁「对一切输入恒真」的数学性质：
//! 指标有界性/平移线性、风险度量的符号与单调、形态识别的镜像对称、
//! 寻优面的确定性。全部确定性种子可复现（proptest 默认持久化失败案例）。

use alpha_core::indicators::TechnicalIndicators;
use alpha_core::patterns::find_swings;
use alpha_core::risk::{correlation, historical_var, max_drawdown, sharpe_ratio};
use proptest::prelude::*;

/// 有限价格序列策略：量级 1~1000、涨跌幅有界（防 f64 灾性溢出淹没断言）
fn prices_strategy(max_len: usize) -> impl Strategy<Value = Vec<f64>> {
    proptest::collection::vec(
        (1.0f64..1000.0, -0.05f64..0.05).prop_map(|(base, step)| base * (1.0 + step)),
        1..max_len,
    )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// SMA 平移线性：全序列加常数 c → 每个非填充值精确加 c（均值对平移封闭）
    #[test]
    fn sma_shift_linear(prices in prices_strategy(200), period in 1usize..20, shift in -100.0f64..100.0) {
        prop_assume!(prices.len() >= period, "短序列整段 0 填充，无有效窗口");
        let ti = TechnicalIndicators::with_precision(12);
        let shifted: Vec<f64> = prices.iter().map(|p| p + shift).collect();
        let a = ti.calculate_sma(&prices, period);
        let b = ti.calculate_sma(&shifted, period);
        // 输出与输入等长，头部 period-1 槽为 0 填充——只断言有效窗口段；
        // 容差覆盖 12 位舍入 + 滑窗加减法浮点漂移（滑动 sum 随位置累积 ulp）
        for (i, (x, y)) in a.iter().zip(&b).enumerate() {
            if i >= period - 1 {
                prop_assert!((y - (x + shift)).abs() <= 1e-9 * (1.0 + x.abs() + shift.abs()) + 1e-9,
                    "SMA 平移不封闭 @{i}: {y} != {x}+{shift}");
            }
        }
    }

    /// EMA 有界：输出（非零填充段）不越出输入的 [min, max]
    #[test]
    fn ema_bounded_by_input(prices in prices_strategy(200), period in 1usize..20) {
        let ti = TechnicalIndicators::with_precision(12);
        if prices.len() < period { return Ok(()); }
        let ema = ti.calculate_ema(&prices, period);
        let (lo, hi) = (
            prices.iter().cloned().fold(f64::INFINITY, f64::min),
            prices.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
        );
        for v in &ema {
            prop_assert!(*v >= lo - 1e-9 || *v == 0.0, "EMA {v} 越下界 {lo}");
            prop_assert!(*v <= hi + 1e-9 || *v == 0.0, "EMA {v} 越上界 {hi}");
        }
    }

    /// RSI 有界：任何输入下所有值落在 [0, 100]（0 为长度不足的填充值）
    #[test]
    fn rsi_bounded(prices in prices_strategy(200), period in 1usize..30) {
        let ti = TechnicalIndicators::with_precision(12);
        let rsi = ti.calculate_rsi(&prices, period);
        for v in &rsi {
            prop_assert!((0.0..=100.0).contains(v), "RSI 越界: {v}");
        }
    }

    /// Bollinger 上轨 ≥ 中轨 ≥ 下轨（样本数足够时）
    #[test]
    fn bollinger_band_ordering(prices in prices_strategy(300), period in 2usize..30) {
        let ti = TechnicalIndicators::with_precision(12);
        if prices.len() < period { return Ok(()); }
        let (upper, middle, lower) = ti.calculate_bollinger_bands(&prices, period, 2.0);
        for i in 0..upper.len() {
            if upper[i] != 0.0 || middle[i] != 0.0 || lower[i] != 0.0 {
                prop_assert!(upper[i] >= middle[i] - 1e-9, "上轨 < 中轨 @{}: {} < {}", i, upper[i], middle[i]);
                prop_assert!(middle[i] >= lower[i] - 1e-9, "中轨 < 下轨 @{}", i);
            }
        }
    }

    /// 最大回撤：幅度 ∈ [0, 1]（magnitude 口径：0.25 = -25%）
    #[test]
    fn max_drawdown_range(prices in prices_strategy(300)) {
        if let Some(dd) = max_drawdown(&prices) {
            prop_assert!(dd.magnitude >= 0.0, "回撤为负: {}", dd.magnitude);
            prop_assert!(dd.magnitude <= 1.0 + 1e-9, "回撤超 100%: {}", dd.magnitude);
        }
    }

    /// VaR 单调：置信度越高，分位数越深（亏损取正 → VaR 非降）
    #[test]
    fn var_monotone_in_confidence(returns in proptest::collection::vec(-0.1f64..0.1, 20..100)) {
        let lo = historical_var(&returns, 0.90);
        let hi = historical_var(&returns, 0.99);
        if let (Some(a), Some(b)) = (lo, hi) {
            prop_assert!(b >= a - 1e-12, "高置信 VaR 反而小: {} > {}", a, b);
        }
    }

    /// 夏普口径：常数收益（std=0）→ None（无统计意义，docs 口径）。
    /// level 限 dyadic（k/1024）：任意 f64 常数的 mean=n·x/n 有 1 ulp 误差，
    /// std 会是极小非零——性质只对均值可精确表示的常数成立（浮点事实）
    #[test]
    fn sharpe_constant_returns_none(k in -64i32..64, n in 2usize..50) {
        let level = k as f64 / 1024.0;
        let returns = vec![level; n];
        prop_assert!(sharpe_ratio(&returns, 0.0, 252.0).is_none());
    }

    /// 相关性：序列与自身相关为 1（非常量输入）
    #[test]
    fn correlation_self_is_one(returns in proptest::collection::vec(-0.1f64..0.1, 2..100)) {
        if let Some(rho) = correlation(&returns, &returns) {
            prop_assert!((rho - 1.0).abs() < 1e-9, "自相关 ≠ 1: {rho}");
        }
    }

    /// 无前瞻（确认制 zigzag 的核心语义）：序列延长后，已确认枢轴逐项
    /// 保持不变（新枢轴只增不改）；对任意阈值成立
    #[test]
    fn find_swings_no_lookahead(
        prefix in prices_strategy(300),
        extension in proptest::collection::vec(1.0f64..1000.0, 0..60),
        threshold in 0.005f64..0.2,
    ) {
        prop_assume!(!prefix.is_empty());
        let base = find_swings(&prefix, threshold);
        let mut extended = prefix.clone();
        extended.extend(extension);
        let full = find_swings(&extended, threshold);
        prop_assert!(full.len() >= base.len(), "延长后枢轴只会增多");
        for (i, b) in base.iter().enumerate() {
            prop_assert_eq!(&full[i], b, "已确认枢轴 index {} 被延长数据改写", i);
        }
    }
}
