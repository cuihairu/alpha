//! 流式数据处理与并行聚合核心（TODO「开发流式数据处理和并行计算机制（Web Workers + Rayon）」
//! 的 L0 纯计算落地件，无平台依赖）。
//!
//! 设计口径（与 hybrid_cache 同款「最小可用 + 显式接口缝」）：
//! * **分块**：[`chunk_plan`] 把全长序列切成连续块（纯函数，块内下标边界全量单测）；
//! * **并行聚合**：[`Summary`] 是可合并统计摘要（Welford 均值/方差 + Chan 等
//!   parallel combine），`merge` 满足结合/交换律——块级结果与合并顺序无关，
//!   这是可安全并行的代数前提（native 参考实现 [`ThreadedStreamingExecutor`]，
//!   wasm 单线程走 [`SequentialStreamingExecutor`]）；
//! * **执行器缝**：[`StreamingExecutor`] trait 对象安全。native 多线程参考实现
//!   用 `std::thread::scope`（Rayon 可直接替换此缝：map 阶段即 rayon::par_iter），
//!   wasm 侧 Web Workers 桥接是 JS glue——Rust 侧只提供 worker 内执行的纯 kernel
//!   （wasm-analyzer `worker.rs` 的 WorkerTask/handle_task 协议 + 本模块聚合）。
//!
//! 线程模型：native `ThreadedStreamingExecutor` 仅 scoped 线程借用 `&[f64]`，
//! 无生命周期逃逸；wasm32 下该实现整体 cfg 门控（std::thread 不可用）。

use serde::{Deserialize, Serialize};
use std::ops::Range;

/// 分块统计摘要：可安全合并的并行聚合单元。
///
/// 字段用 Welford 形式（mean + m2 = Σ(x-mean)²），合并数值稳定性优于
/// sum/sum_sq 形式（大数吃小数）。空片 count=0、min/max=±inf（不直接序列化，
/// 对外经 [`AggregateReport`] 转 Option）。
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Summary {
    count: u64,
    min: f64,
    max: f64,
    mean: f64,
    m2: f64,
}

impl Summary {
    /// 空摘要（合并幺元）
    pub fn empty() -> Self {
        Self {
            count: 0,
            min: f64::INFINITY,
            max: f64::NEG_INFINITY,
            mean: 0.0,
            m2: 0.0,
        }
    }

    /// 单片统计（Welford 单遍）
    pub fn from_slice(values: &[f64]) -> Self {
        let mut acc = Self::empty();
        for &x in values {
            acc.push(x);
        }
        acc
    }

    fn push(&mut self, x: f64) {
        self.count += 1;
        if x < self.min {
            self.min = x;
        }
        if x > self.max {
            self.max = x;
        }
        let delta = x - self.mean;
        self.mean += delta / self.count as f64;
        self.m2 += delta * (x - self.mean);
    }

    /// 并行合并（Chan et al. parallel combine；空摘要为幺元 → 顺序无关）
    pub fn merge(&self, other: &Summary) -> Summary {
        if self.count == 0 {
            return *other;
        }
        if other.count == 0 {
            return *self;
        }
        let total = self.count + other.count;
        let delta = other.mean - self.mean;
        let mean = self.mean + delta * other.count as f64 / total as f64;
        let m2 =
            self.m2 + other.m2 + delta * delta * (self.count * other.count) as f64 / total as f64;
        Summary {
            count: total,
            min: self.min.min(other.min),
            max: self.max.max(other.max),
            mean,
            m2,
        }
    }

    pub fn count(&self) -> u64 {
        self.count
    }

    pub fn mean(&self) -> f64 {
        self.mean
    }

    /// 样本标准差（n-1 分母；样本数 < 2 时为 0，与 backtest::annualized_sharpe 口径一致）
    pub fn std_dev(&self) -> f64 {
        if self.count < 2 {
            return 0.0;
        }
        (self.m2 / (self.count - 1) as f64).sqrt()
    }

    pub fn min(&self) -> f64 {
        self.min
    }

    pub fn max(&self) -> f64 {
        self.max
    }
}

