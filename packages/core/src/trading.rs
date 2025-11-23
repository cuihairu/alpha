//! 交易策略模块
//!
//! 提供量化交易策略实现和回测功能

use crate::models::*;
use crate::errors::{AlphaResult, AlphaError};
use crate::indicators::TechnicalIndicators;
use crate::indicators::advanced::AdvancedIndicators;

/// 交易策略特征
#[async_trait::async_trait]
pub trait TradingStrategy: Send + Sync {
    /// 策略名称
    fn name(&self) -> &str;

    /// 策略描述
    fn description(&self) -> &str;

    /// 生成交易信号
    async fn generate_signals(&self, data: &[MarketData]) -> AlphaResult<Vec<TradingSignal>>;

    /// 计算策略性能指标
    fn calculate_performance(&self, signals: &[TradingSignal], data: &[MarketData]) -> StrategyPerformance;
}

/// 交易信号
#[derive(Debug, Clone)]
pub struct TradingSignal {
    pub symbol: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub signal_type: SignalType,
    pub price: f64,
    pub confidence: f64,
    pub strategy_name: String,
    pub stop_loss: Option<f64>,
    pub take_profit: Option<f64>,
    pub position_size: Option<f64>,
    pub metadata: serde_json::Value,
}

impl TradingSignal {
    pub fn buy(
        symbol: String,
        price: f64,
        confidence: f64,
        strategy: &str,
    ) -> Self {
        Self {
            symbol,
            timestamp: chrono::Utc::now(),
            signal_type: SignalType::Buy,
            price,
            confidence,
            strategy_name: strategy.to_string(),
            stop_loss: None,
            take_profit: None,
            position_size: None,
            metadata: serde_json::Value::Null,
        }
    }

    pub fn sell(
        symbol: String,
        price: f64,
        confidence: f64,
        strategy: &str,
    ) -> Self {
        Self {
            symbol,
            timestamp: chrono::Utc::now(),
            signal_type: SignalType::Sell,
            price,
            confidence,
            strategy_name: strategy.to_string(),
            stop_loss: None,
            take_profit: None,
            position_size: None,
            metadata: serde_json::Value::Null,
        }
    }

    pub fn with_stop_loss(mut self, stop_loss: f64) -> Self {
        self.stop_loss = Some(stop_loss);
        self
    }

    pub fn with_take_profit(mut self, take_profit: f64) -> Self {
        self.take_profit = Some(take_profit);
        self
    }

    pub fn with_position_size(mut self, size: f64) -> Self {
        self.position_size = Some(size);
        self
    }
}

/// 策略性能指标
#[derive(Debug, Clone)]
pub struct StrategyPerformance {
    /// 总收益率
    pub total_return: f64,
    /// 年化收益率
    pub annualized_return: f64,
    /// 夏普比率
    pub sharpe_ratio: f64,
    /// 最大回撤
    pub max_drawdown: f64,
    /// 胜胜率
    pub win_rate: f64,
    /// 平均收益/亏损比
    pub profit_loss_ratio: f64,
    /// 总交易次数
    pub total_trades: u32,
    /// 获胜交易数
    pub winning_trades: u32,
    /// 平均持仓天数
    pub avg_holding_days: f64,
}

impl StrategyPerformance {
    pub fn new() -> Self {
        Self {
            total_return: 0.0,
            annualized_return: 0.0,
            sharpe_ratio: 0.0,
            max_drawdown: 0.0,
            win_rate: 0.0,
            profit_loss_ratio: 0.0,
            total_trades: 0,
            winning_trades: 0,
            avg_holding_days: 0.0,
        }
    }
}

/// 双移动平均线交叉策略
#[derive(Debug, Clone)]
pub struct DualMovingAverage {
    fast_period: usize,
    slow_period: usize,
    position_size: f64,
    stop_loss_percent: f64,
    take_profit_percent: f64,
}

impl DualMovingAverage {
    pub fn new(fast_period: usize, slow_period: usize) -> Self {
        Self {
            fast_period,
            slow_period,
            position_size: 0.1, // 10% 仓位
            stop_loss_percent: 0.02, // 2% 止损
            take_profit_percent: 0.05, // 5% 止盈
        }
    }

    pub fn with_position_size(mut self, size: f64) -> Self {
        self.position_size = size;
        self
    }

    pub fn with_stops(mut self, stop_loss: f64, take_profit: f64) -> Self {
        self.stop_loss_percent = stop_loss;
        self.take_profit_percent = take_profit;
        self
    }
}

