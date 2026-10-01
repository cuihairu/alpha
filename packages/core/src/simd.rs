//! SIMD 优化的向量化计算（L452）。
//!
//! 路线：**显式 SIMD 与 portable lane 分解共享同一数学语义**。
//! `sum_f64`/`dot_f64` 的核心是「4 lane 部分和」：
//! 元素按下标 `i % 4` 累入 4 个独立部分和（lane 内保持顺序结合），
//! 最后按 `(l0+l2)+(l1+l3)` 水平归约。
//! - x86_64 且运行时检测到 AVX2 → `_mm256_add_pd` 一次吃 4 个 f64；
//! - 其余平台（wasm32/aarch64/无 AVX2 主机）→ [`sum_lane_reference`]
//!   纯 Rust 同顺序实现（LLVM 自动向量化兜底）。
//!
//! 两条路径按位一致（同 lane 分配、同归约顺序），单测用
//! lane 参考实现锁定位级等价；与严格顺序 naive 求和的差异仅在
//! 浮点结合序（相对误差 ULP 级，容差断言）。
//!
//! 舱位豁免（L463 安全审计）：本模块是 alpha-core 两个 `#![allow(unsafe_code)]`
//! 豁免点之一（target_feature intrinsics），`lib.rs` 的 `#![deny(unsafe_code)]`
//! 下新增 unsafe 仅限此处且需同步核对 [`crate::safety_audit`] 预算。
//!
//! NaN 语义：加法/乘法 NaN 传播与标量一致，安全；比较类运算
//! （min/max）x86 SIMD 指令与 `f64::min` 的 NaN 行为不同，
//! 故 [`min_max_f64`] 只用 portable lane 分解，不引入指令级语义差异。

#![allow(unsafe_code)]

/// 4 lane 部分和的 portable 参考实现（SIMD 路径的语义基准，
/// 同时是无 AVX2 平台的实际执行体）
pub fn sum_lane_reference(values: &[f64]) -> f64 {
    let mut lanes = [0.0f64; 4];
    for (i, v) in values.iter().enumerate() {
        lanes[i % 4] += v;
    }
    (lanes[0] + lanes[2]) + (lanes[1] + lanes[3])
}

/// 同构 4 lane 乘加（点积 lane 参考）
pub fn dot_lane_reference(a: &[f64], b: &[f64]) -> f64 {
    let mut lanes = [0.0f64; 4];
    for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
        lanes[i % 4] += x * y;
    }
    (lanes[0] + lanes[2]) + (lanes[1] + lanes[3])
}

/// 求和：AVX2 可用时 4-f64 向量加，否则 lane 参考
pub fn sum_f64(values: &[f64]) -> f64 {
    #[cfg(target_arch = "x86_64")]
    {
        if std::arch::is_x86_feature_detected!("avx2") {
            // 安全性：函数体仅做对齐无关的 loadu/storeu 与算术，
            // 输入为合法 f64 切片；target_feature 函数须 unsafe 调用
            return unsafe { sum_avx2(values) };
        }
    }
    sum_lane_reference(values)
}

/// 点积：长度不等返回错误；空切片为 0.0
pub fn dot_f64(a: &[f64], b: &[f64]) -> crate::errors::AlphaResult<f64> {
    if a.len() != b.len() {
        return Err(crate::errors::AlphaError::InvalidInput(format!(
            "点积长度不等：{} vs {}",
            a.len(),
            b.len()
        )));
    }
    #[cfg(target_arch = "x86_64")]
    {
        if std::arch::is_x86_feature_detected!("avx2") {
            return Ok(unsafe { dot_avx2(a, b) });
        }
    }
    Ok(dot_lane_reference(a, b))
}

/// 最小/最大值（portable：f64::min/max 语义，NaN 被忽略——
/// 与 `f64::min` 一致；空切片返回 None）
pub fn min_max_f64(values: &[f64]) -> Option<(f64, f64)> {
    let first = *values.first()?;
    let mut min = first;
    let mut max = first;
    for v in values {
        min = min.min(*v);
        max = max.max(*v);
    }
    Some((min, max))
}

