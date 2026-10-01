//! 回测引擎（纯计算，L0 共享核心层；WASM 侧经 `alpha-wasm-analyzer` 薄绑定暴露）
//!
//! 最小可用口径（TODO「开发高性能 Rust WASM 核心计算库」，2026-09-28）：
//! * 单标的、日频收盘价序列、多头/空仓两态（无杠杆、无做空、无滑点模型）；
//! * 信号在收盘价 `i` 产生即以 `close[i]` 成交（策略自身保证不前瞻：
//!   只读 `prices[..=i]`；跨 bar 确认类策略由调用方在信号函数内约束）；
//! * 手续费按换仓名义额计（`fee_bps` / 10000），进出各计一次；
//! * 指标假设日频：夏普比率以日收益 × √252 年化。
//!
//! 扩展接口缝（留待后续立项，届时按需泛型化而非现在过度设计）：
//! * 多空两态/部分仓位：扩展 [`Position`] 与成交模型；
//! * 更多策略：实现 [`Strategy`] trait 即可接入 [`BacktestEngine::run`]；
//! * 止盈止损/滑点：在 [`Trade`] 成交价生成处插入价格变换层。

use serde::{Deserialize, Serialize};

/// 策略信号：对当前收盘价的趋势判断
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Signal {
    /// 持有/开多
    Long,
    /// 空仓/清仓
    Flat,
}

/// 策略接口：逐 bar 消费收盘价，产出目标仓位信号。
/// 实现自身必须保证不前瞻（只使用当 bar 及之前的数据）。
pub trait Strategy {
    /// 通知当前收盘价（`index` 为该价格在序列中的下标，便于策略做窗口判断）
    fn on_price(&mut self, index: usize, price: f64) -> Signal;
}

/// 双均线交叉策略：快线上穿慢线做多，下穿清仓。
/// 前 `slow_period` 根 bar（不足慢线窗口）保持初始仓位 [`Signal::Flat`]。
#[derive(Debug, Clone)]
pub struct SmaCrossStrategy {
    fast_period: usize,
    slow_period: usize,
    /// 滚动窗口之和：O(1) 更新，长序列无重算
    fast_sum: f64,
    slow_sum: f64,
    fast_win: std::collections::VecDeque<f64>,
    slow_win: std::collections::VecDeque<f64>,
    last_signal: Signal,
}

impl SmaCrossStrategy {
    pub fn new(fast_period: usize, slow_period: usize) -> Self {
        assert!(
            fast_period >= 1 && slow_period > 1 && fast_period < slow_period,
            "要求 1 <= fast < slow"
        );
        Self {
            fast_period,
            slow_period,
            fast_sum: 0.0,
            slow_sum: 0.0,
            fast_win: std::collections::VecDeque::with_capacity(fast_period),
            slow_win: std::collections::VecDeque::with_capacity(slow_period),
            last_signal: Signal::Flat,
        }
    }

    fn fast_sma(&self) -> f64 {
        self.fast_sum / self.fast_win.len() as f64
    }

    fn slow_sma(&self) -> f64 {
        self.slow_sum / self.slow_win.len() as f64
    }
}

impl Strategy for SmaCrossStrategy {
    fn on_price(&mut self, _index: usize, price: f64) -> Signal {
        // 滚动窗口更新：O(1)/bar
        self.fast_win.push_back(price);
        self.fast_sum += price;
        if self.fast_win.len() > self.fast_period {
            if let Some(old) = self.fast_win.pop_front() {
                self.fast_sum -= old;
            }
        }
        self.slow_win.push_back(price);
        self.slow_sum += price;
        if self.slow_win.len() > self.slow_period {
            if let Some(old) = self.slow_win.pop_front() {
                self.slow_sum -= old;
            }
        }

        // 窗口未满：维持 Flat（慢线尚无有效均值）
        if self.slow_win.len() < self.slow_period {
            return self.last_signal;
        }

        // 交叉判定：快线在慢线上方 → Long，否则 Flat
        let signal = if self.fast_sma() > self.slow_sma() {
            Signal::Long
        } else {
            Signal::Flat
        };
        self.last_signal = signal;
        signal
    }
}

/// 一笔完整往返交易（开仓到清仓）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trade {
    pub entry_index: usize,
    pub exit_index: usize,
    pub entry_price: f64,
    pub exit_price: f64,
    /// 扣除双边手续费后的收益率（%）
    pub pnl_pct: f64,
}

/// 回测报告
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestReport {
    /// 逐 bar 净值（初始为 1.0，含手续费）
    pub equity_curve: Vec<f64>,
    /// 累计收益率（%）
    pub total_return_pct: f64,
    /// 最大回撤（%，峰值到谷值）
    pub max_drawdown_pct: f64,
    /// 年化夏普比率（日收益 × √252；无风险利率按 0 处理）
    pub sharpe_ratio: f64,
    /// 胜率（往返交易 pnl > 0 的比例，%）；无成交时为 0
    pub win_rate_pct: f64,
    pub trade_count: usize,
    pub trades: Vec<Trade>,
}

