import { describe, expect, it } from 'vitest'
import { fmtPrice, sma } from './indicators'

/**
 * 契约锚点：与 `packages/core/src/indicators.rs` 的 `calculate_sma` 同口径
 * （等长输出 / 前导 0.0 占位 / 样本不足全 0 / 4 位小数取整），
 * 样本向量沿用 Rust 侧单测 `test_sma_calculation`（docs/web-framework-selection.md §5）。
 */
describe('sma（口径对齐 Rust calculate_sma）', () => {
  it('Rust 同名样本：vec![1,2,3,4,5] period 3 → sma[2]=2.0 sma[3]=3.0 sma[4]=4.0', () => {
    const out = sma([1, 2, 3, 4, 5], 3)
    expect(out[2]).toBe(2.0)
    expect(out[3]).toBe(3.0)
    expect(out[4]).toBe(4.0)
  })

  it('输出与输入等长（Rust 返回同长 Vec，非截断窗口）', () => {
    const prices = [1, 2, 3, 4, 5]
    expect(sma(prices, 3)).toHaveLength(prices.length)
    expect(sma(prices, 1)).toHaveLength(prices.length)
  })

  it('前 period-1 位为 0.0 占位（非 NaN、非窗口值）', () => {
    const out = sma([1, 2, 3, 4, 5], 3)
    expect(out.slice(0, 2)).toEqual([0, 0])
    expect(out.every((v) => Number.isFinite(v))).toBe(true)
    expect(out.some(Number.isNaN)).toBe(false)
  })

  it('样本不足 period：等长全 0（Rust vec![0.0; len]，不 panic、不抛错）', () => {
    expect(sma([1, 2], 3)).toEqual([0, 0])
    expect(sma([], 3)).toEqual([])
  })

  it('4 位小数取整（Rust precision=4）：4/3 → 1.3333', () => {
    expect(sma([1, 1, 2, 3], 3)).toEqual([0, 0, 1.3333, 2])
  })

  it('滑动窗口：末值等于末 period 根均值', () => {
    const prices = [10, 11, 12, 13, 14, 15]
    const out = sma(prices, 4)
    expect(out[out.length - 1]).toBeCloseTo((12 + 13 + 14 + 15) / 4, 10)
  })

  it('period 非法：非正整数抛错（Rust 侧 usize 会 panic，此处显式拒绝）', () => {
    expect(() => sma([1, 2, 3], 0)).toThrow('正整数')
    expect(() => sma([1, 2, 3], 1.5)).toThrow('正整数')
    expect(() => sma([1, 2, 3], -2)).toThrow('正整数')
  })
})

describe('fmtPrice', () => {
  it('固定两位小数（含进位展示口径）', () => {
    expect(fmtPrice(90.5)).toBe('90.50')
    expect(fmtPrice(1234.567)).toBe('1234.57')
    expect(fmtPrice(0)).toBe('0.00')
  })
})