//! 风险管理与投资组合分析（L502）。
//!
//! 纯函数面：输入 `&[f64]`（净值曲线 / 逐期收益率），输出风险度量——
//! 与 [`crate::backtest`]（策略面）和 [`crate::analytics`]（业绩面）互补。
//! 口径约定：
//! - 收益率 = 简单收益率（`(p_t - p_{t-1}) / p_{t-1}`），调用方自行从价格
//!   序列生成；`returns` 为空或长度不足时一律返回 `None`/`0.0` 并在文档
//!   注明，不 panic（行情数据天然有噪声/缺口）；
//! - 年化参数显式传入（A股 252 交易日是惯例不是常数）；
//! - 历史分位数用手写线性插值（Nearest-Rank 保守取向 + 插值平滑），
//!   不引入外部依赖。

use crate::errors::AlphaError;

/// 最大回撤：峰值到其后最低点的最大跌幅
#[derive(Debug, Clone, PartialEq)]
pub struct MaxDrawdown {
    /// 回撤幅度（正数，如 0.25 = -25%）
    pub magnitude: f64,
    /// 峰值下标（equity 序列内）
    pub peak_idx: usize,
    /// 谷底下标
    pub trough_idx: usize,
}

/// 净值序列的最大回撤；空序列/全零返回 None
pub fn max_drawdown(equity: &[f64]) -> Option<MaxDrawdown> {
    if equity.len() < 2 {
        return None;
    }
    let mut peak = equity[0];
    let mut peak_idx = 0usize;
    let mut best = MaxDrawdown {
        magnitude: 0.0,
        peak_idx: 0,
        trough_idx: 0,
    };
    for (i, &v) in equity.iter().enumerate() {
        if v > peak {
            peak = v;
            peak_idx = i;
        }
        // 峰值 ≤ 0 时回撤无意义（分母非正），跳过
        let dd = if peak > 0.0 { (peak - v) / peak } else { 0.0 };
        if dd > best.magnitude {
            best = MaxDrawdown {
                magnitude: dd,
                peak_idx,
                trough_idx: i,
            };
        }
    }
    Some(best)
}

/// 年化波动率：`std(returns) * sqrt(periods_per_year)`
/// （样本标准差，n-1；不足 2 期返回 None）
pub fn volatility_annualized(returns: &[f64], periods_per_year: f64) -> Option<f64> {
    Some(sample_std(returns)? * periods_per_year.sqrt())
}

/// 历史在险价值（Historical VaR）：给定置信度的单期分位亏损，
/// 返回正数（亏损幅度），如 0.03 = 单期 3% VaR。
///
/// 口径：置信度 c 下「亏损超过 VaR 的概率 ≤ 1-c」，即收益升序
/// 分位的 (1-c) 位取负（等价于损失分布的 c 分位）。取 `q(c)`
/// 是收益的乐观端，截断后几乎恒为 0——proptest
/// `var_monotone_in_confidence` 抓出的方向反转，勿回退。
pub fn historical_var(returns: &[f64], confidence: f64) -> Option<f64> {
    if returns.is_empty() || !(0.0..1.0).contains(&confidence) {
        return None;
    }
    let mut sorted = returns.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    // 线性插值分位数：loss = -q(1-confidence)，收益 (1-c) 分位在亏损侧
    let q = quantile(&sorted, 1.0 - confidence)?;
    Some((-q).max(0.0))
}

/// 夏普比率：`(mean(r) - rf) / std(r) * sqrt(periods_per_year)`
/// （算术年化；std=0 时返回 None——全赢或全平的策略没有统计意义）
pub fn sharpe_ratio(
    returns: &[f64],
    risk_free_per_period: f64,
    periods_per_year: f64,
) -> Option<f64> {
    let std = sample_std(returns)?;
    if std <= 0.0 || returns.is_empty() {
        return None;
    }
    let mean = returns.iter().sum::<f64>() / returns.len() as f64;
    Some((mean - risk_free_per_period) / std * periods_per_year.sqrt())
}

