//! 高级技术指标算法
//!
//! 提供更复杂的技术分析指标实现

/// 高级技术指标计算器
#[derive(Debug, Clone)]
pub struct AdvancedIndicators {
    precision: usize,
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
    fn calculate_cci(
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

        // 调整数组长度
        for i in 0..atr_sma.len() {
            atr_values[i + 1] = atr_sma[i]; // TR 从 index 1 开始
        }

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
    pub fn identify_elliott_waves(&self, prices: &[f64]) -> Vec<ElliottWave> {
        // 简化的艾略特波浪识别
        let mut waves = Vec::new();

        // 这里可以实现更复杂的波浪识别算法
        // 当前只是一个简单的示例实现

        if prices.len() < 100 {
            return waves;
        }

        // 寻找简单的 5 波模式
        for i in 20..prices.len().saturating_sub(20) {
            let segment = &prices[i - 20..=i];

            // 检查是否形成了类似波浪的模式
            if self.is_five_wave_pattern(segment) {
                waves.push(ElliottWave {
                    start_index: i - 20,
                    end_index: i,
                    wave_type: WaveType::Impulse,
                    confidence: 0.7,
                });
            }
        }

        waves
    }

    /// 检查是否为 5 波模式
    fn is_five_wave_pattern(&self, prices: &[f64]) -> bool {
        if prices.len() < 5 {
            return false;
        }

        // 简单检查：是否有 3 个更高的高点
        let mut peaks = 0;
        let mut troughs = 0;

        for i in 1..prices.len() - 1 {
            if prices[i] > prices[i - 1] && prices[i] > prices[i + 1] {
                peaks += 1;
            }
            if prices[i] < prices[i - 1] && prices[i] < prices[i + 1] {
                troughs += 1;
            }
        }

        peaks >= 2 && troughs >= 2
    }

    /// 内部 SMA 计算方法
    fn calculate_sma_internal(&self, values: &[f64], period: usize) -> Vec<f64> {
        if values.len() < period {
            return vec![0.0; values.len()];
        }

        let mut sma = vec![0.0; values.len()];
        let mut sum = 0.0;

        // 计算第一个平均值
        for i in 0..period {
            sum += values[i];
        }
        sma[period - 1] = sum / period as f64;

        // 滑动窗口计算
        for i in period..values.len() {
            sum = sum - values[i - period] + values[i];
            sma[i] = sum / period as f64;
        }

        sma
    }
}

/// 艾略特波浪标记
#[derive(Debug, Clone)]
pub struct ElliottWave {
    pub start_index: usize,
    pub end_index: usize,
    pub wave_type: WaveType,
    pub confidence: f64,
}

/// 波浪类型
#[derive(Debug, Clone)]
pub enum WaveType {
    Impulse,    // 推动浪 (1, 3, 5)
    Corrective, // 调整浪 (2, 4)
    Extended,   // 延伸浪
    Diagonal,   // 斜纹浪
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

    #[test]
    fn test_elliott_wave_identification() {
        let indicators = AdvancedIndicators::new();
        let prices = vec![
            10.0, 11.0, 12.0, 11.5, 12.5, 13.0, 12.0, 11.5, 12.0, 13.5, 12.5, 11.0, 10.5, 11.5,
            12.5, 13.5, 12.0, 11.5, 10.0, 11.0, 12.0,
        ];

        let waves = indicators.identify_elliott_waves(&prices);
        // 输入长度不足时，应返回空结果
        assert!(waves.is_empty());
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
