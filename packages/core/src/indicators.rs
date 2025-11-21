//! 技术指标计算模块
//!
//! 提供跨平台的技术指标算法实现，确保所有平台计算结果一致

use crate::models::{IndicatorResult, SignalType, MarketData};
use crate::errors::AlphaError;
use num_traits::Float;

/// 技术指标计算器
#[derive(Debug, Clone)]
pub struct TechnicalIndicators {
    precision: usize,
}

impl TechnicalIndicators {
    /// 创建新的技术指标计算器
    pub fn new() -> Self {
        Self { precision: 4 }
    }

    /// 设置计算精度
    pub fn with_precision(precision: usize) -> Self {
        Self { precision }
    }

    /// 计算简单移动平均线 (SMA)
    pub fn calculate_sma(&self, prices: &[f64], period: usize) -> Vec<f64> {
        if prices.len() < period {
            return vec![0.0; prices.len()];
        }

        let mut sma = vec![0.0; prices.len()];
        let mut sum = 0.0;

        // 计算第一个平均值
        for i in 0..period {
            sum += prices[i];
        }
        sma[period - 1] = (sum / period as f64).round_to(self.precision);

        // 滑动窗口计算
        for i in period..prices.len() {
            sum = sum - prices[i - period] + prices[i];
            sma[i] = (sum / period as f64).round_to(self.precision);
        }

        sma
    }

    /// 计算指数移动平均线 (EMA)
    pub fn calculate_ema(&self, prices: &[f64], period: usize) -> Vec<f64> {
        if prices.is_empty() {
            return vec![];
        }

        let mut ema = vec![0.0; prices.len()];
        let multiplier = 2.0 / (period + 1) as f64;

        // 第一个 EMA 值使用第一个价格
        ema[0] = prices[0];

        // 计算后续 EMA 值
        for i in 1..prices.len() {
            ema[i] = ((prices[i] - ema[i - 1]) * multiplier + ema[i - 1]).round_to(self.precision);
        }

        ema
    }

    /// 计算相对强弱指标 (RSI)
    pub fn calculate_rsi(&self, prices: &[f64], period: usize) -> Vec<f64> {
        if prices.len() < period + 1 {
            return vec![0.0; prices.len()];
        }

        let mut rsi = vec![0.0; prices.len()];
        let mut gains = 0.0;
        let mut losses = 0.0;

        // 计算初始平均增益和损失
        for i in 1..=period {
            let change = prices[i] - prices[i - 1];
            if change > 0.0 {
                gains += change;
            } else {
                losses -= change;
            }
        }

        let mut avg_gain = gains / period as f64;
        let mut avg_loss = losses / period as f64;

        // 计算 RSI 值
        for i in period..prices.len() {
            if avg_loss == 0.0 {
                rsi[i] = 100.0;
            } else {
                let rs = avg_gain / avg_loss;
                rsi[i] = (100.0 - (100.0 / (1.0 + rs))).round_to(self.precision);
            }

            // 更新平均增益和损失
            if i < prices.len() - 1 {
                let change = prices[i + 1] - prices[i];
                let gain = if change > 0.0 { change } else { 0.0 };
                let loss = if change < 0.0 { -change } else { 0.0 };

                avg_gain = (avg_gain * (period - 1) as f64 + gain) / period as f64;
                avg_loss = (avg_loss * (period - 1) as f64 + loss) / period as f64;
            }
        }

        rsi
    }

    /// 计算布林带 (Bollinger Bands)
    pub fn calculate_bollinger_bands(&self, prices: &[f64], period: usize, std_dev: f64) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
        let sma = self.calculate_sma(prices, period);
        let mut upper_band = vec![0.0; prices.len()];
        let mut lower_band = vec![0.0; prices.len()];