#[async_trait::async_trait]
impl TradingStrategy for DualMovingAverage {
    fn name(&self) -> &str {
        "Dual Moving Average"
    }

    fn description(&self) -> &str {
        "基于快慢移动平均线交叉的简单趋势跟随策略"
    }

    async fn generate_signals(&self, data: &[MarketData]) -> AlphaResult<Vec<TradingSignal>> {
        if data.len() < self.slow_period.max(self.fast_period) {
            return Ok(vec![]);
        }

        let prices: Vec<f64> = data.iter().map(|d| d.price).collect();
        let indicators = TechnicalIndicators::new();

        let fast_sma = indicators.calculate_sma(&prices, self.fast_period);
        let slow_sma = indicators.calculate_sma(&prices, self.slow_period);

        let mut signals = Vec::new();
        let mut in_position = false;
        let entry_price = 0.0;

        for i in (self.slow_period - 1)..data.len() {
            let fast = fast_sma[i];
            let slow = slow_sma[i];

            if !in_position {
                // 检查金叉信号 (快线上穿慢线)
                if fast > slow && i > 0 && fast_sma[i - 1] <= slow_sma[i - 1] {
                    let stop_loss = data[i].price * (1.0 - self.stop_loss_percent);
                    let take_profit = data[i].price * (1.0 + self.take_profit_percent);

                    let signal = TradingSignal::buy(
                        data[i].symbol.clone(),
                        data[i].price,
                        0.7,
                        self.name(),
                    )
                    .with_stop_loss(stop_loss)
                    .with_take_profit(take_profit)
                    .with_position_size(self.position_size);

                    signals.push(signal);
                    in_position = true;
                    entry_price = data[i].price;
                }
            } else {
                // 检查死叉信号 (快线下穿慢线)
                if fast < slow && i > 0 && fast_sma[i - 1] >= slow_sma[i - 1] {
                    let signal = TradingSignal::sell(
                        data[i].symbol.clone(),
                        data[i].price,
                        0.7,
                        self.name(),
                    );

                    signals.push(signal);
                    in_position = false;
                }
            }
        }

        Ok(signals)
    }

    fn calculate_performance(&self, signals: &[TradingSignal], data: &[MarketData]) -> StrategyPerformance {
        let mut performance = StrategyPerformance::new();

        if signals.is_empty() {
            return performance;
        }

        let mut total_return = 0.0;
        let mut max_drawdown = 0.0;
        let mut peak = 0.0;
        let mut total_profit = 0.0;
        let mut total_loss = 0.0;
        let mut wins = 0;
        let mut total_holding_days = 0;
        let mut holdings = Vec::new();

        for signal in signals {
            match signal.signal_type {
                SignalType::Buy => {
                    if let Some(take_profit) = signal.take_profit {
                        total_profit += take_profit - signal.price;
                    }
                    holdings.push((signal.price, signal.timestamp));
                    peak = peak.max(total_return);
                }
                SignalType::Sell => {
                    if let Some(entry_price) = holdings.pop() {
                        let pnl = signal.price - entry_price.0;
                        total_return += pnl;

                        if pnl > 0.0 {
                            total_profit += pnl;
                            wins += 1;
                        } else {
                            total_loss -= pnl;
                        }

                        let drawdown = peak - total_return;
                        max_drawdown = max_drawdown.max(drawdown);

                        total_holding_days += (signal.timestamp - entry_price.1).num_days() as f64;
                    }
                }
                _ => {}
            }
        }

        performance.total_return = total_return;
        performance.max_drawdown = max_drawdown;
        performance.total_trades = signals.len() as u32;
        performance.winning_trades = wins;
        performance.win_rate = if signals.len() > 0 {
            wins as f64 / signals.len() as f64
        } else {
            0.0
        };
        performance.profit_loss_ratio = if total_loss > 0.0 {
            total_profit / total_loss
        } else {
            total_profit.max(1.0) // 避免除零
        };
        performance.avg_holding_days = if signals.len() > 0 {
            total_holding_days / signals.len() as f64
        } else {
            0.0
        };

        // 计算年化收益率（假设252个交易日）
        let days_elapsed = (data[data.len() - 1].timestamp - data[0].timestamp).num_days() as f64;
        if days_elapsed > 0 {
            performance.annualized_return = ((total_return / 100.0) + 1.0).powf(252.0 / days_elapsed) - 1.0) * 100.0;
        }

