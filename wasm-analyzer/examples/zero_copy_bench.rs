//! 零拷贝 A/B 基准（native 近似口径，TODO「实现零拷贝内存管理」验证件）
//!
//! A（修复前口径）：每次调用先 `to_vec()` 拷入再计算——对应 wasm 便捷接口
//!   `backtestSmaCross` 内部路径；
//! B（修复后口径）：`SharedF64Buffer` 切片直读零拷贝——对应 `allocPriceBuffer`
//!   + `backtestSmaCrossPtr` 组合（JS ingress 一次写入后跨调用复用）。
//!
//! 运行：cargo run -p alpha-wasm-analyzer --example zero_copy_bench --release
//! 真实 pkg 产物（JS 边界）口径见 node-bench/zero-copy.mjs。

use alpha_core::backtest::{BacktestEngine, SmaCrossStrategy};
use alpha_wasm_analyzer::SharedF64Buffer;
use std::time::Instant;

/// LCG 伪随机游走价格序列（确定性、免 rand 依赖）
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

fn main() {
    println!("序列长度   迭代  拷入路径(µs/次)  零拷贝路径(µs/次)  节省   每次消除 memcpy");
    for &(n, iters) in &[(10_000_usize, 400_usize), (100_000, 100)] {
        let prices = random_walk(n);

        // A：修复前口径——每次调用 to_vec 拷入（memcpy n*8 字节 + 堆分配）
        let start = Instant::now();
        for _ in 0..iters {
            let owned = prices.to_vec();
            let mut strategy = SmaCrossStrategy::new(1, 20);
            let _ = BacktestEngine::new(0.0).run(&owned, &mut strategy);
        }
        let with_copy = start.elapsed().as_secs_f64() * 1e6 / iters as f64;

        // B：修复后口径——共享缓冲切片直读（ingress 一次写入，计算零拷贝）
        let mut buf = SharedF64Buffer::alloc(n);
        buf.as_mut_slice().copy_from_slice(&prices);
        let start = Instant::now();
        for _ in 0..iters {
            let mut strategy = SmaCrossStrategy::new(1, 20);
            let _ = BacktestEngine::new(0.0).run(buf.as_slice(), &mut strategy);
        }
        let zero_copy = start.elapsed().as_secs_f64() * 1e6 / iters as f64;

        let saved = (with_copy - zero_copy) / with_copy * 100.0;
        let memcpy_kb = (n * 8) as f64 / 1024.0;
        println!(
            "{:>8} {:>6} {:>13.2} {:>15.2} {:>12.1}% {:>7.0} KB",
            n, iters, with_copy, zero_copy, saved, memcpy_kb
        );
    }
    println!();
    println!("说明：A-B 差值即每次调用消除的 to_vec memcpy + 堆分配开销；引擎本体计算两者同口径。");
    println!("      数值随机器波动，绝对值仅供对比参考（记录于 TODO L54 注与 commit message）。");
}
