//! 策略参数优化引擎（L501 的「优化」半边；回测半边见 [`crate::backtest`]）。
//!
//! 两层寻优面，全部复用 [`BacktestEngine`] 记账、以夏普为选择目标：
//! * [`grid_search`]：网格穷举 + 训练/验证切分——样本内选优，样本外
//!   验证（验证段用全新策略实例，杜绝状态泄漏），并给出过拟合比；
//! * [`walk_forward`]：滚动前推检验——把序列切成 n 折，每折独立
//!   「段内寻优 → 紧随其后的段外评估」，拼接全部样本外净值。这是
//!   检验参数稳健性的标准口径：单次 train/val 切分只验证一个时点，
//!   walk-forward 验证「每段都重新寻优」的整条样本外路径。
//!
//! 口径约定：
//! * 参数取离散值（`usize` 轴），策略由调用方工厂闭包按参数元组构造
//!   （如 `|p| Box::new(SmaCrossStrategy::new(p[0], p[1]))`）——不引入
//!   连续优化器，量化参数（周期/阈值档位）天然离散；轴间做独立
//!   笛卡尔积、**不做跨轴约束过滤**，组合合法性由调用方选轴值保证
//!   （如 `fast < slow`，非法组合会在工厂构造时 panic 暴露）；
//! * 选择目标 = [`BacktestReport::sharpe_ratio`]（引擎已按日频年化；
//!   退化序列得 0 分，与回测口径一致）；并列时取参数字典序最小者
//!   （确定性，可复现）；
//! * 切分按时间先后（训练段在前、验证段在后），先入先出——不可用
//!   未来数据训练，这是回测的硬边界。
//!
//! 非目标（登记不做）：连续参数寻优（坐标下降/贝叶斯优化）、成交
//! 滑点模型、多标的组合回测——见 backtest.rs 头注释的扩展接口缝。

use crate::backtest::{BacktestEngine, BacktestReport, Strategy};
use crate::errors::AlphaError;

/// 一维参数轴：名字 + 候选值
#[derive(Debug, Clone)]
pub struct ParamAxis {
    pub name: String,
    pub values: Vec<usize>,
}

impl ParamAxis {
    pub fn new(name: &str, values: &[usize]) -> Self {
        Self {
            name: name.to_string(),
            values: values.to_vec(),
        }
    }
}

/// 单个参数组合的样本内（训练段）成绩
#[derive(Debug, Clone)]
pub struct TrainResult {
    /// 参数元组（与传入轴一一对应）
    pub params: Vec<usize>,
    pub train_sharpe: f64,
    pub train_return_pct: f64,
    pub train_max_drawdown_pct: f64,
}

/// 网格搜索报告：样本内排名 + 样本外验证 + 全样本报告
#[derive(Debug, Clone)]
pub struct GridSearchReport {
    /// 全部组合按样本内夏普降序（并列按参数字典序升序）
    pub ranked: Vec<TrainResult>,
    /// 样本内最优组合
    pub best_params: Vec<usize>,
    /// 最优组合在验证段（样本外）的报告——全新策略实例跑验证段
    pub validation_report: BacktestReport,
    /// 最优组合在全样本上的报告
    pub full_report: BacktestReport,
    /// 过拟合比 = 样本内夏普 / 样本外夏普。样本外夏普 ≤ 0 时记
    /// `INFINITY`（样本内赚钱样本外亏钱 = 典型过拟合信号，调用方
    /// 必须检查）；> 2 通常意味着参数对训练段记忆过深
    pub overfit_ratio: f64,
}

/// 单折 walk-forward 结果：本折寻得的参数 + 段外成绩
#[derive(Debug, Clone)]
pub struct WalkForwardFold {
    pub params: Vec<usize>,
    pub train_sharpe: f64,
    /// 样本外段收益率（%）
    pub oos_return_pct: f64,
    /// 样本外段最大回撤（%）
    pub oos_max_drawdown_pct: f64,
}

/// 滚动前推检验报告：逐折寻优 + 全部样本外净值的复利拼接
#[derive(Debug, Clone)]
pub struct WalkForwardReport {
    pub folds: Vec<WalkForwardFold>,
    /// 拼接的样本外净值曲线（首点 ~1.0；折与折按时间顺序复利衔接，
    /// 与「样本外全程只按当时寻得的参数交易」的实盘语义一致）
    pub oos_equity_curve: Vec<f64>,
    pub oos_total_return_pct: f64,
    pub oos_max_drawdown_pct: f64,
}

