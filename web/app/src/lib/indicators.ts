/**
 * 纯 TS 指标函数（L427 骨架示例，docs/web-framework-selection.md §4）。
 *
 * **契约对齐** `packages/core/src/indicators.rs::TechnicalIndicators::calculate_sma`
 * （`wasm-analyzer` 的 `calculateSMA` 是其薄封装，见 wasm-analyzer/src/lib.rs:101）：
 *   ① 输出与输入**等长**，前 `period - 1` 位为 `0.0` 占位（Rust 侧非 NaN）；
 *   ② `prices.length < period` 时整段为 `0.0`；
 *   ③ 窗口均值按 **4 位小数**取整（`TechnicalIndicators::new()` 的 precision=4）。
 * 真值以 wasm 引擎为准，本模块是「wasm 未构建时的前端降级计算」与单测锚点；
 * 单测直接沿用 Rust 侧同名单测的样本向量，便于两侧对账。
 */

/** 取整精度，与 Rust 侧 `TechnicalIndicators::new()` 的 `precision: 4` 同值 */
export const SMA_PRECISION = 4

/** 对齐 Rust `RoundTo for f64`：`round(v * 10^p) / 10^p` */
function roundTo(v: number, precision: number): number {
  const multiplier = 10 ** precision
  return Math.round(v * multiplier) / multiplier
}

/**
 * 简单移动平均（口径同 Rust 侧 `calculate_sma`）。
 * @param prices 收盘序列
 * @param period 窗口长度，须为正整数（Rust 侧为 `usize`，非法值会 panic；此处显式抛错）
 * @returns 与 `prices` 等长的数组，前 `period - 1` 位为 0.0 占位
 */
export function sma(prices: number[], period: number): number[] {
  if (!Number.isInteger(period) || period <= 0) {
    throw new Error(`period 须为正整数（对齐 Rust usize 口径），收到 ${period}`)
  }
  if (prices.length < period) {
    // Rust: `prices.len() < period` → vec![0.0; prices.len()]（样本不足全 0，不 panic）
    return new Array<number>(prices.length).fill(0)
  }
  const out = new Array<number>(prices.length).fill(0)
  let sum = 0
  for (let i = 0; i < period; i++) sum += prices[i] ?? 0
  out[period - 1] = roundTo(sum / period, SMA_PRECISION)
  // 滑动窗口：与 Rust 相同的加减顺序（sum - 旧 + 新），保证浮点累加一致
  for (let i = period; i < prices.length; i++) {
    sum = sum - (prices[i - period] ?? 0) + (prices[i] ?? 0)
    out[i] = roundTo(sum / period, SMA_PRECISION)
  }
  return out
}

/** 价格格式化：两位小数（行情展示口径，与 wasm 引擎精度无关，仅用于 UI 呈现）。 */
export function fmtPrice(v: number): string {
  return v.toFixed(2)
}