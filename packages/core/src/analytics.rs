//! 分析引擎模块

use crate::models::*;
use crate::errors::AlphaResult;
use crate::indicators::TechnicalIndicators;
use chrono::Utc;

/// 市场数据分析引擎
#[derive(Debug, Clone)]
pub struct AnalysisEngine {
    indicators: TechnicalIndicators,
}

impl AnalysisEngine {
    /// 创建新的分析引擎
    pub fn new() -> Self {
        Self {
            indicators: TechnicalIndicators::new(),
        }
    }

    /// 带精度的分析引擎
    pub fn with_precision(precision: usize) -> Self {
        Self {
            indicators: TechnicalIndicators::with_precision(precision),
        }
    }

    /// 分析单个股票的技术指标
    pub async fn analyze_symbol(
        &self,
        data: &[MarketData],
        strategy: Option<&TradingStrategy>,
    ) -> AlphaResult<AnalysisResult> {
        if data.is_empty() {
            return Err(AlphaError::invalid_input("No market data provided"));
        }

        let symbol = &data[0].symbol;
        let mut indicators = Vec::new();

        // 计算 RSI
        let rsi_result = self.indicators.calculate_from_market_data(data, symbol)?;
        indicators.push(rsi_result);

        // 计算移动平均线
        let prices: Vec<f64> = data.iter().map(|d| d.price).collect();
        let timestamps: Vec<_> = data.iter().map(|d| d.timestamp).collect();

        let sma_short = self.indicators.calculate_sma(&prices, 20);
        let sma_long = self.indicators.calculate_sma(&prices, 50);

        indicators.push(IndicatorResult {
            name: "SMA(20)".to_string(),
            timestamps: timestamps.clone(),
            values: sma_short,
            signals: Vec::new(),
        });

        indicators.push(IndicatorResult {
            name: "SMA(50)".to_string(),
            timestamps: timestamps.clone(),
            values: sma_long,
            signals: Vec::new(),
        });

        // 计算 MACD
        let (macd_line, signal_line, histogram) = self.indicators.calculate_macd(&prices, 12, 26, 9);
        indicators.push(IndicatorResult {
            name: "MACD".to_string(),
            timestamps: timestamps.clone(),
            values: macd_line,
            signals: Vec::new(),
        });

        // 计算风险指标
        let risk_metrics = self.calculate_risk_metrics(&prices);

        // 生成推荐信号
        let recommendation = self.generate_recommendation(&indicators, &risk_metrics);
        let confidence = self.calculate_confidence(&indicators, &risk_metrics);

        Ok(AnalysisResult {
            symbol: symbol.clone(),
            analyzed_at: Utc::now(),
            indicators,
            risk_metrics,
            recommendation,
            confidence,
        })
    }

    /// 计算风险指标
    fn calculate_risk_metrics(&self, prices: &[f64]) -> RiskMetrics {
        if prices.len() < 2 {
            return RiskMetrics {
                volatility: 0.0,
                sharpe_ratio: None,
                max_drawdown: 0.0,
                beta: None,
            };
        }

        // 计算收益率
        let returns: Vec<f64> = prices.iter()
            .zip(prices.iter().skip(1))
            .map(|(prev, curr)| (curr - prev) / prev)
            .collect();

        // 计算波动率 (年化)
        let mean_return = returns.iter().sum::<f64>() / returns.len() as f64;
        let variance = returns.iter()
            .map(|r| (r - mean_return).powi(2))
            .sum::<f64>() / (returns.len() - 1) as f64;
        let volatility = variance.sqrt() * (252.0_f64).sqrt(); // 年化波动率

        // 计算最大回撤
        let mut max_price = prices[0];
        let mut max_drawdown = 0.0;
        for &price in prices.iter().skip(1) {
            if price > max_price {
                max_price = price;
            }
            let drawdown = (max_price - price) / max_price;
            if drawdown > max_drawdown {
                max_drawdown = drawdown;
            }
        }

        // 计算夏普比率 (假设无风险利率为 2%)
        let annual_return = (prices[prices.len() - 1] / prices[0] - 1.0) * 252.0 / prices.len() as f64;
        let risk_free_rate = 0.02;
        let sharpe_ratio = if volatility > 0.0 {
            Some((annual_return - risk_free_rate) / volatility)
        } else {
            None
        };

        RiskMetrics {
            volatility,
            sharpe_ratio,
            max_drawdown,
            beta: None, // 需要市场数据才能计算 beta
        }
    }

