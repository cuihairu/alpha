//! 高级技术指标算法
//!
//! 提供更复杂的技术分析指标实现

/// 高级技术指标计算器
#[derive(Debug, Clone)]
pub struct AdvancedIndicators {
    /// 输出精度（小数位）；预留配置位，当前指标实现按调用方约定输出
    #[allow(dead_code)]
    precision: usize,
}

impl Default for AdvancedIndicators {
    fn default() -> Self {
        Self::new()
    }
}

impl AdvancedIndicators {
    /// 创建新的高级指标计算器
    pub fn new() -> Self {
        Self { precision: 4 }
    }

    /// 创建带精度的指标计算器
    pub fn with_precision(precision: usize) -> Self {
        Self { precision }
    }

    /// 计算随机指标 (Stochastic Oscillator)
    pub fn calculate_stochastic(
        &self,
        highs: &[f64],
        lows: &[f64],
        closes: &[f64],
        k_period: usize,
        d_period: usize,
    ) -> (Vec<f64>, Vec<f64>) {
        if highs.len() < k_period {
            return (vec![0.0; highs.len()], vec![0.0; highs.len()]);
        }

        let mut k_values = vec![0.0; highs.len()];
        let mut d_values = vec![0.0; highs.len()];

        // 计算 %K
        for i in (k_period - 1)..highs.len() {
            let window_high = &highs[i - (k_period - 1)..=i];
            let window_low = &lows[i - (k_period - 1)..=i];

            let highest = window_high.iter().fold(f64::MIN, |a, &b| a.max(b));
            let lowest = window_low.iter().fold(f64::MAX, |a, &b| a.min(b));

            if highest != lowest {
                k_values[i] = 100.0 * (closes[i] - lowest) / (highest - lowest);
            }
        }

        // 计算 %D (作为 %K 的移动平均)
        let d_sma = self.calculate_sma_internal(&k_values, d_period);
        d_values = d_sma;

        (k_values, d_values)
    }

    /// 计算威廉指标 (Williams %R)
    pub fn calculate_williams_r(
        &self,
        highs: &[f64],
        lows: &[f64],
        closes: &[f64],
        period: usize,
    ) -> Vec<f64> {
        if highs.len() < period {
            return vec![0.0; highs.len()];
        }

        let mut wr_values = vec![0.0; highs.len()];

        for i in (period - 1)..highs.len() {
            let window_high = &highs[i - (period - 1)..=i];
            let window_low = &lows[i - (period - 1)..=i];

            let highest = window_high.iter().fold(f64::MIN, |a, &b| a.max(b));
            let lowest = window_low.iter().fold(f64::MAX, |a, &b| a.min(b));

            if highest != lowest {
                wr_values[i] = -100.0 * (highest - closes[i]) / (highest - lowest);
            }
        }

        wr_values
    }

    /// 计算商品通道指数 (CCI)
    pub fn calculate_cci(
        &self,
        highs: &[f64],
        lows: &[f64],
        closes: &[f64],
        period: usize,
        constant: f64,
    ) -> Vec<f64> {
        if highs.len() < period {
            return vec![0.0; highs.len()];
        }

        let mut cci_values = vec![0.0; highs.len()];

        for i in (period - 1)..highs.len() {
            let window_high = &highs[i - (period - 1)..=i];
            let window_low = &lows[i - (period - 1)..=i];

            let highest = window_high.iter().fold(f64::MIN, |a, &b| a.max(b));
            let lowest = window_low.iter().fold(f64::MAX, |a, &b| a.min(b));

            let typical_price = (highs[i] + lows[i] + closes[i]) / 3.0;
            let sma_tp = (highest + lowest + typical_price) / 3.0;

            let mean_deviation = (highest + lowest) / 2.0;

            if mean_deviation != 0.0 {
                cci_values[i] = (typical_price - sma_tp) / (constant * mean_deviation);
            }
        }

        cci_values
    }