/// 把 `total_len` 切成连续块，返回块内下标区间（对全长序列的划分：不重叠、
/// 有序、并集恰为 `0..total_len`）。`chunk_size` 为 0 属构造期错误。
///
/// 与 wasm-analyzer `worker.rs::plan_chunks` 的关系：那一个按 worker 数均衡
/// 切数据集条目，本函数按固定块大小切单条序列——分别服务「任务并行」与
/// 「数据并行」两种粒度。
pub fn chunk_plan(total_len: usize, chunk_size: usize) -> Vec<Range<usize>> {
    assert!(chunk_size > 0, "chunk_size 须大于 0");
    (0..total_len)
        .step_by(chunk_size)
        .map(|start| start..(start + chunk_size).min(total_len))
        .collect()
}

/// 流式执行器缝：对给定块区间计算块级 [`Summary`]，实现方决定执行模型
/// （单线程 / native 多线程 / Worker 内执行）。对象安全，可 `dyn` 分发。
pub trait StreamingExecutor {
    /// 返回与 `ranges` 等长、同序的摘要序列
    fn map_summaries(&self, prices: &[f64], ranges: &[Range<usize>]) -> Vec<Summary>;
}

/// 单线程参考实现（wasm 主线程与 Worker 内均为此语义）
#[derive(Debug, Clone, Copy, Default)]
pub struct SequentialStreamingExecutor;

impl StreamingExecutor for SequentialStreamingExecutor {
    fn map_summaries(&self, prices: &[f64], ranges: &[Range<usize>]) -> Vec<Summary> {
        ranges
            .iter()
            .map(|range| Summary::from_slice(&prices[range.clone()]))
            .collect()
    }
}

/// native 多线程参考实现（scoped 线程借用切片，线程数自适应封顶）。
/// Rayon 替换缝：map 阶段换 `prices[r].par_iter()` 语义即可，聚合代数不变。
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone, Copy)]
pub struct ThreadedStreamingExecutor {
    /// 线程上限（0 按 1 处理）；实际并发再与块数取 min
    pub max_threads: usize,
}