    /// 生成推荐信号
    fn generate_recommendation(&self, indicators: &[IndicatorResult], risk_metrics: &RiskMetrics) -> SignalType {
        let mut buy_signals = 0;
        let mut sell_signals = 0;

        for indicator in indicators {
            if indicator.values.is_empty() {
                continue;
            }

            let latest_value = indicator.values[indicator.values.len() - 1];

            match indicator.name.as_str() {
                "RSI(14)" => {
                    if latest_value < 30.0 {
                        buy_signals += 1;
                    } else if latest_value > 70.0 {
                        sell_signals += 1;
                    }
                }
                "MACD" => {
                    if let (Some(macd), Some(signal)) = (indicator.values.last(), indicator.values.get(indicator.values.len().saturating_sub(9))) {
                        if macd > signal {
                            buy_signals += 1;
                        } else {
                            sell_signals += 1;
                        }
                    }
                }
                _ => {}
            }
        }

        // 考虑风险指标
        if risk_metrics.volatility > 0.5 {
            // 高波动率，降低买入信号权重
            buy_signals /= 2;
        }

        if risk_metrics.max_drawdown > 0.2 {
            // 大幅回撤，增加卖出信号
            sell_signals += 1;
        }

        if buy_signals > sell_signals {
            SignalType::Buy
        } else if sell_signals > buy_signals {
            SignalType::Sell
        } else {
            SignalType::Hold
        }
    }

    /// 计算推荐置信度
    fn calculate_confidence(&self, indicators: &[IndicatorResult], _risk_metrics: &RiskMetrics) -> f64 {
        if indicators.is_empty() {
            return 0.0;
        }

        let valid_indicators = indicators.iter()
            .filter(|i| !i.values.is_empty())
            .count();

        // 基于指标数量和数据质量的简单置信度计算
        let base_confidence = (valid_indicators as f64 / indicators.len() as f64) * 100.0;

        // 可以进一步基于数据一致性、信号强度等因素调整置信度
        base_confidence.min(100.0).max(0.0)
    }
}

impl Default for AnalysisEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::StrategyParameters;

    #[test]
    fn test_analysis_engine() {
        let engine = AnalysisEngine::new();
        let data = vec![
            MarketData::new("AAPL".to_string(), 100.0, 1000),
            MarketData::new("AAPL".to_string(), 101.0, 1100),
            MarketData::new("AAPL".to_string(), 102.0, 1200),
            MarketData::new("AAPL".to_string(), 103.0, 1300),
            MarketData::new("AAPL".to_string(), 104.0, 1400),
        ];

        // 同步测试 (去除 async)
        let result = std::thread::spawn(move || {
            // 由于是异步函数，我们需要在测试中使用 block_on
            tokio_test::block_on(engine.analyze_symbol(&data, None))
        }).join().unwrap();

        assert!(result.is_ok());
        let analysis = result.unwrap();
        assert_eq!(analysis.symbol, "AAPL");
        assert!(!analysis.indicators.is_empty());
        assert!(matches!(analysis.recommendation, SignalType::Buy | SignalType::Sell | SignalType::Hold));
    }

    #[test]
    fn test_risk_metrics() {
        let engine = AnalysisEngine::new();
        let prices = vec![100.0, 102.0, 98.0, 105.0, 95.0, 110.0];

        let risk = engine.calculate_risk_metrics(&prices);
        assert!(risk.volatility >= 0.0);
        assert!(risk.max_drawdown >= 0.0);
    }
}