/// 网格搜索：样本内穷举选优，样本外验证，全样本出报告。
///
/// * `factory`：按参数元组构造全新策略（每个组合、每段数据都要新实例，
///   策略带滚动窗口状态，复用会串数据）；
/// * `train_ratio` ∈ (0, 1)：训练段占比，切分点按 `floor(len × ratio)`
///   且保证两段各 ≥ 1 根 bar。
pub fn grid_search(
    prices: &[f64],
    engine: &BacktestEngine,
    axes: &[ParamAxis],
    factory: &mut dyn FnMut(&[usize]) -> Box<dyn Strategy>,
    train_ratio: f64,
) -> Result<GridSearchReport, AlphaError> {
    let split = check_inputs_and_split(prices.len(), axes, train_ratio, 1)?;
    let (train, validation) = prices.split_at(split);

    // 网格穷举（多重基数计数器产笛卡尔积），全部在训练段打分
    let mut ranked: Vec<TrainResult> = Vec::new();
    for params in cartesian(axes) {
        let mut strategy = factory(&params);
        let report = engine.run(train, strategy.as_mut());
        ranked.push(TrainResult {
            train_sharpe: report.sharpe_ratio,
            train_return_pct: report.total_return_pct,
            train_max_drawdown_pct: report.max_drawdown_pct,
            params,
        });
    }
    // 降序：夏普优先，并列取参数字典序最小（确定性 tie-break）
    ranked.sort_by(|a, b| {
        b.train_sharpe
            .partial_cmp(&a.train_sharpe)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.params.cmp(&b.params))
    });

    let best = ranked[0].params.clone();
    let mut best_strategy = factory(&best);
    let validation_report = engine.run(validation, best_strategy.as_mut());
    let mut full_strategy = factory(&best);
    let full_report = engine.run(prices, full_strategy.as_mut());

    let train_sharpe = ranked[0].train_sharpe;
    let overfit_ratio = if validation_report.sharpe_ratio > 0.0 {
        train_sharpe / validation_report.sharpe_ratio
    } else {
        f64::INFINITY
    };

    Ok(GridSearchReport {
        ranked,
        best_params: best,
        validation_report,
        full_report,
        overfit_ratio,
    })
}

/// 滚动前推检验（walk-forward）：序列均分 `folds` 折，每折内部按
/// `train_ratio` 切「段内寻优 / 紧随段外评估」，折间独立（每折重新
/// 寻优——模拟实盘按周期重训参数的纪律）。
pub fn walk_forward(
    prices: &[f64],
    engine: &BacktestEngine,
    axes: &[ParamAxis],
    factory: &mut dyn FnMut(&[usize]) -> Box<dyn Strategy>,
    folds: usize,
    train_ratio: f64,
) -> Result<WalkForwardReport, AlphaError> {
    if folds == 0 {
        return Err(AlphaError::InvalidInput("折数必须 ≥ 1".into()));
    }
    if prices.len() < folds {
        return Err(AlphaError::InvalidInput(format!(
            "价格 {} 根不足 {} 折（每折至少 1 根）",
            prices.len(),
            folds
        )));
    }
    let chunk = prices.len() / folds;

    let mut report = WalkForwardReport {
        folds: Vec::with_capacity(folds),
        oos_equity_curve: Vec::new(),
        oos_total_return_pct: 0.0,
        oos_max_drawdown_pct: 0.0,
    };

    let mut prev_end = 1.0_f64; // 折间复利衔接因子
    for f in 0..folds {
        // 末折吸收整除余数
        let end = if f == folds - 1 {
            prices.len()
        } else {
            (f + 1) * chunk
        };
        let start = f * chunk;
        let split = check_inputs_and_split(end - start, axes, train_ratio, 1)?;
        let (train, oos) = prices[start..end].split_at(split);

        // 段内寻优（完整网格重跑——参数必须只看得到本折训练段）
        let mut best_params = Vec::new();
        let mut best_sharpe = f64::NEG_INFINITY;
        for params in cartesian(axes) {
            let mut strategy = factory(&params);
            let r = engine.run(train, strategy.as_mut());
            // 并列取字典序最小：cartesian 按字典序产出，严格大于才替换。
            // 首个组合无条件入选——sharpe 若为 NaN，任何比较都为 false，
            // 不能让 best_params 留空传给工厂
            if r.sharpe_ratio > best_sharpe || best_params.is_empty() {
                best_sharpe = r.sharpe_ratio;
                best_params = params;
            }
        }

        // 紧随其后的段外评估：全新策略实例只跑样本外段，净值直接衔接
        let mut oos_strategy = factory(&best_params);
        let oos_report = engine.run(oos, oos_strategy.as_mut());
        for &v in &oos_report.equity_curve {
            report.oos_equity_curve.push(prev_end * v);
        }
        prev_end = *report.oos_equity_curve.last().unwrap_or(&prev_end);

        report.folds.push(WalkForwardFold {
            params: best_params,
            train_sharpe: best_sharpe,
            oos_return_pct: oos_report.total_return_pct,
            oos_max_drawdown_pct: oos_report.max_drawdown_pct,
        });
    }

    report.oos_total_return_pct = (prev_end - 1.0) * 100.0;
    // 样本外路径的整体回撤（拼接曲线上量，而非逐折平均——折间亏损
    // 叠加才是实盘承受的真实回撤）
    let mut peak = f64::MIN;
    let mut max_dd = 0.0_f64;
    for &v in &report.oos_equity_curve {
        if v > peak {
            peak = v;
        }
        if peak > 0.0 {
            let dd = (peak - v) / peak * 100.0;
            if dd > max_dd {
                max_dd = dd;
            }
        }
    }
    report.oos_max_drawdown_pct = max_dd;

    Ok(report)
}