        for i in period - 1..prices.len() {
            let slice = &prices[i - period + 1..=i];
            let mean = sma[i];
            let variance = slice.iter()
                .map(|&price| (price - mean).powi(2))
                .sum::<f64>() / period as f64;
            let std_deviation = variance.sqrt();

            upper_band[i] = (mean + std_dev * std_deviation).round_to(self.precision);
            lower_band[i] = (mean - std_dev * std_deviation).round_to(self.precision);
        }

        (upper_band, sma, lower_band)
    }

    /// 计算移动平均收敛散度 (MACD)
    pub fn calculate_macd(&self, prices: &[f64], fast_period: usize, slow_period: usize, signal_period: usize) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
        let ema_fast = self.calculate_ema(prices, fast_period);
        let ema_slow = self.calculate_ema(prices, slow_period);

        let mut macd_line = vec![0.0; prices.len()];
        for i in 0..prices.len() {
            macd_line[i] = (ema_fast[i] - ema_slow[i]).round_to(self.precision);
        }

        let signal_line = self.calculate_ema(&macd_line, signal_period);
        let mut histogram = vec![0.0; prices.len()];

        for i in 0..prices.len() {
            histogram[i] = ((macd_line[i] - signal_line[i]) * 1000.0).round_to(self.precision); // 放大显示
        }

        (macd_line, signal_line, histogram)
    }

    /// 从市场数据计算技术指标
    pub fn calculate_from_market_data(&self, data: &[MarketData], symbol: &str) -> Result<IndicatorResult, AlphaError> {
        if data.is_empty() {
            return Err(AlphaError::InvalidInput("Empty market data".to_string()));
        }

        let prices: Vec<f64> = data.iter().map(|d| d.price).collect();
        let timestamps: Vec<_> = data.iter().map(|d| d.timestamp).collect();

        // 计算 RSI 作为示例
        let rsi_values = self.calculate_rsi(&prices, 14);
        let signals: Vec<SignalType> = rsi_values.iter()
            .map(|&rsi| {
                if rsi > 70.0 { SignalType::Sell }
                else if rsi < 30.0 { SignalType::Buy }
                else { SignalType::Hold }
            })
            .collect();

        Ok(IndicatorResult {
            name: "RSI(14)".to_string(),
            timestamps,
            values: rsi_values,
            signals,
        })
    }
}

/// 浮点数精度处理辅助 trait
trait RoundTo {
    fn round_to(self, precision: usize) -> Self;
}

impl RoundTo for f64 {
    fn round_to(self, precision: usize) -> Self {
        let multiplier = 10_f64.powi(precision as i32);
        (self * multiplier).round() / multiplier
    }
}

impl Default for TechnicalIndicators {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn test_sma_calculation() {
        let indicators = TechnicalIndicators::new();
        let prices = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let sma = indicators.calculate_sma(&prices, 3);

        assert_eq!(sma[2], 2.0); // (1+2+3)/3
        assert_eq!(sma[3], 3.0); // (2+3+4)/3
        assert_eq!(sma[4], 4.0); // (3+4+5)/3
    }

    #[test]
    fn test_rsi_calculation() {
        let indicators = TechnicalIndicators::new();
        let prices = vec![44.0, 44.5, 45.0, 44.8, 45.2, 45.8, 46.2, 46.5, 46.0, 45.8, 45.5, 45.2, 44.8, 44.5, 44.0];
        let rsi = indicators.calculate_rsi(&prices, 14);

        assert!(!rsi.is_empty());
        assert!(rsi[14] >= 0.0 && rsi[14] <= 100.0);
    }

    #[test]
    fn test_market_data_indicators() {
        let indicators = TechnicalIndicators::new();
        let data = vec![
            MarketData::new("AAPL".to_string(), 100.0, 1000),
            MarketData::new("AAPL".to_string(), 101.0, 1100),
            MarketData::new("AAPL".to_string(), 102.0, 1200),
        ];

        let result = indicators.calculate_from_market_data(&data, "AAPL");
        assert!(result.is_ok());
    }
}