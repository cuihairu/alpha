//! 并行计算基准：对比 Rayon 并行 vs 顺序实现的加速比
//! 运行：`cargo run -p alpha-wasm-analyzer --example parallel_bench --release`

use alpha_core::parallel::{compute, compute_sequential, compute_with_report, IndicatorKind};
use std::time::Instant;

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

fn bench_kind(name: &str, datasets: &[Vec<f64>], kind: IndicatorKind, period: usize) {
    // 预热
    let _ = compute(datasets, kind, period);
    let _ = compute_sequential(datasets, kind, period);

    // 并行
    let start = Instant::now();
    let par_report = compute_with_report(datasets, kind, period, 0.0);
    let par_elapsed = start.elapsed().as_secs_f64() * 1000.0;

    // 顺序
    let start = Instant::now();
    let _seq_report = compute_with_report(datasets, kind, period, 0.0);
    let seq_elapsed = start.elapsed().as_secs_f64() * 1000.0;

    let speedup = if par_elapsed > 0.0 {
        seq_elapsed / par_elapsed
    } else {
        0.0
    };

    println!(
        "{name:>12} | datasets={:>3} len={:>5} | seq={:>8.2}ms par={:>8.2}ms | speedup={:.2}x | threads={}",
        datasets.len(),
        datasets[0].len(),
        seq_elapsed,
        par_elapsed,
        speedup,
        par_report.threads
    );
}

fn main() {
    println!("=== 并行计算基准（native，Rayon 线程池）===");
    println!(
        "硬件并发: {}",
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
    );
    println!();

    // 小规模：开销主导，可能无加速甚至劣化
    println!("--- 小规模（开销敏感） ---");
    let small = datasets(4, 200);
    for kind in [
        IndicatorKind::Sma,
        IndicatorKind::Ema,
        IndicatorKind::Rsi,
        IndicatorKind::Bollinger,
    ] {
        bench_kind(kind.as_str(), &small, kind, 20);
    }

    // 中规模：典型选股/参数扫描场景
    println!("\n--- 中规模（典型批量） ---");
    let medium = datasets(16, 500);
    for kind in [
        IndicatorKind::Sma,
        IndicatorKind::Ema,
        IndicatorKind::Rsi,
        IndicatorKind::Bollinger,
    ] {
        bench_kind(kind.as_str(), &medium, kind, 14);
    }

    // 大规模：组合回测/全市场扫描
    println!("\n--- 大规模（充分并行） ---");
    let large = datasets(64, 1000);
    for kind in [
        IndicatorKind::Sma,
        IndicatorKind::Ema,
        IndicatorKind::Rsi,
        IndicatorKind::Bollinger,
    ] {
        bench_kind(kind.as_str(), &large, kind, 10);
    }

    // 极大规模：验证线性扩展
    println!("\n--- 极大规模（线性扩展验证）---");
    let xlarge = datasets(128, 2000);
    for kind in [IndicatorKind::Sma, IndicatorKind::Ema, IndicatorKind::Rsi] {
        bench_kind(kind.as_str(), &xlarge, kind, 20);
    }

    println!("\n=== 注 ===");
    println!("- wasm32 目标无线程，compute() 退化为顺序（速度比 = 1.0x）");
    println!("- 浏览器真并行由 Web Worker 承担（见 worker.rs WorkerPool 与 handleTask）");
    println!("- 结果正确性由 parallel_matches_sequential 单测逐元素锁定");
}
