//! 流式 + 并行 A/B 基准（TODO「开发流式数据处理和并行计算机制」验证件）
//!
//! 两组对比：
//! * **流式 vs 批式**：N 根 bar 逐根计算。A = 批式每 bar 重算整段窗口
//!   （`TechnicalIndicators`，O(N·period)）；B = 流式增量（`StreamEngine`，
//!   O(1)/bar）。数值等价由 `packages/core/src/streaming.rs` 单测锁定。
//! * **并行 vs 顺序**：M 个标的批量算同一指标。A = 顺序；B = Rayon 多核
//!   （`alpha_core::parallel`，wasm32 上退化为顺序——见该模块平台策略表）。
//!
//! 运行：cargo run -p alpha-wasm-analyzer --example streaming_bench --release

use alpha_core::indicators::TechnicalIndicators;
use alpha_core::parallel::{self, IndicatorKind};
use alpha_core::streaming::StreamEngine;
use std::time::Instant;

/// LCG 伪随机游走价格序列（确定性、免 rand 依赖）
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

fn bench_streaming() {
    println!("── 流式 vs 批式（逐 bar 推进，20 根 SMA 窗口）──");
    println!(
        "{:>10} {:>6} {:>14} {:>14} {:>10}",
        "bar 数", "迭代", "批式(µs/次)", "流式(µs/次)", "提速"
    );
    // 批式口径是 O(N²)（每 bar 重算整个前缀），N 只能取到万级——
    // 这本身即是结论：长序列下批式根本跑不动，流式才有实用价值。
    for &(n, iters) in &[(2_000_usize, 20_usize), (10_000, 5)] {
        let prices = random_walk(n, 0x5EED);

        // A：批式——每根 bar 用完整前缀重算窗口（实时看板的朴素做法）
        let ind = TechnicalIndicators::new();
        let start = Instant::now();
        let mut sink = 0.0;
        for _ in 0..iters {
            for i in 0..n {
                sink += ind.calculate_sma(&prices[..=i], 20)[i];
            }
        }
        let batch = start.elapsed().as_secs_f64() * 1e6 / iters as f64;

        // B：流式——单 bar O(1) 增量（迭代数对齐，便于逐次对比）
        let start = Instant::now();
        for _ in 0..iters {
            let mut engine = StreamEngine::new(20, 12, 14);
            for &p in &prices {
                sink += engine.push(p).sma.unwrap_or(0.0);
            }
        }
        let stream = start.elapsed().as_secs_f64() * 1e6 / iters as f64;
        std::hint::black_box(sink);

        let speedup = batch / stream;
        println!("{n:>10} {iters:>6} {batch:>14.2} {stream:>14.2} {speedup:>9.0}x");
    }
    println!();
}

fn bench_parallel() {
    let threads = parallel::available_parallelism();
    println!("── 并行 vs 顺序（Rayon 多核，批量 SMA(20)）──");
    println!("并行度: {threads} 线程");
    println!(
        "{:>8} {:>8} {:>8} {:>14} {:>14} {:>10}",
        "标的数", "序列长", "迭代", "顺序(µs/次)", "并行(µs/次)", "加速"
    );
    for &(count, len, iters) in &[
        (16_usize, 5_000_usize, 20_usize),
        (64, 5_000, 10),
        (16, 50_000, 5),
    ] {
        let datasets: Vec<Vec<f64>> = (0..count)
            .map(|i| random_walk(len, 0xBEEF + i as u64 * 7919))
            .collect();

        let start = Instant::now();
        for _ in 0..iters {
            std::hint::black_box(parallel::compute_sequential(
                &datasets,
                IndicatorKind::Sma,
                20,
            ));
        }
        let seq = start.elapsed().as_secs_f64() * 1e6 / iters as f64;

        let start = Instant::now();
        for _ in 0..iters {
            std::hint::black_box(parallel::compute(&datasets, IndicatorKind::Sma, 20));
        }
        let par = start.elapsed().as_secs_f64() * 1e6 / iters as f64;

        let speedup = seq / par;
        println!("{count:>8} {len:>8} {iters:>8} {seq:>14.2} {par:>14.2} {speedup:>9.2}x");
    }
    println!();
}

fn main() {
    println!("流式 + 并行基准（native 口径，release）");
    println!();
    bench_streaming();
    bench_parallel();
    println!("说明：流式组比较「每 bar 重算窗口」与「O(1) 增量」，数值等价由单测锁定；");
    println!("      并行组为 Rayon 真实多核（wasm32 无线程，该路径退化为顺序，见 parallel.rs）；");
    println!("      数值随机器波动，绝对值仅供对比参考（记录于 TODO L89 注与 commit message）。");
}