#[cfg(not(target_arch = "wasm32"))]
impl ThreadedStreamingExecutor {
    /// 按可用并行度封顶的便捷构造（不可用时退化为 2）
    pub fn with_available_parallelism() -> Self {
        let max_threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(2);
        Self { max_threads }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl StreamingExecutor for ThreadedStreamingExecutor {
    fn map_summaries(&self, prices: &[f64], ranges: &[Range<usize>]) -> Vec<Summary> {
        let threads = self.max_threads.max(1).min(ranges.len().max(1));
        if threads <= 1 || ranges.len() <= 1 {
            return SequentialStreamingExecutor.map_summaries(prices, ranges);
        }
        let batch = ranges.len().div_ceil(threads);
        // scoped 线程借用 prices/ranges，join 前不归还——无生命周期逃逸
        let groups: Vec<Vec<Summary>> = std::thread::scope(|scope| {
            let handles: Vec<_> = ranges
                .chunks(batch)
                .map(|group| {
                    scope.spawn(move || {
                        group
                            .iter()
                            .map(|range| Summary::from_slice(&prices[range.clone()]))
                            .collect()
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().expect("聚合线程 panic"))
                .collect()
        });
        // groups 组序 = ranges 大块序、组内序 = 区间序，concat 后与入参同序
        groups.concat()
    }
}

/// 流式聚合报告（对外 JSON 形态；空输入 min/max 为 null）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AggregateReport {
    pub count: u64,
    pub mean: f64,
    pub std_dev: f64,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub chunk_count: usize,
}

/// 分块并行聚合入口：切块 → 执行器并行 map → 可合并归约。
/// 结果与块大小/执行器无关（数学上恒等，测试锁定），块大小只影响并行度与缓存局部性。
pub fn stream_aggregate(
    prices: &[f64],
    chunk_size: usize,
    executor: &dyn StreamingExecutor,
) -> AggregateReport {
    let ranges = chunk_plan(prices.len(), chunk_size);
    let total = executor
        .map_summaries(prices, &ranges)
        .iter()
        .fold(Summary::empty(), |acc, s| acc.merge(s));

    AggregateReport {
        count: total.count(),
        mean: total.mean(),
        std_dev: total.std_dev(),
        min: total.count().checked_sub(1).map(|_| total.min()),
        max: total.count().checked_sub(1).map(|_| total.max()),
        chunk_count: ranges.len(),
    }
}

/// 单 bar 增量流引擎：逐根推进价格，O(1)/bar 维护 SMA/EMA/RSI
/// （与 `TechnicalIndicators` 批式结果逐步等价——语义复刻，等价性由单测锁定）。
///
/// * SMA：滚动原始和（输出才舍入，与批式「和未舍入、值舍入」一致），窗口未满为 `None`；
/// * EMA：首价播种（`ema[0] = price`，不舍入），此后 `round_to(2/(p+1))` 递推；
/// * RSI：Wilder 平滑，前 `period` 个涨跌播种均值，`avg_loss == 0` 时输出 100，
///   与批式一致在累计满 `period` 个变化（即第 `period+1` 个价格）后开始有值。
///
/// 用途：实时看板/Worker 内逐 bar 推进（wasm-analyzer `worker.rs` 流式任务、
/// `examples/streaming_bench.rs` 流式 vs 批式基准）。
#[derive(Debug, Clone)]
pub struct StreamEngine {
    sma_period: usize,
    ema_period: usize,
    rsi_period: usize,
    precision: usize,
    /// 已接收价格数（1-based bar 序号）
    seen: usize,
    sma_win: std::collections::VecDeque<f64>,
    sma_sum: f64,
    ema_value: Option<f64>,
    /// RSI 播种期累计（涨跌各一），`Some` 表示已播种进入 Wilder 平滑阶段
    rsi_pending_gains: f64,
    rsi_pending_losses: f64,
    rsi_changes_seen: usize,
    avg_gain: Option<f64>,
    avg_loss: Option<f64>,
    prev_price: Option<f64>,
}

/// 单 bar 流式输出（窗口未满的指标为 `None`，对齐批式 0 填充语义）
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct StreamBar {
    pub sma: Option<f64>,
    pub ema: Option<f64>,
    pub rsi: Option<f64>,
}

impl StreamEngine {
    /// `sma_period`/`ema_period`/`rsi_period` 均 ≥ 1；精度默认 4
    /// （与 `TechnicalIndicators::new()` 一致，保证等价性测试同口径）
    pub fn new(sma_period: usize, ema_period: usize, rsi_period: usize) -> Self {
        assert!(
            sma_period > 0 && ema_period > 0 && rsi_period > 0,
            "周期须大于 0"
        );
        Self {
            sma_period,
            ema_period,
            rsi_period,
            precision: 4,
            seen: 0,
            sma_win: std::collections::VecDeque::with_capacity(sma_period),
            sma_sum: 0.0,
            ema_value: None,
            rsi_pending_gains: 0.0,
            rsi_pending_losses: 0.0,
            rsi_changes_seen: 0,
            avg_gain: None,
            avg_loss: None,
            prev_price: None,
        }
    }

    fn round(&self, value: f64) -> f64 {
        crate::utils::numeric::round_to(value, self.precision)
    }

    /// 推进一根 bar，返回该 bar 的流式指标值
    pub fn push(&mut self, price: f64) -> StreamBar {
        self.seen += 1;

        // SMA：滚动原始和，窗口满后输出舍入值
        self.sma_win.push_back(price);
        self.sma_sum += price;
        if self.sma_win.len() > self.sma_period {
            if let Some(old) = self.sma_win.pop_front() {
                self.sma_sum -= old;
            }
        }
        let sma = if self.seen >= self.sma_period {
            Some(self.round(self.sma_sum / self.sma_period as f64))
        } else {
            None
        };

        // EMA：首价播种不舍入，此后递推舍入（复刻批式 ema[0] = prices[0]）
        let ema = match self.ema_value {
            None => {
                self.ema_value = Some(price);
                Some(price)
            }
            Some(prev) => {
                let multiplier = 2.0 / (self.ema_period + 1) as f64;
                let value = self.round((price - prev) * multiplier + prev);
                self.ema_value = Some(value);
                Some(value)
            }
        };

        // RSI：播种期累计 → Wilder 平滑；输出节奏与批式一致（变化满 period 后）
        let mut rsi = None;
        if let Some(prev) = self.prev_price {
            let change = price - prev;
            let gain = if change > 0.0 { change } else { 0.0 };
            let loss = if change < 0.0 { -change } else { 0.0 };

            if self.avg_gain.is_none() {
                self.rsi_pending_gains += gain;
                self.rsi_pending_losses += loss;
                self.rsi_changes_seen += 1;
                if self.rsi_changes_seen == self.rsi_period {
                    self.avg_gain = Some(self.rsi_pending_gains / self.rsi_period as f64);
                    self.avg_loss = Some(self.rsi_pending_losses / self.rsi_period as f64);
                }
            } else {
                let p = self.rsi_period as f64;
                self.avg_gain = Some((self.avg_gain.unwrap_or(0.0) * (p - 1.0) + gain) / p);
                self.avg_loss = Some((self.avg_loss.unwrap_or(0.0) * (p - 1.0) + loss) / p);
            }

            // 本 bar 是否产出 RSI：变化数已满播种期（批式首个 RSI 在下标 period）
            if self.rsi_changes_seen >= self.rsi_period {
                let avg_loss = self.avg_loss.unwrap_or(0.0);
                rsi = Some(if avg_loss == 0.0 {
                    100.0
                } else {
                    let rs = self.avg_gain.unwrap_or(0.0) / avg_loss;
                    self.round(100.0 - (100.0 / (1.0 + rs)))
                });
            }
        }
        self.prev_price = Some(price);

        StreamBar { sma, ema, rsi }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// LCG 伪随机序列（确定性、免 rand 依赖，与 zero_copy_bench 同款）
    fn random_walk(n: usize) -> Vec<f64> {
        let mut state = 0x2545_F491_4F6C_DD1D_u64;
        let mut price = 100.0_f64;
        (0..n)
            .map(|_| {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                let r = ((state >> 33) as f64) / (u32::MAX as f64) - 0.5;
                price = (price * (1.0 + r * 0.01)).max(1.0);
                price
            })
            .collect()
    }

    /// 参照实现：两遍朴素统计（独立于 Welford/merge 路径）
    fn naive_stats(prices: &[f64]) -> (usize, f64, f64, f64, f64) {
        let n = prices.len();
        let mean = prices.iter().sum::<f64>() / n as f64;
        let std = (prices.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1) as f64).sqrt();
        (
            n,
            mean,
            std,
            prices.iter().cloned().fold(f64::INFINITY, f64::min),
            prices.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
        )
    }

    #[test]
    fn chunk_plan_partitions_fully_and_contiguously() {
        for &(total, chunk) in &[(10usize, 4usize), (8, 4), (7, 8), (1, 1), (100, 7), (0, 5)] {
            let plan = chunk_plan(total, chunk);
            let mut cursor = 0usize;
            for range in &plan {
                assert_eq!(
                    range.start, cursor,
                    "块须连续衔接 total={total} chunk={chunk}"
                );
                assert!(range.end > range.start, "块非空");
                cursor = range.end;
            }
            assert_eq!(cursor, total, "并集须覆盖全长");
            assert_eq!(
                plan.len(),
                total
                    .div_ceil(chunk.max(1))
                    .max(if total == 0 { 0 } else { 1 })
            );
        }
    }

    #[test]
    fn chunk_plan_single_chunk_when_size_ge_total() {
        let plan = chunk_plan(5, 8);
        assert_eq!(plan, vec![0..5]);
    }

    #[test]
    fn chunk_plan_empty_input_has_no_chunks() {
        assert!(chunk_plan(0, 4).is_empty());
    }

    #[test]
    fn chunk_plan_zero_chunk_size_panics() {
        let result = std::panic::catch_unwind(|| chunk_plan(10, 0));
        assert!(result.is_err(), "chunk_size=0 必须构造期拒绝");
    }

    #[test]
    fn summary_from_slice_matches_naive_stats() {
        let prices = random_walk(5000);
        let summary = Summary::from_slice(&prices);
        let (n, mean, std, min, max) = naive_stats(&prices);
        assert_eq!(summary.count() as usize, n);
        assert!(
            (summary.mean() - mean).abs() < 1e-9,
            "mean {} vs {}",
            summary.mean(),
            mean
        );
        assert!(
            (summary.std_dev() - std).abs() < 1e-9,
            "std {} vs {}",
            summary.std_dev(),
            std
        );
        assert_eq!(summary.min(), min);
        assert_eq!(summary.max(), max);
    }

    #[test]
    fn summary_merge_is_associative_and_commutative() {
        let prices = random_walk(900);
        let (a, b, c) = (&prices[0..300], &prices[300..600], &prices[600..900]);
        let (sa, sb, sc) = (
            Summary::from_slice(a),
            Summary::from_slice(b),
            Summary::from_slice(c),
        );
        let left = sa.merge(&sb).merge(&sc);
        let right = sa.merge(&sb.merge(&sc));
        let swapped = sb.merge(&sa).merge(&sc);
        for (label, x, y) in [
            ("associative", left.mean(), right.mean()),
            ("assoc-std", left.std_dev(), right.std_dev()),
            ("commutative", left.mean(), swapped.mean()),
            ("comm-std", left.std_dev(), swapped.std_dev()),
            ("comm-min", left.min(), swapped.min()),
        ] {
            assert!((x - y).abs() < 1e-12, "{label}: {x} vs {y}");
        }
        let whole = Summary::from_slice(&prices);
        assert!((left.mean() - whole.mean()).abs() < 1e-9);
        assert!((left.std_dev() - whole.std_dev()).abs() < 1e-9);
    }

    #[test]
    fn merge_with_empty_summary_is_identity() {
        let full = Summary::from_slice(&[1.0, 2.0, 3.0]);
        let empty = Summary::empty();
        assert!((full.merge(&empty).mean() - full.mean()).abs() < 1e-12);
        assert!((empty.merge(&full).mean() - full.mean()).abs() < 1e-12);
        assert_eq!(full.merge(&empty).count(), 3);
        assert_eq!(empty.merge(&empty).count(), 0);
    }

    #[test]
    fn empty_summary_has_zero_std_and_guarded_extremes() {
        let empty = Summary::empty();
        assert_eq!(empty.count(), 0);
        assert_eq!(empty.std_dev(), 0.0);
    }

    #[test]
    fn aggregate_matches_single_pass_reference() {
        let prices = random_walk(10_000);
        for chunk_size in [1usize, 64, 512, 10_000, 20_000] {
            let report = stream_aggregate(&prices, chunk_size, &SequentialStreamingExecutor);
            let summary = Summary::from_slice(&prices);
            assert_eq!(report.count, 10_000);
            assert!(
                (report.mean - summary.mean()).abs() < 1e-9,
                "chunk={chunk_size} mean {} vs {}",
                report.mean,
                summary.mean()
            );
            assert!((report.std_dev - summary.std_dev()).abs() < 1e-9);
            assert_eq!(report.min, Some(summary.min()));
            assert_eq!(report.max, Some(summary.max()));
            assert_eq!(report.chunk_count, 10_000usize.div_ceil(chunk_size));
        }
    }

    #[test]
    fn aggregate_empty_input_yields_null_extremes() {
        let report = stream_aggregate(&[], 10, &SequentialStreamingExecutor);
        assert_eq!(report.count, 0);
        assert_eq!(report.min, None, "空输入 min 应为 null 而非 ±inf");
        assert_eq!(report.max, None);
        assert_eq!(report.chunk_count, 0);
    }

    #[test]
    fn streaming_executor_is_object_safe() {
        // 与 platform.rs 契约同款：trait 可 dyn 分发（Worker 分发路径的前提）
        let executors: Vec<&dyn StreamingExecutor> = vec![&SequentialStreamingExecutor];
        let prices = vec![1.0, 2.0, 3.0, 4.0];
        let report = stream_aggregate(&prices, 2, executors[0]);
        assert_eq!(report.count, 4);
        assert_eq!(report.chunk_count, 2);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn threaded_executor_matches_sequential() {
        let prices = random_walk(20_000);
        for chunk_size in [7usize, 333, 4096] {
            let sequential = stream_aggregate(&prices, chunk_size, &SequentialStreamingExecutor);
            for max_threads in [1usize, 2, 8, 64] {
                let threaded = stream_aggregate(
                    &prices,
                    chunk_size,
                    &ThreadedStreamingExecutor { max_threads },
                );
                assert_eq!(
                    sequential, threaded,
                    "chunk={chunk_size} threads={max_threads} 结果须与串行恒等"
                );
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn threaded_executor_handles_degenerate_inputs() {
        let exec = ThreadedStreamingExecutor::with_available_parallelism();
        // 空输入与单块：线程路径与串行路径一致
        assert_eq!(
            stream_aggregate(&[], 5, &exec),
            stream_aggregate(&[], 5, &SequentialStreamingExecutor)
        );
        let prices = vec![7.0; 10];
        assert_eq!(
            stream_aggregate(&prices, 100, &exec),
            stream_aggregate(&prices, 100, &SequentialStreamingExecutor)
        );
        assert_eq!(
            exec.max_threads,
            ThreadedStreamingExecutor::with_available_parallelism().max_threads
        );
    }

    #[test]
    fn stream_engine_matches_batched_indicators_bar_by_bar() {
        let prices = random_walk(600);
        let ind = crate::indicators::TechnicalIndicators::new();
        let (sma_p, ema_p, rsi_p) = (20usize, 12usize, 14usize);

        let sma_batch = ind.calculate_sma(&prices, sma_p);
        let ema_batch = ind.calculate_ema(&prices, ema_p);
        let rsi_batch = ind.calculate_rsi(&prices, rsi_p);

        let mut engine = StreamEngine::new(sma_p, ema_p, rsi_p);
        for i in 0..prices.len() {
            let bar = engine.push(prices[i]);
            // None ↔ 批式 0 填充；有值则逐步相等（同精度舍入口径）
            assert_eq!(
                bar.sma,
                (i + 1 >= sma_p).then(|| { crate::utils::numeric::round_to(sma_batch[i], 4) }),
                "SMA bar {i}: 流式 {:?} vs 批式 {}",
                bar.sma,
                sma_batch[i]
            );
            // EMA 从首 bar 起就有值且逐步相等
            assert!(
                bar.ema.is_some() && (bar.ema.unwrap() - ema_batch[i]).abs() < 1e-9,
                "EMA bar {i}: 流式 {:?} vs 批式 {}",
                bar.ema,
                ema_batch[i]
            );
            assert_eq!(
                bar.rsi,
                (i >= rsi_p).then(|| crate::utils::numeric::round_to(rsi_batch[i], 4)),
                "RSI bar {i}: 流式 {:?} vs 批式 {}",
                bar.rsi,
                rsi_batch[i]
            );
        }
    }

    #[test]
    fn stream_engine_edge_periods_and_flat_series() {
        // 周期 1 退化口径：SMA 即价格本身、RSI 无变化 → avg_loss 0 → 100
        let mut engine = StreamEngine::new(1, 1, 1);
        let bars: Vec<StreamBar> = [100.0, 100.0, 100.0]
            .iter()
            .map(|&p| engine.push(p))
            .collect();
        assert_eq!(bars[0].sma, Some(100.0));
        assert_eq!(bars[2].rsi, Some(100.0), "零损失序列 RSI = 100");
        assert!(bars.iter().all(|b| b.ema.is_some()), "EMA 首价播种即有值");
    }

    #[test]
    fn stream_engine_rejects_zero_period() {
        assert!(std::panic::catch_unwind(|| StreamEngine::new(0, 1, 1)).is_err());
    }
}
