import { describe, expect, it } from 'vitest'
import { demoCandles, generateCandles, tradingDates } from './demoWalk'

describe('tradingDates（交易日序列）', () => {
  it('起点 2026-01-05，升序且跳过周六日', () => {
    const dates = tradingDates(30)
    expect(dates[0]).toBe('2026-01-05')
    for (let i = 1; i < dates.length; i++) {
      expect(dates[i]! > dates[i - 1]!).toBe(true)
    }
    for (const t of dates) {
      const d = new Date(`${t}T00:00:00Z`)
      const day = d.getUTCDay()
      expect(day === 0 || day === 6).toBe(false)
    }
  })
})

describe('generateCandles（确定性合成日 K）', () => {
  it('同种子两次生成逐位一致（LCG 确定性，可复现）', () => {
    expect(generateCandles(30, 42)).toEqual(generateCandles(30, 42))
  })

  it('K 线不变量：high ≥ max(open,close) ≥ min(open,close) ≥ low > 0，全字段有限', () => {
    for (const c of generateCandles(200, 7)) {
      expect(c.high).toBeGreaterThanOrEqual(Math.max(c.open, c.close))
      expect(c.low).toBeLessThanOrEqual(Math.min(c.open, c.close))
      expect(c.high).toBeGreaterThanOrEqual(c.low)
      expect(c.close).toBeGreaterThan(0)
      for (const v of [c.open, c.high, c.low, c.close]) {
        expect(Number.isFinite(v)).toBe(true)
      }
    }
  })

  it('demoCandles：60 根，时间升序，OHLC 两位小数以内', () => {
    expect(demoCandles).toHaveLength(60)
    expect(demoCandles[0]!.time).toBe('2026-01-05')
    for (const c of demoCandles) {
      expect(c.time).toMatch(/^\d{4}-\d{2}-\d{2}$/)
      for (const v of [c.open, c.high, c.low, c.close]) {
        // round2 口径：数值精确到两位小数（无第三位浮点尾数）
        expect(Math.abs(v * 100 - Math.round(v * 100))).toBeLessThan(1e-9)
      }
    }
  })
})