/// 索提诺比率：分母换下行波动（仅负超额收益的 RMS），补偿上行波动不受罚
pub fn sortino_ratio(
    returns: &[f64],
    risk_free_per_period: f64,
    periods_per_year: f64,
) -> Option<f64> {
    if returns.is_empty() {
        return None;
    }
    let mean = returns.iter().sum::<f64>() / returns.len() as f64;
    let excess_neg: Vec<f64> = returns
        .iter()
        .filter_map(|&r| {
            let e = r - risk_free_per_period;
            if e < 0.0 {
                Some(e * e)
            } else {
                None
            }
        })
        .collect();
    // 下行偏差：全序列长度为分母（无下行期的惩罚为 0，不是 None）
    let downside = if excess_neg.is_empty() {
        0.0
    } else {
        (excess_neg.iter().sum::<f64>() / returns.len() as f64).sqrt()
    };
    if downside <= 0.0 {
        return None;
    }
    Some((mean - risk_free_per_period) / downside * periods_per_year.sqrt())
}

/// Pearson 相关系数；任一序列为常量返回 None
pub fn correlation(a: &[f64], b: &[f64]) -> Option<f64> {
    if a.len() != b.len() || a.len() < 2 {
        return None;
    }
    let ma = a.iter().sum::<f64>() / a.len() as f64;
    let mb = b.iter().sum::<f64>() / b.len() as f64;
    let (mut cov, mut va, mut vb) = (0.0, 0.0, 0.0);
    for i in 0..a.len() {
        let da = a[i] - ma;
        let db = b[i] - mb;
        cov += da * db;
        va += da * da;
        vb += db * db;
    }
    if va <= 0.0 || vb <= 0.0 {
        return None;
    }
    Some((cov / (va.sqrt() * vb.sqrt())).clamp(-1.0, 1.0))
}

/// 组合波动率：`sqrt(wᵀ Σ w)`，`covariance_matrix` 由调用方给 n×n 协方差
/// （行主序）；权重与矩阵维度不匹配返回错误
pub fn portfolio_volatility(
    weights: &[f64],
    covariance_matrix: &[Vec<f64>],
    periods_per_year: f64,
) -> Result<f64, AlphaError> {
    let n = weights.len();
    if n == 0 || covariance_matrix.len() != n || covariance_matrix.iter().any(|row| row.len() != n)
    {
        return Err(AlphaError::InvalidInput(format!(
            "组合维度不匹配: {n} 权重 vs {} 行协方差",
            covariance_matrix.len()
        )));
    }
    let mut quad = 0.0;
    for i in 0..n {
        for j in 0..n {
            quad += weights[i] * covariance_matrix[i][j] * weights[j];
        }
    }
    // 数值噪声下 quad 理论非负；负值钳 0 防止 sqrt(NaN)
    Ok(quad.max(0.0).sqrt() * periods_per_year.sqrt())
}

/// 固定分数仓位：给定权益、单笔风险分数、入场价与止损价，
/// 返回应买数量（股）。价格/止损非法返回 0。
pub fn position_size_fixed_fractional(
    equity: f64,
    risk_fraction: f64,
    entry_price: f64,
    stop_price: f64,
) -> f64 {
    if entry_price <= 0.0 || equity <= 0.0 {
        return 0.0;
    }
    let risk_amount = equity * risk_fraction;
    let per_unit_risk = (entry_price - stop_price).abs();
    if per_unit_risk <= 0.0 {
        return 0.0;
    }
    (risk_amount / per_unit_risk).floor()
}

/// Kelly 公式：`f* = W - (1 - W) / R`（W 胜率，R 盈亏比），
/// 返回理论最优分数；输入非法返回 None。**工程取向：调用方应使用
/// 半 Kelly 或更低**——估计误差在高杠杆下被放大，见测试注释。
pub fn kelly_fraction(win_rate: f64, win_loss_ratio: f64) -> Option<f64> {
    if !(0.0..=1.0).contains(&win_rate) || win_loss_ratio <= 0.0 {
        return None;
    }
    Some(win_rate - (1.0 - win_rate) / win_loss_ratio)
}

/// 汇总报告：净值 + 收益一次出全套核心指标（工具/面板消费面）
#[derive(Debug, Clone, PartialEq)]
pub struct RiskReport {
    pub max_drawdown: Option<MaxDrawdown>,
    pub volatility_annual: Option<f64>,
    pub var_95: Option<f64>,
    pub sharpe: Option<f64>,
    pub sortino: Option<f64>,
}