        // 简化的夏普比率计算（假设无风险利率为2%）
        if days_elapsed > 0 {
            let daily_return = performance.total_return / 100.0 / (days_elapsed / 365.0);
            let daily_volatility = 0.15; // 假设年化波动率为15%
            performance.sharpe_ratio = (daily_return * 252.0 - 0.02) / (daily_volatility * (252.0_f64).sqrt());
        }

        performance
    }
}

/// RSI 超买超卖策略
#[derive(Debug, Clone)]
pub struct RSIStrategy {
    rsi_period: usize,
    oversold_level: f64,
    overbought_level: f64,
    position_size: f64,
}

impl RSIStrategy {
    pub fn new(rsi_period: usize) -> Self {
        Self {
            rsi_period,
            oversold_level: 30.0,
            overbought_level: 70.0,
            position_size: 0.1,
        }
    }

    pub fn with_levels(mut self, oversold: f64, overbought: f64) -> Self {
        self.oversold_level = oversold;
        self.overbought_level = overbought;
        self
    }

    pub fn with_position_size(mut self, size: f64) -> Self {
        self.position_size = size;
        self
    }
}

#[async_trait::async_trait]
impl TradingStrategy for RSIStrategy {
    fn name(&self) -> &str {
        "RSI Mean Reversion"
    }

    fn description(&self) -> &str {
        "基于RSI超买超卖水平的均值回归策略"
    }

    async fn generate_signals(&self, data: &[MarketData]) -> AlphaResult<Vec<TradingSignal>> {
        if data.len() < self.rsi_period + 1 {
            return Ok(vec![]);
        }

        let prices: Vec<f64> = data.iter().map(|d| d.price).collect();
        let indicators = TechnicalIndicators::new();

        let rsi = indicators.calculate_rsi(&prices, self.rsi_period);
        let mut signals = Vec::new();

        for i in 0..data.len() {
            let current_rsi = rsi[i];

            if current_rsi < self.oversold_level && (i == 0 || rsi[i - 1] >= self.oversold_level) {
                // RSI 从上方穿过超卖水平，买入信号
                let signal = TradingSignal::buy(
                    data[i].symbol.clone(),
                    data[i].price,
                    0.6,
                    self.name(),
                )
                .with_position_size(self.position_size);

                signals.push(signal);
            } else if current_rsi > self.overbought_level && (i == 0 || rsi[i - 1] <= self.overbought_level) {
                // RSI 从下方穿过超买水平，卖出信号
                let signal = TradingSignal::sell(
                    data[i].symbol.clone(),
                    data[i].price,
                    0.6,
                    self.name(),
                )
                .with_position_size(self.position_size);

                signals.push(signal);
            }
        }

        Ok(signals)
    }

    fn calculate_performance(&self, signals: &[TradingSignal], data: &[MarketData]) -> StrategyPerformance {
        // 重用性能计算逻辑
        let ma_strategy = DualMovingAverage::new(10, 20);
        ma_strategy.calculate_performance(signals, data)
    }
}

/// 策略组合
#[derive(Debug)]
pub struct StrategyPortfolio {
    strategies: Vec<Box<dyn TradingStrategy>>,
    weights: Vec<f64>,
}

impl StrategyPortfolio {
    pub fn new() -> Self {
        Self {
            strategies: Vec::new(),
            weights: Vec::new(),
        }
    }

    pub fn add_strategy(mut self, strategy: Box<dyn TradingStrategy>, weight: f64) -> Self {
        self.strategies.push(strategy);
        self.weights.push(weight);
        self
    }

    pub async fn generate_combined_signals(&self, data: &[MarketData]) -> AlphaResult<Vec<CombinedSignal>> {
        let mut combined_signals = Vec::new();

        for (strategy, &weight) in self.strategies.iter().zip(&self.weights) {
            let signals = strategy.generate_signals(data).await?;

            for signal in signals {
                let combined = CombinedSignal {
                    signal,
                    weight,
                    strategy_name: strategy.name().to_string(),
                };
                combined_signals.push(combined);
            }
        }

        // 按时间排序
        combined_signals.sort_by(|a, b| a.signal.timestamp.cmp(&b.signal.timestamp));

        Ok(combined_signals)
    }
}

/// 组合交易信号
#[derive(Debug)]
pub struct CombinedSignal {
    pub signal: TradingSignal,
    pub weight: f64,
    pub strategy_name: String,
}

impl CombinedSignal {
    /// 获取加权置信度
    pub fn weighted_confidence(&self) -> f64 {
        self.signal.confidence * self.weight
    }