    /// 计算平均真实波幅 (ATR)
    pub fn calculate_atr(
        &self,
        highs: &[f64],
        lows: &[f64],
        closes: &[f64],
        period: usize,
    ) -> Vec<f64> {
        if highs.len() < 2 || period < 1 {
            return vec![0.0; highs.len()];
        }

        let mut atr_values = vec![0.0; highs.len()];
        let mut true_ranges = Vec::with_capacity(highs.len());

        // 计算真实波幅
        for i in 1..highs.len() {
            let high_low = highs[i] - lows[i];
            let high_close_prev = (highs[i] - closes[i - 1]).abs();
            let low_close_prev = (lows[i] - closes[i - 1]).abs();

            let tr = high_low.max(high_close_prev).max(low_close_prev);
            true_ranges.push(tr);
        }

        // 计算 ATR (TR 的移动平均)
        let atr_sma = self.calculate_sma_internal(&true_ranges, period);

        // 调整数组长度（TR 从 index 1 开始对齐）
        atr_values[1..=atr_sma.len()].copy_from_slice(&atr_sma);

        atr_values
    }

    /// 计算动量指标 (Momentum)
    pub fn calculate_momentum(&self, prices: &[f64], period: usize) -> Vec<f64> {
        let mut momentum = vec![0.0; prices.len()];

        for i in period..prices.len() {
            momentum[i] = prices[i] - prices[i - period];
        }

        momentum
    }

    /// 计算变化率 (Rate of Change)
    pub fn calculate_roc(&self, prices: &[f64], period: usize) -> Vec<f64> {
        let mut roc = vec![0.0; prices.len()];

        for i in period..prices.len() {
            if prices[i - period] != 0.0 {
                roc[i] = ((prices[i] - prices[i - period]) / prices[i - period]) * 100.0;
            }
        }

        roc
    }

    /// 计算移动平均收敛散度 (MACD) 的直方图
    pub fn calculate_macd_histogram(&self, macd_line: &[f64], signal_line: &[f64]) -> Vec<f64> {
        let mut histogram = vec![0.0; macd_line.len()];

        for i in 0..macd_line.len().min(signal_line.len()) {
            histogram[i] = (macd_line[i] - signal_line[i]) * 1000.0; // 放大显示
        }

        histogram
    }

    /// 计算布林带宽度
    pub fn calculate_bollinger_band_width(
        &self,
        upper_band: &[f64],
        lower_band: &[f64],
        middle_band: &[f64],
    ) -> Vec<f64> {
        let mut width = vec![0.0; upper_band.len()];

        for i in 0..upper_band.len().min(lower_band.len()) {
            if middle_band[i] != 0.0 {
                width[i] = (upper_band[i] - lower_band[i]) / middle_band[i] * 100.0;
            }
        }

        width
    }

    /// 计算布林带位置 (%B)
    pub fn calculate_bollinger_band_percent_b(
        &self,
        price: f64,
        upper_band: f64,
        lower_band: f64,
    ) -> f64 {
        if upper_band == lower_band {
            50.0
        } else {
            ((price - lower_band) / (upper_band - lower_band)) * 100.0
        }
    }

    /// 计算艾略特波浪理论标记
    /// 识别艾略特推动浪（L500 重写：占位实现换为 zigzag 摆动 +
    /// 经典三规则校验）。窗口滑动扫过全部枢轴起点，同一序列可报出
    /// 多个（不重叠的）推动浪；每条仅当「五浪 + 可选 A-B-C 修正」
    /// 全部通过规则时入列。
    ///
    /// 三条经典规则（R.N. Elliott，牛/熊镜像）：
    /// R1 浪 2 不回撤到浪 1 起点之外；R2 浪 3 永不是最短；
    /// R3 浪 4 不进入浪 1 价格领地（无重叠）。
    pub fn identify_elliott_waves(&self, prices: &[f64]) -> Vec<ElliottWave> {
        let swings = crate::patterns::find_swings(prices, ELLIOTT_SWING_THRESHOLD);
        let mut waves = Vec::new();
        if swings.len() < 6 {
            return waves;
        }
        for start in 0..=swings.len() - 6 {
            let w = &swings[start..];
            let bullish = w[0].kind == crate::patterns::SwingKind::Low;
            let Some(lens) = impulse_rules(&w[..6], bullish) else {
                continue;
            };
            // 浪 5 之后若有 A-B-C 三摆且成立，带上修正信息
            let correction = w
                .get(6..9)
                .and_then(|c| abc_correction(c, w[5].price, bullish));
            let n_pivots = if correction.is_some() { 9 } else { 6 };
            waves.push(ElliottWave {
                start_index: w[0].index,
                end_index: w[n_pivots - 1].index,
                wave_type: WaveType::Impulse,
                confidence: if correction.is_some() { 1.0 } else { 0.9 },
                pivot_indices: w[..n_pivots].iter().map(|s| s.index).collect(),
                // 五浪幅度（价格单位）：1/3/5 用带方向长度，2/4 用回撤幅度
                wave_lengths: [
                    lens[0],
                    (w[1].price - w[2].price).abs(),
                    lens[1],
                    (w[3].price - w[4].price).abs(),
                    lens[2],
                ],
                correction: correction.map(|b_retr| AbcCorrection {
                    b_retracement: b_retr,
                    pivot_indices: [w[6].index, w[7].index, w[8].index],
                }),
            });
        }
        waves
    }