/// 回测引擎：驱动策略逐 bar 消费价格，按目标仓位换仓并记账。
#[derive(Debug, Clone, Default)]
pub struct BacktestEngine {
    /// 单边手续费（基点，如 5 = 0.05%）
    pub fee_bps: f64,
}

impl BacktestEngine {
    pub fn new(fee_bps: f64) -> Self {
        Self { fee_bps }
    }

    /// 运行回测；`prices` 需按时间升序、非空。
    pub fn run(&self, prices: &[f64], strategy: &mut dyn Strategy) -> BacktestReport {
        assert!(!prices.is_empty(), "价格序列不能为空");

        let fee = self.fee_bps / 10_000.0;
        let mut equity = 1.0_f64;
        let mut position = false;
        let mut entry_index = 0usize;
        let mut entry_price = 0.0_f64;
        let mut prev_price = prices[0];
        let mut equity_curve = Vec::with_capacity(prices.len());
        let mut trades: Vec<Trade> = Vec::new();

        for (index, &price) in prices.iter().enumerate() {
            let signal = strategy.on_price(index, price);

            // 持仓期间净值按 bar 间价格比推进（入场当 bar 的入场价即收盘价，不计浮动）
            if position {
                equity *= price / prev_price;
            }

            let want_long = signal == Signal::Long;
            if want_long && !position {
                // 开仓：当 bar 收盘价成交，计一次手续费
                equity *= 1.0 - fee;
                position = true;
                entry_index = index;
                entry_price = price;
            } else if !want_long && position {
                // 清仓：当 bar 收盘价成交，计一次手续费
                equity *= 1.0 - fee;
                let pnl_pct = (price / entry_price * (1.0 - fee) * (1.0 - fee) - 1.0) * 100.0;
                trades.push(Trade {
                    entry_index,
                    exit_index: index,
                    entry_price,
                    exit_price: price,
                    pnl_pct,
                });
                position = false;
            }

            equity_curve.push(equity);
            prev_price = price;
        }

        // 期末仍持仓：按最后收盘价强制估值（未平仓交易不计入 trades 统计）
        let final_equity = equity_curve.last().copied().unwrap_or(1.0);

        BacktestReport {
            total_return_pct: (final_equity - 1.0) * 100.0,
            max_drawdown_pct: max_drawdown(&equity_curve),
            sharpe_ratio: annualized_sharpe(&equity_curve),
            win_rate_pct: win_rate(&trades),
            trade_count: trades.len(),
            trades,
            equity_curve,
        }
    }
}

/// 最大回撤（%）：历史峰值到其后谷值的最大跌幅
fn max_drawdown(equity_curve: &[f64]) -> f64 {
    let mut peak = f64::MIN;
    let mut max_dd = 0.0_f64;
    for &value in equity_curve {
        if value > peak {
            peak = value;
        }
        if peak > 0.0 {
            let dd = (peak - value) / peak * 100.0;
            if dd > max_dd {
                max_dd = dd;
            }
        }
    }
    max_dd
}

/// 年化夏普（日频假设；样本标准差需 ≥ 2 个收益样本，不足返回 0——
/// 2 点净值只有 1 个收益，方差分母 n-1 = 0 会产 NaN 污染寻优排序）
fn annualized_sharpe(equity_curve: &[f64]) -> f64 {
    if equity_curve.len() < 3 {
        return 0.0;
    }
    let returns: Vec<f64> = equity_curve
        .windows(2)
        .map(|pair| pair[1] / pair[0] - 1.0)
        .collect();
    let n = returns.len() as f64;
    let mean = returns.iter().sum::<f64>() / n;
    let variance = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (n - 1.0);
    let std = variance.sqrt();
    if std <= 0.0 {
        return 0.0;
    }
    (mean / std) * 252.0_f64.sqrt()
}