    /// 应用权重到仓位大小
    pub fn weighted_position_size(&self) -> Option<f64> {
        self.signal.position_size.map(|size| size * self.weight)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[tokio::test]
    async fn test_dual_moving_average_strategy() {
        let strategy = DualMovingAverage::new(5, 20);

        let data = vec![
            MarketData::new("AAPL".to_string(), 100.0, 1000),
            MarketData::new("AAPL".to_string(), 102.0, 1100),
            MarketData::new("AAPL".to_string(), 101.0, 1050),
            MarketData::new("AAPL".to_string(), 103.0, 1200),
            MarketData::new("AAPL".to_string(), 105.0, 1150),
            MarketData::new("AAPL".to_string(), 107.0, 1300),
            MarketData::new("AAPL".to_string(), 106.0, 1250),
            MarketData::new("AAPL".to_string(), 108.0, 1400),
            MarketData::new("AAPL".to_string(), 110.0, 1500),
            MarketData::new("AAPL".to_string(), 112.0, 1600),
            MarketData::new("AAPL".to_string(), 114.0, 1700),
            MarketData::new("AAPL".to_string(), 113.0, 1650),
            MarketData::new("AAPL".to_string(), 115.0, 1750),
            MarketData::new("AAPL".to_string(), 117.0, 1800),
            MarketData::new("AAPL".to_string(), 119.0, 1900),
            MarketData::new("AAPL".to_string(), 118.0, 1850),
            MarketData::new("AAPL".to_string(), 120.0, 2000),
        ];

        let signals = strategy.generate_signals(&data).await.unwrap();

        assert!(!signals.is_empty());
        assert_eq!(signals[0].strategy_name, "Dual Moving Average");
    }

    #[tokio::test]
    async fn test_rsi_strategy() {
        let strategy = RSIStrategy::new(14);

        let data = vec![
            MarketData::new("AAPL".to_string(), 100.0, 1000),
            MarketData::new("AAPL".to_string(), 102.0, 1100),
            MarketData::new("AAPL".to_string(), 104.0, 1200),
            MarketData::new("AAPL".to_string(), 106.0, 1300),
            MarketData::new("AAPL".to_string(), 108.0, 1400),
            MarketData::new("AAPL".to_string(), 110.0, 1500),
            MarketData::new("AAPL".to_string(), 108.0, 1400),
            MarketData::new("AAPL".to_string(), 106.0, 1300),
            MarketData::new("AAPL".to_string(), 104.0, 1200),
            MarketData::new("AAPL".to_string(), 106.0, 1300),
            MarketData::new("AAPL".to_string(), 108.0, 1400),
            MarketData::new("AAPL".to_string(), 110.0, 1500),
            MarketData::new("AAPL".to_string(), 112.0, 1600),
        ];

        let signals = strategy.generate_signals(&data).await.unwrap();

        assert!(!signals.is_empty());
        assert_eq!(signals[0].strategy_name, "RSI Mean Reversion");
    }

    #[test]
    fn test_trading_signal_creation() {
        let signal = TradingSignal::buy(
            "AAPL".to_string(),
            150.0,
            0.8,
            "Test Strategy"
        );

        assert_eq!(signal.signal_type, SignalType::Buy);
        assert_eq!(signal.symbol, "AAPL");
        assert_eq!(signal.price, 150.0);
        assert_eq!(signal.confidence, 0.8);
    }

    #[test]
    fn test_strategy_performance() {
        let strategy = DualMovingAverage::new(5, 20);

        let signals = vec![
            TradingSignal::buy("AAPL".to_string(), 100.0, 0.7, "Dual Moving Average"),
            TradingSignal::sell("AAPL".to_string(), 110.0, 0.7, "Dual Moving Average"),
        ];

        let data = vec![
            MarketData::new("AAPL".to_string(), 100.0, 1000),
            MarketData::new("AAPL".to_string(), 110.0, 1500),
        ];

        let performance = strategy.calculate_performance(&signals, &data);

        assert_eq!(performance.total_trades, 2);
        assert_eq!(performance.total_return, 10.0);
    }

    #[test]
    fn test_strategy_portfolio() {
        let portfolio = StrategyPortfolio::new()
            .add_strategy(Box::new(DualMovingAverage::new(5, 20)), 0.6)
            .add_strategy(Box::new(RSIStrategy::new(14)), 0.4);

        assert_eq!(portfolio.strategies.len(), 2);
        assert_eq!(portfolio.weights.len(), 2);
    }
}