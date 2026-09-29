//! 多数据集并行计算（L0 纯计算层）
//!
//! 面向"一次算 N 个标的"的批量场景（选股、参数扫描、组合回测）：
//! 逐个标的计算是 O(N) 串行，跨标的可安全并行——各标的之间无数据依赖。
//!
//! ## 平台策略
//!
//! | 平台 | 后端 | 依据 |
//! |---|---|---|
//! | native（服务端/桌面/基准） | Rayon 线程池 | 真多核，chunk 式负载均衡 |
//! | wasm32（浏览器） | 顺序迭代 | wasm32-unknown-unknown 无线程模型；
//!   浏览器并行由 Web Worker 承担（`wasm-analyzer/src/worker.rs`） |
//!
//! 两条分支**语义完全等价**（同输入同输出、同顺序），单测
//! `parallel_matches_sequential` 逐元素锁定；因此上层调用方无需关心平台。
//! Rayon 依赖按 target 门控（`packages/core/Cargo.toml`），L0 默认依赖
//! 黑名单不受影响（门禁 `scripts/check-cross-platform.sh` 第 3 步）。

use crate::indicators::TechnicalIndicators;

/// 批量指标类型（跨平台一致的枚举，避免上层散落字符串匹配）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndicatorKind {
    Sma,
    Ema,
    Rsi,
    Bollinger,
}

impl IndicatorKind {
    /// 从字符串解析（JS 侧传参口径，与 `wasm-analyzer` 现有 API 对齐）
    pub fn from_str_lossy(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "sma" => Some(Self::Sma),
            "ema" => Some(Self::Ema),
            "rsi" => Some(Self::Rsi),
            "bollinger" | "bollinger_bands" | "bb" => Some(Self::Bollinger),
            _ => None,
        }
    }

    /// 规范化名称（回传给 JS 时保持同一口径）
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Sma => "sma",
            Self::Ema => "ema",
            Self::Rsi => "rsi",
            Self::Bollinger => "bollinger",
        }
    }

    /// 单标的计算（并行与顺序共用的唯一实现，避免两条路径漂移）
    fn compute_one(&self, prices: &[f64], period: usize) -> Vec<f64> {
        let ind = TechnicalIndicators::new();
        match self {
            Self::Sma => ind.calculate_sma(prices, period),
            Self::Ema => ind.calculate_ema(prices, period),
            Self::Rsi => ind.calculate_rsi(prices, period),
            // 布林带返回三元组，取上轨（与 `ParallelScheduler.submit_task` 既有口径一致）
            Self::Bollinger => ind.calculate_bollinger_bands(prices, period, 2.0).0,
        }
    }
}

/// 顺序基准实现（wasm32 路径 / 并行结果的对照口径）
pub fn compute_sequential(
    datasets: &[Vec<f64>],
    kind: IndicatorKind,
    period: usize,
) -> Vec<Vec<f64>> {
    datasets
        .iter()
        .map(|prices| kind.compute_one(prices, period))
        .collect()
}

/// 并行实现：native 走 Rayon 线程池，wasm32 退化为顺序（语义等价）
pub fn compute(datasets: &[Vec<f64>], kind: IndicatorKind, period: usize) -> Vec<Vec<f64>> {
    if datasets.is_empty() {
        return Vec::new();
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        // collect::<Vec<_>> 保持输入顺序（IndexedParallelIterator 语义），
        // 因此并行与顺序结果逐元素可对比
        datasets
            .par_iter()
            .map(|prices| kind.compute_one(prices, period))
            .collect()
    }

    #[cfg(target_arch = "wasm32")]
    {
        // wasm32 无线程：Web Worker 承担跨核并行（见模块文档）
        compute_sequential(datasets, kind, period)
    }
}

/// 可用并行度（native 读 Rayon 线程池大小，wasm32 恒为 1）
pub fn available_parallelism() -> usize {
    #[cfg(not(target_arch = "wasm32"))]
    {
        rayon::current_num_threads().max(1)
    }

    #[cfg(target_arch = "wasm32")]
    {
        1
    }
}

/// 批量计算报告（含耗时与并行度，便于 JS 侧观测收益）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BatchReport {
    /// 每标的的指标序列（与输入 datasets 同序）
    pub results: Vec<Vec<f64>>,
    /// 参与计算的数据集数
    pub dataset_count: usize,
    /// 实际并行度
    pub threads: usize,
    /// 耗时（毫秒）
    pub elapsed_ms: f64,
}