impl RiskReport {
    /// `equity` 净值曲线；`returns` 对应逐期收益；252 为 A 股年化基数
    pub fn compute(
        equity: &[f64],
        returns: &[f64],
        risk_free_per_period: f64,
        periods_per_year: f64,
    ) -> Self {
        Self {
            max_drawdown: max_drawdown(equity),
            volatility_annual: volatility_annualized(returns, periods_per_year),
            var_95: historical_var(returns, 0.95),
            sharpe: sharpe_ratio(returns, risk_free_per_period, periods_per_year),
            sortino: sortino_ratio(returns, risk_free_per_period, periods_per_year),
        }
    }
}

// ---------- 内部工具 ----------

/// 样本标准差（n-1）；不足 2 期 None
fn sample_std(xs: &[f64]) -> Option<f64> {
    if xs.len() < 2 {
        return None;
    }
    let mean = xs.iter().sum::<f64>() / xs.len() as f64;
    let var = xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (xs.len() - 1) as f64;
    Some(var.sqrt())
}

/// 线性插值分位数（sorted 升序输入），p ∈ (0, 1)
fn quantile(sorted: &[f64], p: f64) -> Option<f64> {
    let n = sorted.len();
    if n == 0 || !(0.0..=1.0).contains(&p) {
        return None;
    }
    let pos = p * (n - 1) as f64;
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    if lo == hi {
        return Some(sorted[lo]);
    }
    let frac = pos - lo as f64;
    Some(sorted[lo] * (1.0 - frac) + sorted[hi] * frac)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_drawdown_peaks_and_troughs() {
        // 100 → 120（峰 idx1）→ 90（谷 idx2，-25%）→ 110（未超峰）
        let equity = [100.0, 120.0, 90.0, 110.0];
        let dd = max_drawdown(&equity).unwrap();
        assert_eq!(dd.magnitude, 0.25);
        assert_eq!(dd.peak_idx, 1);
        assert_eq!(dd.trough_idx, 2);
        assert!(max_drawdown(&[1.0]).is_none());
    }

    #[test]
    fn var_is_confidence_quantile_of_losses() {
        // 100 期收益铺满 -0.05..0.049，手造确定性序列验证插值：
        // q(0.05) 落在 sorted[4]..sorted[5]（亏损侧）→ VaR(0.95) = 0.04505
        let returns: Vec<f64> = (0..100).map(|i| (i as f64 - 50.0) / 1000.0).collect();
        let var95 = historical_var(&returns, 0.95).unwrap();
        let mut sorted = returns.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let expect = -(sorted[4] * (1.0 - 0.95) + sorted[5] * 0.95);
        assert!(expect > 0.0, "5% 分位在亏损侧，否则测试失去断言力");
        assert!((var95 - expect).abs() < 1e-12);
        assert!(var95 > 0.0, "VaR 是正数亏损幅度");
        // 单调：置信度越高分位越深
        let var99 = historical_var(&returns, 0.99).unwrap();
        assert!(var99 > var95);
        assert!(historical_var(&returns, 1.0).is_none());
    }

    #[test]
    fn sharpe_and_sortino_reward_downside_separation() {
        // 恒定收益：std=0 → Sharpe None（无统计意义）；0.5 精确二进制，
        // 求和/均值无浮点噪声（0.001 这类值会产生 ~1e-19 的假 std）
        let flat = [0.5; 10];
        assert!(sharpe_ratio(&flat, 0.0, 252.0).is_none());
        // 稳赚序列 vs 高波动序列：同均值下 Sharpe 更高
        let steady: Vec<f64> = (0..100)
            .map(|i| if i % 2 == 0 { 0.02 } else { -0.01 })
            .collect();
        let wild: Vec<f64> = (0..100)
            .map(|i| if i % 2 == 0 { 0.06 } else { -0.05 })
            .collect();
        let s1 = sharpe_ratio(&steady, 0.0, 252.0).unwrap();
        let s2 = sharpe_ratio(&wild, 0.0, 252.0).unwrap();
        assert!(s1 > s2, "同向均值下低波动应有更高夏普: {s1} vs {s2}");
        // Sortino 分母只有下行：同均值下 wild 下行更大 → Sortino 更低
        let t1 = sortino_ratio(&steady, 0.0, 252.0).unwrap();
        let t2 = sortino_ratio(&wild, 0.0, 252.0).unwrap();
        assert!(t1 > t2, "下行更大者 Sortino 应更低: {t1} vs {t2}");

        // 上行不被罚的增量语义：固定下行、放大上行——Sortino 增量 > Sharpe 增量
        // （Sharpe 总波动分母被上行同步放大，增益被稀释）
        let base: Vec<f64> = (0..100)
            .map(|i| if i % 2 == 0 { 0.04 } else { -0.01 })
            .collect();
        let bigger: Vec<f64> = (0..100)
            .map(|i| if i % 2 == 0 { 0.40 } else { -0.01 })
            .collect();
        let s_gain =
            sharpe_ratio(&bigger, 0.0, 252.0).unwrap() - sharpe_ratio(&base, 0.0, 252.0).unwrap();
        let t_gain =
            sortino_ratio(&bigger, 0.0, 252.0).unwrap() - sortino_ratio(&base, 0.0, 252.0).unwrap();
        assert!(
            t_gain > s_gain,
            "上行增益在 Sortino 下应更足额: {t_gain} vs {s_gain}"
        );
    }

    #[test]
    fn correlation_bounds_and_portfolio_quadratic_form() {
        let a: Vec<f64> = (0..50).map(|i| i as f64).collect();
        assert_eq!(correlation(&a, &a).unwrap(), 1.0);
        let neg: Vec<f64> = (0..50).map(|i| -(i as f64)).collect();
        assert_eq!(correlation(&a, &neg).unwrap(), -1.0);
        assert!(correlation(&[1.0, 1.0], &[1.0, 2.0]).is_none());

        // 独立性语义（ppY=1 免年化噪声）：两资产 ρ=0、各 σ=0.1 →
        // 等权组合波动 = 0.1/√2；ρ=1 → 0.1（分散化失效）
        let indep = vec![vec![0.01, 0.0], vec![0.0, 0.01]];
        let pv = portfolio_volatility(&[0.5, 0.5], &indep, 1.0).unwrap();
        assert!((pv - 0.1 / 2.0f64.sqrt()).abs() < 1e-12);
        let perfect = vec![vec![0.01, 0.01], vec![0.01, 0.01]];
        let pv_perfect = portfolio_volatility(&[0.5, 0.5], &perfect, 1.0).unwrap();
        assert!((pv_perfect - 0.1).abs() < 1e-12);
        assert!(portfolio_volatility(&[0.5, 0.5], &indep[..1], 1.0).is_err());
    }

    #[test]
    fn position_sizing_and_kelly_bounds() {
        // 10 万权益、单笔 1% 风险、入场 10 止损 9 → 1000/1 = 1000 股
        assert_eq!(
            position_size_fixed_fractional(100_000.0, 0.01, 10.0, 9.0),
            1000.0
        );
        // 止损在上方（空头对称）取绝对距
        assert_eq!(
            position_size_fixed_fractional(100_000.0, 0.01, 9.0, 10.0),
            1000.0
        );
        assert_eq!(
            position_size_fixed_fractional(100_000.0, 0.01, 10.0, 10.0),
            0.0
        );

        // W=0.6, R=2 → f* = 0.6 - 0.4/2 = 0.4
        assert!((kelly_fraction(0.6, 2.0).unwrap() - 0.4).abs() < 1e-12);
        assert!(kelly_fraction(1.2, 2.0).is_none());
        // 全胜 + 有限盈亏比 → f*=1（理论上限）；工程上用半 Kelly 防估计误差
        assert_eq!(kelly_fraction(1.0, 2.0).unwrap(), 1.0);
    }

    #[test]
    fn risk_report_aggregates() {
        // 阴跌序列：95% 分位（收益 5% 位）落在亏损侧 → VaR > 0
        let equity = [100.0, 99.0, 98.0, 97.0];
        let returns = [-0.01, -0.0101, -0.0102];
        let r = RiskReport::compute(&equity, &returns, 0.0, 252.0);
        let dd = r.max_drawdown.unwrap();
        assert!((dd.magnitude - 0.03).abs() < 1e-12); // 100→97
        assert!(r.var_95.unwrap() > 0.0);
        assert!(r.sharpe.unwrap() < 0.0); // 持续亏损
        assert!(r.sortino.is_some());
    }
}