// ---------- 内部工具 ----------

/// 入口校验 + 切分点计算：`total` 根价格按 `train_ratio` 切，训练/验证
/// 各至少 `min_each` 根。axes 需非空且每轴至少一个候选值。
fn check_inputs_and_split(
    total: usize,
    axes: &[ParamAxis],
    train_ratio: f64,
    min_each: usize,
) -> Result<usize, AlphaError> {
    if axes.is_empty() || axes.iter().any(|a| a.values.is_empty()) {
        return Err(AlphaError::InvalidInput(
            "参数轴不能为空且每轴至少一个候选值".into(),
        ));
    }
    if !(0.0..1.0).contains(&train_ratio) || train_ratio == 0.0 {
        return Err(AlphaError::InvalidInput(format!(
            "train_ratio 须在 (0,1) 开区间内，得到 {train_ratio}"
        )));
    }
    if total < 2 * min_each {
        return Err(AlphaError::InvalidInput(format!(
            "价格序列至少 {min_each} 根训练段 + {min_each} 根验证段，得到 {total} 根"
        )));
    }
    // floor 切分，两端下限钳位
    let split = ((total as f64 * train_ratio).floor() as usize).clamp(min_each, total - min_each);
    Ok(split)
}

/// 参数轴笛卡尔积（字典序产出：末轴变化最快）
fn cartesian(axes: &[ParamAxis]) -> CartesianIter<'_> {
    CartesianIter {
        axes,
        done: axes.iter().any(|a| a.values.is_empty()),
        idx: vec![0; axes.len()],
    }
}

struct CartesianIter<'a> {
    axes: &'a [ParamAxis],
    done: bool,
    idx: Vec<usize>,
}

