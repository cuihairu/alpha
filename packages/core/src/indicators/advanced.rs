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

    /// 计算平均趋向指数 (ADX, Wilder 平滑)
    ///
    /// 口径（Wilder 原著 / StockCharts / Investopedia 一致）：
    /// - TR = max(H−L, |H−C_prev|, |L−C_prev|)；+DM/−DM 取当日单向位移
    ///   （需严格大于反向位移且为正，否则 0）
    /// - TR/±DM 经 Wilder 平滑：首值 = 前 `period` 个样本之和，
    ///   其后 `prev − prev/period + current`（当根全权重）
    /// - ±DI = 100 × 平滑±DM / 平滑TR（平滑 TR 为 0 时双 0）；
    ///   DX = 100 × |+DI−−DI| / (+DI+−DI)（和为 0 时 0）
    /// - ADX = DX 的 Wilder 移动平均：首值 = 前 `period` 个 DX 的均值，
    ///   其后 `(prev×(period−1) + DX)/period`（RMA 递推，凸组合恒 ∈ [0,100]）
    ///
    /// 下标对齐输入：DX 自第 `period` 根 bar 起有定义，ADX 自第
    /// `2×period−1` 根 bar 起有定义，之前一律 0。样本不足两根或周期
    /// 小于 1 → 全 0（与 [`calculate_atr`] 同一兜底口径）。
    pub fn calculate_adx(
        &self,
        highs: &[f64],
        lows: &[f64],
        closes: &[f64],
        period: usize,
    ) -> Vec<f64> {
        if highs.len() < 2 || period < 1 {
            return vec![0.0; highs.len()];
        }

        let n = highs.len();
        let mut adx = vec![0.0; n];

        // TR / +DM / -DM（下标对齐输入，首根 bar 无定义占位 0）
        let mut tr = vec![0.0; n];
        let mut plus_dm = vec![0.0; n];
        let mut minus_dm = vec![0.0; n];
        for i in 1..n {
            let high_low = highs[i] - lows[i];
            let high_close_prev = (highs[i] - closes[i - 1]).abs();
            let low_close_prev = (lows[i] - closes[i - 1]).abs();
            tr[i] = high_low.max(high_close_prev).max(low_close_prev);

            let up_move = highs[i] - highs[i - 1];
            let down_move = lows[i - 1] - lows[i];
            plus_dm[i] = if up_move > down_move && up_move > 0.0 {
                up_move
            } else {
                0.0
            };
            minus_dm[i] = if down_move > up_move && down_move > 0.0 {
                down_move
            } else {
                0.0
            };
        }

        // Wilder 平滑 + DX：平滑值自第 period 根 bar 起（首值 = 前 period 个样本和）
        let period_f = period as f64;
        let mut smooth_tr = 0.0;
        let mut smooth_plus = 0.0;
        let mut smooth_minus = 0.0;
        let mut dx: Vec<f64> = Vec::new();
        for i in period..n {
            if i == period {
                smooth_tr = tr[1..=period].iter().sum();
                smooth_plus = plus_dm[1..=period].iter().sum();
                smooth_minus = minus_dm[1..=period].iter().sum();
            } else {
                smooth_tr = smooth_tr - smooth_tr / period_f + tr[i];
                smooth_plus = smooth_plus - smooth_plus / period_f + plus_dm[i];
                smooth_minus = smooth_minus - smooth_minus / period_f + minus_dm[i];
            }
            let (di_plus, di_minus) = if smooth_tr > 0.0 {
                (
                    100.0 * smooth_plus / smooth_tr,
                    100.0 * smooth_minus / smooth_tr,
                )
            } else {
                (0.0, 0.0)
            };
            let di_sum = di_plus + di_minus;
            let dx_value = if di_sum > 0.0 {
                100.0 * (di_plus - di_minus).abs() / di_sum
            } else {
                0.0
            };
            dx.push(dx_value);
        }

        // ADX = DX 的 Wilder 移动平均（首值 = 前 period 个 DX 均值，其后 RMA 递推）；
        // dx[0] 对齐输入下标 period → 首个 ADX 落在 2×period−1
        if dx.len() >= period {
            let mut prev = dx[..period].iter().sum::<f64>() / period_f;
            let first_adx_index = 2 * period - 1;
            adx[first_adx_index] = prev;
            for (offset, &dx_value) in dx[period..].iter().enumerate() {
                prev = (prev * (period_f - 1.0) + dx_value) / period_f;
                adx[first_adx_index + 1 + offset] = prev;
            }
        }

        adx
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

    /// 随机指标（KDJ 语义）手算对账：k_period=3 / d_period=2。
    /// %K[i] = 100·(C−LL)/(HH−LL)（窗口内最高/最低），%D = %K 的 2 期 SMA。
    /// 手算：K = [0, 0, 75, 200/3, 80, 20, 50/3]，
    /// D = [0, 0, 75/2, 425/6, 220/3, 50, 55/3]（逐项分数精确值）。
    #[test]
    fn test_stochastic_oscillator_hand_priced() {
        let indicators = AdvancedIndicators::new();
        let highs = vec![10.0, 11.0, 12.0, 11.5, 12.5, 11.0, 10.5];
        let lows = vec![8.0, 9.0, 10.0, 10.5, 11.5, 10.0, 9.5];
        let closes = vec![9.0, 10.0, 11.0, 11.0, 12.0, 10.5, 10.0];

        let (k, d) = indicators.calculate_stochastic(&highs, &lows, &closes, 3, 2);

        let expected_k = [0.0, 0.0, 75.0, 200.0 / 3.0, 80.0, 20.0, 50.0 / 3.0];
        let expected_d = [
            0.0,
            0.0,
            75.0 / 2.0,
            425.0 / 6.0,
            220.0 / 3.0,
            50.0,
            55.0 / 3.0,
        ];
        assert_eq!(k.len(), expected_k.len());
        assert_eq!(d.len(), expected_d.len());
        for (got, want) in k.iter().zip(expected_k.iter()) {
            assert!(
                (got - want).abs() < 1e-9,
                "k 手算不符: got {got}, want {want}"
            );
        }
        for (got, want) in d.iter().zip(expected_d.iter()) {
            assert!(
                (got - want).abs() < 1e-9,
                "d 手算不符: got {got}, want {want}"
            );
        }
    }

    /// ADX 手算对账（period=3，9 根 bar）。TR/+DM/−DM、Wilder 平滑、
    /// ±DI/DX 逐项手算后取精确分数：
    /// TR=[0,2,2,1,1.5,1.5,1,1.5,1]，+DM=[0,1,1,0,1,0,0,1,0.5]，
    /// −DM=[0,0,0,0,0,1.5,0.5,0,0]；DX[3..8]=[100,100,20/11,2500/137,2800/109,9340/223]；
    /// ADX 自第 2×period−1=5 根起：[5..8]=[740/11, 230260/4521, 62855480/1478367,
    /// 41841491860/989027523]。
    #[test]
    fn test_adx_hand_priced_wilder_smoothing() {
        let indicators = AdvancedIndicators::new();
        let highs = vec![10.0, 11.0, 12.0, 11.5, 12.5, 11.0, 10.5, 11.5, 12.0];
        let lows = vec![8.0, 9.0, 10.0, 10.5, 11.5, 10.0, 9.5, 10.5, 11.0];
        let closes = vec![9.0, 10.0, 11.0, 11.0, 12.0, 10.5, 10.0, 11.0, 11.5];

        let adx = indicators.calculate_adx(&highs, &lows, &closes, 3);

        assert_eq!(adx.len(), highs.len());
        // 暖机期（第 5 根之前）ADX 为 0
        for (i, &v) in adx[..5].iter().enumerate() {
            assert_eq!(v, 0.0, "bar {i} 应处暖机期");
        }
        let expected = [
            740.0 / 11.0,
            230260.0 / 4521.0,
            62855480.0 / 1478367.0,
            41841491860.0 / 989027523.0,
        ];
        for (got, want) in adx[5..].iter().zip(expected.iter()) {
            assert!(
                (got - want).abs() < 1e-9,
                "adx 手算不符: got {got}, want {want}"
            );
        }
        // DX 恒 ∈ [0,100] 且 RMA 为凸组合 → ADX 有界
        for &v in &adx[5..] {
            assert!((0.0..=100.0).contains(&v));
        }
    }

    /// 无趋势市场（价格走平）ADX 恒 0；样本不足（bar 数 < 2×period）全 0
    #[test]
    fn test_adx_flat_and_insufficient_inputs_stay_zero() {
        let indicators = AdvancedIndicators::new();
        let flat = [10.0; 12];
        let adx_flat = indicators.calculate_adx(&flat, &flat, &flat, 3);
        assert!(adx_flat.iter().all(|&v| v == 0.0), "走平市场 ADX 应恒 0");

        // 单根 bar / 零周期：与 calculate_atr 同一兜底口径
        assert_eq!(
            indicators.calculate_adx(&[10.0], &[8.0], &[9.0], 3).len(),
            1
        );
        assert_eq!(indicators.calculate_adx(&flat, &flat, &flat, 0)[0], 0.0);
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
