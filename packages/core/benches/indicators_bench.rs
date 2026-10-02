//! alpha-core 性能基准套件（L492，criterion；L495 的回归检测以
//! `--save-baseline` 比对本套件数字）。
//!
//! 覆盖面：热路径指标（SMA/EMA/RSI/Bollinger/MACD）、形态识别
//! （K 线形态 + zigzag 摆动）、优化引擎（小网格 grid_search）。
//! 价格序列用确定性 LCG 生成（同 simd.rs 测试口径，无 rand 依赖、
//! 可复现）。跑法：`cargo bench -p alpha-core`。

use alpha_core::backtest::{BacktestEngine, SmaCrossStrategy};
use alpha_core::indicators::TechnicalIndicators;
use alpha_core::optimize::{grid_search, ParamAxis};
use alpha_core::patterns::{detect_candle_patterns, find_swings, Bar};
use criterion::{black_box, criterion_group, criterion_main, Criterion};

/// 确定性价格发生器：LCG 均匀游走（种子 42，可复现）
fn gen_prices(n: usize) -> Vec<f64> {
    let mut state: u32 = 42;
    let mut price = 100.0;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        // (state 归一化到 [-0.5%, +0.5%]) 的单步收益
        let step = (state % 10_000) as f64 / 10_000.0 - 0.5;
        price *= 1.0 + step / 100.0;
        out.push(price);
    }
    out
}

fn bench_indicators(c: &mut Criterion) {
    let ti = TechnicalIndicators::new();
    let prices_10k = gen_prices(10_000);

    let mut group = c.benchmark_group("indicators");
    group.throughput(criterion::Throughput::Elements(prices_10k.len() as u64));
    group.bench_function("sma_10k_p20", |b| {
        b.iter(|| ti.calculate_sma(black_box(&prices_10k), black_box(20)))
    });
    group.bench_function("ema_10k_p20", |b| {
        b.iter(|| ti.calculate_ema(black_box(&prices_10k), black_box(20)))
    });
    group.bench_function("rsi_10k_p14", |b| {
        b.iter(|| ti.calculate_rsi(black_box(&prices_10k), black_box(14)))
    });
    group.bench_function("bollinger_10k_p20", |b| {
        b.iter(|| {
            ti.calculate_bollinger_bands(black_box(&prices_10k), black_box(20), black_box(2.0))
        })
    });
    group.bench_function("macd_10k", |b| {
        b.iter(|| {
            ti.calculate_macd(
                black_box(&prices_10k),
                black_box(12),
                black_box(26),
                black_box(9),
            )
        })
    });
    group.finish();
}

fn bench_patterns(c: &mut Criterion) {
    let prices_10k = gen_prices(10_000);
    let bars: Vec<Bar> = prices_10k
        .iter()
        .enumerate()
        .map(|(i, &close)| {
            let open = if i == 0 { close } else { prices_10k[i - 1] };
            Bar {
                open,
                high: close.max(open) * 1.002,
                low: close.min(open) * 0.998,
                close,
            }
        })
        .collect();

    let mut group = c.benchmark_group("patterns");
    group.throughput(criterion::Throughput::Elements(prices_10k.len() as u64));
    group.bench_function("candle_patterns_10k", |b| {
        b.iter(|| detect_candle_patterns(black_box(&bars), black_box(0.1)))
    });
    group.bench_function("find_swings_10k", |b| {
        b.iter(|| find_swings(black_box(&prices_10k), black_box(0.02)))
    });
    group.finish();
}

fn bench_optimize(c: &mut Criterion) {
    // 小网格（2×3=6 组合 × 训练段 800 根）：寻优编排开销的代表面
    let prices = gen_prices(1_000);
    let engine = BacktestEngine::new(2.0);
    let mut group = c.benchmark_group("optimize");
    group.bench_function("grid_search_2x3_1k", |b| {
        b.iter(|| {
            let axes = [
                ParamAxis {
                    name: "fast".to_string(),
                    values: vec![3, 5],
                },
                ParamAxis {
                    name: "slow".to_string(),
                    values: vec![20, 40, 60],
                },
            ];
            let mut factory = |params: &[usize]| {
                Box::new(SmaCrossStrategy::new(params[0], params[1]))
                    as Box<dyn alpha_core::backtest::Strategy>
            };
            grid_search(black_box(&prices), &engine, &axes, &mut factory, 0.8).is_ok()
        })
    });
    group.finish();
}

criterion_group!(benches, bench_indicators, bench_patterns, bench_optimize);
criterion_main!(benches);