    /// 内部 SMA 计算方法
    fn calculate_sma_internal(&self, values: &[f64], period: usize) -> Vec<f64> {
        if values.len() < period {
            return vec![0.0; values.len()];
        }

        let mut sma = vec![0.0; values.len()];
        // 计算第一个平均值
        let mut sum: f64 = values[..period].iter().sum();
        sma[period - 1] = sum / period as f64;

        // 滑动窗口计算
        for i in period..values.len() {
            sum = sum - values[i - period] + values[i];
            sma[i] = sum / period as f64;
        }

        sma
    }
}

/// 摆动确认阈值：枢轴需被反向 ≥ 2% 的波动确认（过滤噪声级波动）
pub const ELLIOTT_SWING_THRESHOLD: f64 = 0.02;

/// 艾略特推动浪识别结果（L500：枢轴下标 + 五浪幅度 + 可选 A-B-C 修正；
/// 返回者均已通过经典三规则校验）
#[derive(Debug, Clone)]
pub struct ElliottWave {
    /// 起点枢轴价格下标
    pub start_index: usize,
    /// 末枢轴价格下标（有修正时为 C 浪端点，否则浪 5 端点）
    pub end_index: usize,
    pub wave_type: WaveType,
    /// 0.9 = 裸推动浪；1.0 = 通过全部规则且 A-B-C 修正成立
    pub confidence: f64,
    /// 枢轴价格下标（起点→浪 5 端点共 6 个；带修正时 9 个）
    pub pivot_indices: Vec<usize>,
    /// 五浪幅度（价格单位）
    pub wave_lengths: [f64; 5],
    /// 紧随浪 5 的 A-B-C 修正（B 对 A 的回撤比例 + 三枢轴下标）
    pub correction: Option<AbcCorrection>,
}

/// A-B-C 修正段
#[derive(Debug, Clone)]
pub struct AbcCorrection {
    /// B 浪对 A 浪的回撤比例（0,1]——B 不越过浪 5 端点
    pub b_retracement: f64,
    /// A/B/C 三个枢轴的价格下标
    pub pivot_indices: [usize; 3],
}

/// 波浪类型
#[derive(Debug, Clone)]
pub enum WaveType {
    Impulse,    // 推动浪 (1, 3, 5)
    Corrective, // 调整浪 (2, 4)
    Extended,   // 延伸浪
    Diagonal,   // 斜纹浪
}

/// 校验 swings[0..6] 为枢轴的推动浪（牛：L,H,L,H,L,H；熊镜像），
/// 通过三条经典规则返回 [浪1, 浪3, 浪5] 幅度。zigzag 摆动的交替性
/// 保证各段长度为正。
fn impulse_rules(w: &[crate::patterns::Swing], bullish: bool) -> Option<[f64; 3]> {
    let (p0, p1, p2, p3, p4, p5) = (
        w[0].price, w[1].price, w[2].price, w[3].price, w[4].price, w[5].price,
    );
    let (len1, len3, len5) = if bullish {
        (p1 - p0, p3 - p2, p5 - p4)
    } else {
        (p0 - p1, p2 - p3, p4 - p5)
    };
    // R1：浪 2 不回撤到浪 1 起点之外；R2：浪 3 永不是最短；
    // R3：浪 4 不进入浪 1 价格领地（牛：浪 4 底 > 浪 1 顶）
    let r1 = if bullish { p2 > p0 } else { p2 < p0 };
    let r2 = len3 >= len1.min(len5);
    let r3 = if bullish { p4 > p1 } else { p4 < p1 };
    (r1 && r2 && r3).then_some([len1, len3, len5])
}