impl Iterator for CartesianIter<'_> {
    type Item = Vec<usize>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let out: Vec<usize> = self
            .axes
            .iter()
            .zip(&self.idx)
            .map(|(a, &i)| a.values[i])
            .collect();
        // 多重基数计数器进位（末轴最快）
        self.done = true;
        for i in (0..self.idx.len()).rev() {
            self.idx[i] += 1;
            if self.idx[i] < self.axes[i].values.len() {
                self.done = false;
                break;
            }
            self.idx[i] = 0;
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backtest::SmaCrossStrategy;

    /// SmaCross 工厂：params = [fast, slow]
    fn sma_factory(params: &[usize]) -> Box<dyn Strategy> {
        Box::new(SmaCrossStrategy::new(params[0], params[1]))
    }

    fn axes() -> Vec<ParamAxis> {
        // 全组合满足 fast < slow（轴独立积不做跨轴约束）
        vec![
            ParamAxis::new("fast", &[1, 2]),
            ParamAxis::new("slow", &[3, 4]),
        ]
    }

    /// 单边上涨序列：任意 fast<slow 组合在两段都应持多赚钱——
    /// 验证网格穷举完整性（4 组合全入 ranked）、排序正确性、
    /// 样本外为正、过拟合比有限为正。
    #[test]
    fn grid_search_ranks_and_validates_on_trend() {
        let prices: Vec<f64> = (1..=20).map(|i| i as f64 * 10.0).collect();
        let engine = BacktestEngine::new(0.0);
        let report = grid_search(&prices, &engine, &axes(), &mut sma_factory, 0.5).unwrap();

        assert_eq!(report.ranked.len(), 4, "2×2 网格全组合");
        assert!(
            report
                .ranked
                .windows(2)
                .all(|p| p[0].train_sharpe >= p[1].train_sharpe),
            "ranked 按样本内夏普降序"
        );
        assert_eq!(report.best_params, report.ranked[0].params, "best 即榜首");
        assert!(
            report.validation_report.total_return_pct > 0.0,
            "上涨段样本外必为正: {}",
            report.validation_report.total_return_pct
        );
        assert!(
            report.overfit_ratio.is_finite() && report.overfit_ratio > 0.0,
            "同向行情下过拟合比有限为正: {}",
            report.overfit_ratio
        );
        assert_eq!(
            report.full_report.equity_curve.len(),
            prices.len(),
            "全样本报告逐 bar 净值"
        );
    }

    /// 入口校验负路径：空轴 / 空候选 / ratio 出界 / 序列过短
    #[test]
    fn grid_search_rejects_bad_inputs() {
        let prices = vec![10.0, 11.0, 12.0, 13.0];
        let engine = BacktestEngine::new(0.0);
        let one_axis = [ParamAxis::new("fast", &[1])];

        assert!(grid_search(&prices, &engine, &[], &mut sma_factory, 0.5).is_err());
        assert!(grid_search(
            &prices,
            &engine,
            &[ParamAxis::new("fast", &[])],
            &mut sma_factory,
            0.5
        )
        .is_err());
        assert!(grid_search(&prices, &engine, &one_axis, &mut sma_factory, 0.0).is_err());
        assert!(grid_search(&prices, &engine, &one_axis, &mut sma_factory, 1.0).is_err());
        assert!(grid_search(&prices[..1], &engine, &one_axis, &mut sma_factory, 0.5).is_err());
    }

    /// 样本内过拟合登记语义：训练段单边涨、验证段单边跌——最优
    /// 组合样本外夏普 ≤ 0 → overfit_ratio = INFINITY（调用方必须
    /// 检查的信号位）
    #[test]
    fn overfit_ratio_is_infinite_when_validation_fails() {
        // 训练段涨（选多），验证段跌（任何多头策略亏）
        let mut prices: Vec<f64> = (1..=10).map(|i| i as f64 * 10.0).collect();
        prices.extend((1..=10).map(|i| 110.0 - i as f64 * 10.0));
        let engine = BacktestEngine::new(0.0);
        let one_axis = [ParamAxis::new("fast", &[1]), ParamAxis::new("slow", &[2])];
        let report = grid_search(&prices, &engine, &one_axis, &mut sma_factory, 0.5).unwrap();

        assert!(
            report.ranked[0].train_sharpe > 0.0,
            "训练段上涨选中多头参数"
        );
        // long-only 策略在单边下跌验证段持币观望：收益 0、夏普 0——
        // INFINITY 的触发面就是「样本外夏普不 > 0」（含躺平）
        assert!(
            report.validation_report.total_return_pct <= 0.0,
            "验证段下跌段无利可图: {}",
            report.validation_report.total_return_pct
        );
        assert!(
            report.overfit_ratio.is_infinite(),
            "样本外夏普 ≤ 0 → 无穷过拟合比"
        );
    }

    /// walk-forward 手算折账：12 根严格上涨、2 折（各 6 根）、
    /// train_ratio=1/3 → 每折训练 2 根 + 样本外 4 根。SMA(1,2) 在
    /// 样本外段 bar1 入场，折 1 样本外净值 [1, 1, 14/13, 15/13]，
    /// 折 2 复利衔接后合计 15/13 × 21/19。
    #[test]
    fn walk_forward_chains_oos_equity_by_hand() {
        let prices: Vec<f64> = (10..=21).map(|i| i as f64).collect();
        let engine = BacktestEngine::new(0.0);
        let one_axis = [ParamAxis::new("fast", &[1]), ParamAxis::new("slow", &[2])];
        let report =
            walk_forward(&prices, &engine, &one_axis, &mut sma_factory, 2, 1.0 / 3.0).unwrap();

        assert_eq!(report.folds.len(), 2);
        assert!(report.folds.iter().all(|f| f.params == vec![1, 2]));
        // 折 1 样本外 = prices[2..6] = [12,13,14,15]；折 2 = [18..21]
        assert_eq!(report.oos_equity_curve.len(), 8, "每折样本外 4 bar");
        let f1_end = 15.0 / 13.0;
        assert!((report.oos_equity_curve[3] - f1_end).abs() < 1e-12);
        let expected_total = f1_end * (21.0 / 19.0);
        assert!(
            (report.oos_equity_curve[7] - expected_total).abs() < 1e-12,
            "折间复利衔接: {}",
            report.oos_equity_curve[7]
        );
        assert!((report.oos_total_return_pct - (expected_total - 1.0) * 100.0).abs() < 1e-9);
        assert!(
            (report.oos_max_drawdown_pct - 0.0).abs() < 1e-9,
            "全程上涨拼接净值无回撤"
        );
    }

    /// walk-forward 的价值位：单段寻优（grid_search）只验证一个切分点，
    /// 滚动重寻优的样本外路径能暴露行情切换后的亏损——后折样本外
    /// 「冲高回落」段 41 入场 30 出场，真金白银亏 30/41-1。
    #[test]
    fn walk_forward_exposes_regime_change() {
        // fold1 [10..80] 上涨；fold2 训练 [80,70,60,50] 下跌（全组合
        // 躺平、字典序选 (1,2)），样本外 [40,41,30,29] 冲高回落
        let prices = vec![
            10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, // fold1
            80.0, 70.0, 60.0, 50.0, 40.0, 41.0, 30.0, 29.0, // fold2
        ];
        let engine = BacktestEngine::new(0.0);
        let one_axis = [ParamAxis::new("fast", &[1]), ParamAxis::new("slow", &[2])];
        let report = walk_forward(&prices, &engine, &one_axis, &mut sma_factory, 2, 0.5).unwrap();

        assert_eq!(report.folds.len(), 2);
        assert!(
            report.folds[0].oos_return_pct > 0.0,
            "上涨折样本外赚钱: {}",
            report.folds[0].oos_return_pct
        );
        // 样本外 [40,41,30,29]：bar1 追多 @41，bar2 快慢线同值清仓 @30
        let expect = (30.0 / 41.0 - 1.0) * 100.0;
        assert!(
            (report.folds[1].oos_return_pct - expect).abs() < 1e-9,
            "后折冲高回落真亏: {} vs {expect}",
            report.folds[1].oos_return_pct
        );
        assert!(report.oos_max_drawdown_pct > 0.0, "拼接净值有回撤");
        assert!(
            report.oos_total_return_pct < report.folds[0].oos_return_pct,
            "后折亏损拖累整条样本外路径"
        );
    }

    /// walk-forward 入口校验：0 折 / 折数超序列长
    #[test]
    fn walk_forward_rejects_bad_inputs() {
        let prices = vec![10.0, 11.0, 12.0];
        let engine = BacktestEngine::new(0.0);
        let one_axis = [ParamAxis::new("fast", &[1])];

        assert!(walk_forward(&prices, &engine, &one_axis, &mut sma_factory, 0, 0.5).is_err());
        assert!(walk_forward(&prices, &engine, &one_axis, &mut sma_factory, 4, 0.5).is_err());
    }

    /// 笛卡尔积字典序产出（walk_forward 的 tie-break 依赖此顺序）
    #[test]
    fn cartesian_is_lexicographic() {
        let axes = [
            ParamAxis::new("a", &[1, 2]),
            ParamAxis::new("b", &[10, 20, 30]),
        ];
        let got: Vec<Vec<usize>> = cartesian(&axes).collect();
        assert_eq!(
            got,
            vec![
                vec![1, 10],
                vec![1, 20],
                vec![1, 30],
                vec![2, 10],
                vec![2, 20],
                vec![2, 30],
            ]
        );
    }
}