/// 胜率（%）：pnl > 0 的往返交易占比；无成交返回 0
fn win_rate(trades: &[Trade]) -> f64 {
    if trades.is_empty() {
        return 0.0;
    }
    let wins = trades.iter().filter(|t| t.pnl_pct > 0.0).count();
    wins as f64 / trades.len() as f64 * 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 单边上涨序列：SMA(1,2) 于 bar1（价格 20）入场后全程 Long，
    /// 无清仓（期末强平不计入 trades），净值复利 20→100 = +400%
    #[test]
    fn uptrend_holds_single_trade() {
        let prices: Vec<f64> = (1..=10).map(|i| i as f64 * 10.0).collect();
        let mut strategy = SmaCrossStrategy::new(1, 2);
        let report = BacktestEngine::new(0.0).run(&prices, &mut strategy);

        assert_eq!(
            report.trade_count, 0,
            "全程 Long 无清仓，trades 为空（期末强平不计）"
        );
        assert!(
            (report.total_return_pct - 400.0).abs() < 1e-9,
            "入场 20 持有到 100 应 +400%，实际 {}",
            report.total_return_pct
        );
        assert!(
            (report.max_drawdown_pct - 0.0).abs() < 1e-9,
            "单调上涨无回撤"
        );
    }

    /// 手算序列验证换仓与费用记账（fee=100bps=1%）
    #[test]
    fn hand_computed_series_matches_ledger() {
        // 价格 100 → 110（Long 更优）→ 90（Flat 更优）
        // SMA(1,2)：bar1 窗口未满 Flat；bar2: fast=110 slow=105 → Long(入场@110)
        //           bar3: fast=90 slow=100 → Flat(清仓@90)
        let prices = vec![100.0, 110.0, 90.0];
        let mut strategy = SmaCrossStrategy::new(1, 2);
        let report = BacktestEngine::new(100.0).run(&prices, &mut strategy);

        assert_eq!(report.trade_count, 1);
        let trade = &report.trades[0];
        assert_eq!(trade.entry_index, 1);
        assert_eq!(trade.exit_index, 2);
        assert_eq!(trade.entry_price, 110.0);
        assert_eq!(trade.exit_price, 90.0);
        // 毛收益 90/110 = 0.8181…，双边 1% 手续费
        let expected_pnl = (90.0 / 110.0 * 0.99_f64 * 0.99_f64 - 1.0) * 100.0;
        assert!((trade.pnl_pct - expected_pnl).abs() < 1e-9);

        // 净值：bar0 1.0；bar1 入场扣费 0.99；bar2 浮亏后清仓扣费
        let expected_final = 0.99 * (90.0 / 110.0) * 0.99;
        assert!((report.equity_curve[0] - 1.0).abs() < 1e-9);
        assert!((report.equity_curve[1] - 0.99).abs() < 1e-9);
        assert!((report.total_return_pct - (expected_final - 1.0) * 100.0).abs() < 1e-9);

        assert!(report.max_drawdown_pct > 0.0, "亏损序列必有回撤");
        assert!(report.win_rate_pct == 0.0, "唯一交易亏损，胜率 0");
    }

    /// 震荡上涨 vs 全程持有：策略净值曲线仍应单调段正确推进
    #[test]
    fn equity_curve_length_and_shape() {
        let prices = vec![50.0, 52.0, 51.0, 55.0, 54.0, 58.0];
        let mut strategy = SmaCrossStrategy::new(2, 3);
        let report = BacktestEngine::new(5.0).run(&prices, &mut strategy);

        assert_eq!(report.equity_curve.len(), prices.len(), "逐 bar 净值");
        assert!(report.equity_curve[0] == 1.0, "bar0 无信号无换仓");
        for pair in report.equity_curve.windows(2) {
            assert!(pair[1] > 0.0, "净值恒正");
        }
        // 均值指标可计算且有限
        assert!(report.sharpe_ratio.is_finite());
        assert!(report.max_drawdown_pct.is_finite() && report.max_drawdown_pct >= 0.0);
    }

    /// 空/平坦序列：窗口未满全程 Flat，无交易零收益
    #[test]
    fn flat_series_yields_no_trades() {
        let prices = vec![100.0; 5];
        let mut strategy = SmaCrossStrategy::new(1, 2);
        let report = BacktestEngine::new(5.0).run(&prices, &mut strategy);

        assert_eq!(report.trade_count, 0);
        assert!((report.total_return_pct - 0.0).abs() < 1e-9);
        assert!((report.max_drawdown_pct - 0.0).abs() < 1e-9);
        assert!(
            (report.sharpe_ratio - 0.0).abs() < 1e-9,
            "零收益序列夏普为 0"
        );
    }

    /// 策略 trait 接口缝：自定义策略可接入引擎（后续多空/止损扩展点）
    #[test]
    fn custom_strategy_plugs_into_engine() {
        // 固定持有策略：bar2 起全程 Long
        struct BuyAndHoldFrom(usize);
        impl Strategy for BuyAndHoldFrom {
            fn on_price(&mut self, index: usize, _price: f64) -> Signal {
                if index >= self.0 {
                    Signal::Long
                } else {
                    Signal::Flat
                }
            }
        }

        let prices = vec![100.0, 100.0, 100.0, 120.0];
        let mut strategy = BuyAndHoldFrom(2);
        let report = BacktestEngine::new(0.0).run(&prices, &mut strategy);

        assert_eq!(report.trade_count, 0);
        assert!(
            (report.total_return_pct - 20.0).abs() < 1e-9,
            "bar2 入场吃到 100→120"
        );
    }
}