/// 校验浪 5 之后的 A-B-C 三摆（牛：A 底/B 顶/C 底，B 不超浪 5 顶、
/// C 低于 A；熊镜像），成立返回 B 对 A 的回撤比例
fn abc_correction(c: &[crate::patterns::Swing], wave5_end: f64, bullish: bool) -> Option<f64> {
    let (a, b, d) = (c[0].price, c[1].price, c[2].price);
    let a_len = if bullish {
        wave5_end - a
    } else {
        a - wave5_end
    };
    if a_len <= 0.0 {
        return None;
    }
    let ok = if bullish {
        b < wave5_end && d < a
    } else {
        b > wave5_end && d > a
    };
    let b_retr = if bullish {
        (wave5_end - b) / a_len
    } else {
        (b - wave5_end) / a_len
    };
    ok.then_some(b_retr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stochastic_oscillator() {
        let indicators = AdvancedIndicators::new();
        let highs = vec![10.0, 11.0, 12.0, 11.5, 12.5, 11.0, 10.5];
        let lows = vec![8.0, 9.0, 10.0, 10.5, 11.5, 10.0, 9.5];
        let closes = vec![9.0, 10.0, 11.0, 11.0, 12.0, 10.5, 10.0];

        let (k, d) = indicators.calculate_stochastic(&highs, &lows, &closes, 14, 3);
        assert!(!k.is_empty());
        assert!(!d.is_empty());
    }

    #[test]
    fn test_williams_r() {
        let indicators = AdvancedIndicators::new();
        let highs = vec![10.0, 11.0, 12.0, 11.5, 12.5];
        let lows = vec![8.0, 9.0, 10.0, 10.5, 11.5];
        let closes = vec![9.0, 10.0, 11.0, 11.0, 12.0];

        let wr = indicators.calculate_williams_r(&highs, &lows, &closes, 14);
        assert!(!wr.is_empty());
    }

    #[test]
    fn test_atr() {
        let indicators = AdvancedIndicators::new();
        let highs = vec![10.0, 11.0, 12.0, 11.5, 12.5, 13.0];
        let lows = vec![8.0, 9.0, 10.0, 10.5, 11.5, 12.0];
        let closes = vec![9.0, 10.0, 11.0, 11.0, 12.0, 12.5];

        let atr = indicators.calculate_atr(&highs, &lows, &closes, 14);
        assert!(!atr.is_empty());
        assert!(atr[0] == 0.0); // 第一天的 ATR 为 0
    }

    /// 手算推动浪 + A-B-C 修正：枢轴 L(10),H(14),L(11.5),H(18),
    /// L(14.5),H(17) 过三规则（R1 11.5>10、R2 6.5≥min(4,2.5)、
    /// R3 14.5>14），修正 A(13)/B(14.8)/C(12.2)，B 回撤 0.55
    #[test]
    fn impulse_with_correction_hand_priced() {
        let indicators = AdvancedIndicators::new();
        let prices = vec![10.0, 14.0, 11.5, 18.0, 14.5, 17.0, 13.0, 14.8, 12.2, 13.0];
        let waves = indicators.identify_elliott_waves(&prices);

        assert_eq!(waves.len(), 1, "唯一有效窗口: {waves:?}");
        let w = &waves[0];
        assert_eq!(w.start_index, 0);
        assert_eq!(w.end_index, 8, "带修正时终点 = C 浪端点");
        assert_eq!(w.pivot_indices.len(), 9);
        assert_eq!(w.confidence, 1.0);
        assert!((w.wave_lengths[0] - 4.0).abs() < 1e-12);
        assert!((w.wave_lengths[2] - 6.5).abs() < 1e-12);
        assert!((w.wave_lengths[4] - 2.5).abs() < 1e-12);
        let corr = w.correction.as_ref().unwrap();
        assert!((corr.b_retracement - 0.55).abs() < 1e-12);
        assert_eq!(corr.pivot_indices, [6, 7, 8]);
    }

    /// 修正缺失：浪 5 后无 A-B-C 三摆 → 裸推动浪（confidence 0.9）
    #[test]
    fn impulse_without_correction() {
        let indicators = AdvancedIndicators::new();
        let prices = vec![10.0, 14.0, 11.5, 18.0, 14.5, 17.0, 13.0];
        let waves = indicators.identify_elliott_waves(&prices);

        assert_eq!(waves.len(), 1);
        let w = &waves[0];
        assert_eq!(w.end_index, 5, "无修正时终点 = 浪 5 顶");
        assert_eq!(w.pivot_indices.len(), 6);
        assert_eq!(w.confidence, 0.9);
        assert!(w.correction.is_none());
    }

    /// 熊市镜像：高位起跌的推动浪 + 修正，同样过三规则
    #[test]
    fn bearish_impulse_mirrors_rules() {
        let indicators = AdvancedIndicators::new();
        let prices = vec![17.0, 13.0, 15.5, 9.0, 12.5, 10.0, 14.0, 12.2, 14.8, 13.5];
        let waves = indicators.identify_elliott_waves(&prices);

        assert_eq!(waves.len(), 1, "只有高位起跌窗口有效: {waves:?}");
        let w = &waves[0];
        assert_eq!(w.start_index, 0);
        assert!((w.wave_lengths[0] - 4.0).abs() < 1e-12, "浪 1 幅度 17→13");
        let corr = w.correction.as_ref().unwrap();
        assert!((corr.b_retracement - 0.55).abs() < 1e-12);
    }

    /// 规则拒绝：R1（浪 2 全回撤）与 R3（浪 4 入浪 1 领地）各自击杀
    #[test]
    fn rule_violations_rejected() {
        let indicators = AdvancedIndicators::new();
        // R1：L2=9.5 < L0=10，浪 2 回撤过起点
        let r1_bad = vec![10.0, 14.0, 9.5, 18.0, 14.5, 17.0, 15.5];
        assert!(
            indicators.identify_elliott_waves(&r1_bad).is_empty(),
            "浪 2 全回撤必须拒绝"
        );
        // R3：L4=13.5 < H1=14，浪 4 进入浪 1 领地
        let r3_bad = vec![10.0, 14.0, 11.5, 18.0, 13.5, 17.0, 15.5];
        assert!(
            indicators.identify_elliott_waves(&r3_bad).is_empty(),
            "浪 4 重叠浪 1 必须拒绝"
        );
        // R2：浪 3 最短（len1=90, len3=65, len5=80）
        let r2_bad = vec![10.0, 100.0, 50.0, 115.0, 110.0, 190.0, 170.0];
        assert!(
            indicators.identify_elliott_waves(&r2_bad).is_empty(),
            "浪 3 最短必须拒绝"
        );
    }

    /// 噪声级波动（< 2% 阈值）不产生枢轴 → 无浪
    #[test]
    fn sub_threshold_noise_yields_no_waves() {
        let indicators = AdvancedIndicators::new();
        let prices = vec![10.0, 10.1, 10.05, 10.15, 10.08];
        assert!(indicators.identify_elliott_waves(&prices).is_empty());
        assert!(indicators.identify_elliott_waves(&[10.0]).is_empty());
    }

    #[test]
    fn test_bollinger_band_percent_b() {
        let indicators = AdvancedIndicators::new();

        let bb_percent = indicators.calculate_bollinger_band_percent_b(
            105.0, // 价格
            110.0, // 上轨
            95.0,  // 下轨
        );

        let rounded = (bb_percent * 100.0).round() / 100.0;
        assert_eq!(rounded, 66.67); // ((105-95)/(110-95) * 100
    }
}
