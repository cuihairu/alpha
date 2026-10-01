/**
 * 图表演示数据源（L429）：确定性合成的日 K 线序列。
 * 用 LCG 种子伪随机而非 Math.random——同一构建/测试永远同一序列
 * （可复现是门禁前提）；日期跳过周末（交易日口径）。
 */

/** 日 K 线（字段名对齐 lightweight-charts CandlestickData） */
export interface CandleBar {
  /** 交易日 YYYY-MM-DD（升序，跳过周末） */
  time: string
  open: number
  high: number
  low: number
  close: number
}

/** LCG 线性同余伪随机（[0,1)，确定性） */
function lcg(seed: number): () => number {
  let s = seed >>> 0
  return () => {
    s = (s * 1664525 + 1013904223) >>> 0
    return s / 0x100000000
  }
}

function round2(v: number): number {
  return Math.round(v * 100) / 100
}

/** 自 2026-01-05（周一）起的交易日序列（YYYY-MM-DD，跳过周六日） */
export function tradingDates(count: number): string[] {
  const out: string[] = []
  const d = new Date(Date.UTC(2026, 0, 5))
  while (out.length < count) {
    const day = d.getUTCDay()
    if (day !== 0 && day !== 6) {
      out.push(d.toISOString().slice(0, 10))
    }
    d.setUTCDate(d.getUTCDate() + 1)
  }
  return out
}

/**
 * 生成确定性合成日 K：微正漂移随机游走。
 * 不变量：high ≥ max(open, close) ≥ min(open, close) ≥ low > 0，全字段有限。
 */
export function generateCandles(count: number, seed: number, start = 100): CandleBar[] {
  const rand = lcg(seed)
  const dates = tradingDates(count)
  const out: CandleBar[] = []
  let prevClose = start
  for (let i = 0; i < count; i++) {
    const open = prevClose
    const close = round2(open * (1 + ((rand() - 0.48) * 2) / 100))
    const high = round2(Math.max(open, close) * (1 + rand() / 200))
    const low = round2(Math.min(open, close) * (1 - rand() / 200))
    out.push({ time: dates[i]!, open, high, low, close })
    prevClose = close
  }
  return out
}

/** 图表演示用 60 根日 K（模块级生成一次，处处一致） */
export const demoCandles: CandleBar[] = generateCandles(60, 20261002)