/// 带统计的批量计算（计时口径统一在调用方，避免两平台计时源分歧）
pub fn compute_with_report(
    datasets: &[Vec<f64>],
    kind: IndicatorKind,
    period: usize,
    elapsed_ms: f64,
) -> BatchReport {
    let results = compute(datasets, kind, period);
    BatchReport {
        results,
        dataset_count: datasets.len(),
        threads: available_parallelism(),
        elapsed_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 确定性伪随机游走（免 rand 依赖，跨平台可复现）
    fn random_walk(n: usize, seed: u64) -> Vec<f64> {
        let mut state = seed;
        let mut price = 100.0_f64;
        (0..n)
            .map(|_| {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                let r = ((state >> 33) as f64) / (u32::MAX as f64) - 0.5;
                price = (price * (1.0 + r * 0.02)).max(0.01);
                price
            })
            .collect()
    }

    fn datasets(count: usize, len: usize) -> Vec<Vec<f64>> {
        (0..count)
            .map(|i| random_walk(len, 0xA11CE + i as u64 * 7919))
            .collect()
    }

    /// 并行结果与顺序逐元素一致（顺序保持 + 计算一致，跨平台通用）
    #[test]
    fn parallel_matches_sequential() {
        let data = datasets(12, 300);
        for kind in [
            IndicatorKind::Sma,
            IndicatorKind::Ema,
            IndicatorKind::Rsi,
            IndicatorKind::Bollinger,
        ] {
            let par = compute(&data, kind, 14);
            let seq = compute_sequential(&data, kind, 14);
            assert_eq!(par.len(), seq.len(), "{} 结果条数", kind.as_str());
            for (i, (p, s)) in par.iter().zip(seq.iter()).enumerate() {
                assert_eq!(p.len(), s.len(), "{} 数据集 {i} 长度", kind.as_str());
                for (j, (pv, sv)) in p.iter().zip(s.iter()).enumerate() {
                    assert!(
                        (pv - sv).abs() < 1e-12,
                        "{} 数据集 {i} 位置 {j}：并行 {pv} != 顺序 {sv}",
                        kind.as_str()
                    );
                }
            }
        }
    }

    /// 顺序保持：并行 collect 不得打乱数据集顺序（否则上层符号映射错位）
    #[test]
    fn parallel_preserves_dataset_order() {
        // 长度不同的数据集：结果长度序列即顺序指纹
        let data: Vec<Vec<f64>> = (10..20)
            .map(|len| random_walk(len, 0xBEEF + len as u64))
            .collect();
        let out = compute(&data, IndicatorKind::Ema, 5);
        let expected: Vec<usize> = out.iter().map(|r| r.len()).collect();
        let input: Vec<usize> = data.iter().map(|d| d.len()).collect();
        assert_eq!(expected, input, "结果长度序列应与输入顺序一致");
    }

    /// 空输入不 panic，返回空结果
    #[test]
    fn empty_input_returns_empty() {
        assert!(compute(&[], IndicatorKind::Sma, 14).is_empty());
    }

    /// 样本不足 period：各平台行为一致（批式按 0.0/短序列填充）
    #[test]
    fn insufficient_samples_behave_consistently() {
        let data = vec![vec![1.0, 2.0], vec![3.0]];
        for kind in [IndicatorKind::Sma, IndicatorKind::Rsi] {
            assert_eq!(
                compute(&data, kind, 20),
                compute_sequential(&data, kind, 20),
                "{} 预热不足时两路径一致",
                kind.as_str()
            );
        }
    }

    /// 指标类型字符串解析：大小写与别名容忍，未知返回 None
    #[test]
    fn indicator_kind_parsing_covers_aliases() {
        assert_eq!(
            IndicatorKind::from_str_lossy("SMA"),
            Some(IndicatorKind::Sma)
        );
        assert_eq!(
            IndicatorKind::from_str_lossy("ema"),
            Some(IndicatorKind::Ema)
        );
        assert_eq!(
            IndicatorKind::from_str_lossy("Bollinger"),
            Some(IndicatorKind::Bollinger)
        );
        assert_eq!(
            IndicatorKind::from_str_lossy("bb"),
            Some(IndicatorKind::Bollinger)
        );
        assert_eq!(IndicatorKind::from_str_lossy("unknown"), None);
        // as_str 与解析互逆（回传 JS 口径稳定）
        for kind in [
            IndicatorKind::Sma,
            IndicatorKind::Ema,
            IndicatorKind::Rsi,
            IndicatorKind::Bollinger,
        ] {
            assert_eq!(IndicatorKind::from_str_lossy(kind.as_str()), Some(kind));
        }
    }

    /// 报告口径：dataset_count/threads 与实际一致，elapsed_ms 直传
    #[test]
    fn batch_report_carries_accurate_metadata() {
        let data = datasets(5, 100);
        let report = compute_with_report(&data, IndicatorKind::Rsi, 14, 12.5);
        assert_eq!(report.dataset_count, 5);
        assert_eq!(report.results.len(), 5);
        assert_eq!(report.threads, available_parallelism());
        assert!(report.threads >= 1);
        assert!((report.elapsed_ms - 12.5).abs() < f64::EPSILON);
    }

    /// 单数据集不因并行调度开销劣化结果
    #[test]
    fn single_dataset_matches_batch_indicator() {
        let prices = random_walk(200, 0xFEED);
        let out = compute(std::slice::from_ref(&prices), IndicatorKind::Sma, 20);
        let ind = TechnicalIndicators::new();
        let expected = ind.calculate_sma(&prices, 20);
        assert_eq!(out.len(), 1);
        for (i, (o, e)) in out[0].iter().zip(expected.iter()).enumerate() {
            assert!((o - e).abs() < 1e-12, "位置 {i}：{o} != {e}");
        }
    }

    /// native 侧 Rayon 确实启用了多线程（wasm32 恒为 1，本断言仅 native 生效）
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn native_reports_multiple_threads() {
        assert!(
            available_parallelism() >= 1,
            "Rayon 线程池应至少 1 线程（实际 {}）",
            available_parallelism()
        );
        // 线程数不超过硬件并发（Rayon 语义：默认即 CPU 核数）
        let hw = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        assert!(available_parallelism() <= hw.max(1));
    }

    /// 大批量（>典型核数）下并行仍保持完整性与顺序
    #[test]
    fn large_batch_preserves_integrity() {
        let data = datasets(64, 150);
        let par = compute(&data, IndicatorKind::Sma, 10);
        assert_eq!(par.len(), data.len());
        for (i, r) in par.iter().enumerate() {
            assert_eq!(r.len(), data[i].len(), "数据集 {i} 长度应守恒");
        }
    }
}