// ---------------------------------------------------------------------------
// x86_64 AVX2 路径（运行时检测后才进入；非 x86_64 目标整体剔除）
// ---------------------------------------------------------------------------

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn sum_avx2(values: &[f64]) -> f64 {
    use std::arch::x86_64::*;

    let mut acc = _mm256_setzero_pd();
    let mut chunks = values.chunks_exact(4);
    for chunk in &mut chunks {
        acc = _mm256_add_pd(acc, _mm256_loadu_pd(chunk.as_ptr()));
    }
    // 尾部余数按 lane 分配：remainder[j] 的全局下标 = 4k + j → lane j，
    // 与 lane 参考 `i % 4` 完全对齐（否则浮点结合序差异破坏位级一致）
    let mut lanes = [0.0f64; 4];
    _mm256_storeu_pd(lanes.as_mut_ptr(), acc);
    for (j, v) in chunks.remainder().iter().enumerate() {
        lanes[j] += v;
    }
    // 水平归约：与 lane 参考同序 (l0+l2)+(l1+l3)
    (lanes[0] + lanes[2]) + (lanes[1] + lanes[3])
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn dot_avx2(a: &[f64], b: &[f64]) -> f64 {
    use std::arch::x86_64::*;

    let mut acc = _mm256_setzero_pd();
    let mut a_chunks = a.chunks_exact(4);
    let mut b_chunks = b.chunks_exact(4);
    for (ca, cb) in (&mut a_chunks).zip(&mut b_chunks) {
        let x = _mm256_loadu_pd(ca.as_ptr());
        let y = _mm256_loadu_pd(cb.as_ptr());
        acc = _mm256_add_pd(acc, _mm256_mul_pd(x, y));
    }
    let mut lanes = [0.0f64; 4];
    _mm256_storeu_pd(lanes.as_mut_ptr(), acc);
    for (j, (x, y)) in a_chunks
        .remainder()
        .iter()
        .zip(b_chunks.remainder())
        .enumerate()
    {
        lanes[j] += x * y;
    }
    (lanes[0] + lanes[2]) + (lanes[1] + lanes[3])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 确定性 LCG（复现性门禁禁 Math.random/unsafe rand）
    fn lcg_series(seed: u64, len: usize) -> Vec<f64> {
        let mut state = seed;
        (0..len)
            .map(|_| {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                ((state >> 33) % 2_000_000) as f64 / 1e4 - 100.0
            })
            .collect()
    }

    #[test]
    fn sum_matches_lane_reference_bitwise_across_lengths() {
        // 尾部余数路径（长度非 4 倍数）也必须与 lane 参考位级一致
        for len in [0usize, 1, 3, 4, 5, 7, 8, 13, 64, 1000] {
            let values = lcg_series(42 + len as u64, len);
            assert_eq!(
                sum_f64(values.as_slice()),
                sum_lane_reference(&values),
                "len={len} 位级不一致"
            );
        }
    }

    #[test]
    fn sum_within_ulps_of_naive_order() {
        let values = lcg_series(2026_1002, 10_000);
        let naive: f64 = values.iter().sum();
        let simd = sum_f64(&values);
        let diff = (simd - naive).abs();
        let scale = naive.abs().max(1.0);
        assert!(
            diff / scale < 1e-9,
            "lane 序与顺序和差异过大: {simd} vs {naive}"
        );
    }

    #[test]
    fn dot_matches_lane_reference_and_rejects_length_mismatch() {
        let a = lcg_series(7, 333);
        let b = lcg_series(99, 333);
        assert_eq!(dot_f64(&a, &b).unwrap(), dot_lane_reference(&a, &b));

        let naive: f64 = a.iter().zip(&b).map(|(x, y)| x * y).sum();
        assert!((dot_f64(&a, &b).unwrap() - naive).abs() / naive.abs().max(1.0) < 1e-9);

        assert!(dot_f64(&a[..3], &b).is_err(), "长度不等必须报错");
        assert_eq!(dot_f64(&[], &[]).unwrap(), 0.0);
    }

    #[test]
    fn min_max_matches_naive_and_handles_edges() {
        let values = lcg_series(5, 257);
        let naive_min = values.iter().copied().fold(f64::INFINITY, f64::min);
        let naive_max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        assert_eq!(min_max_f64(&values), Some((naive_min, naive_max)));

        assert_eq!(min_max_f64(&[]), None);
        assert_eq!(min_max_f64(&[3.5]), Some((3.5, 3.5)));
        // NaN 沿 f64::min/max 语义被忽略
        assert_eq!(min_max_f64(&[f64::NAN, 1.0, 2.0]), Some((1.0, 2.0)));
    }
}
