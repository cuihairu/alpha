import { useMemo, useState } from 'react'
import { demoQuotes } from '../demoData'
import { ema, fmtPrice, sma } from '../lib/indicators'

type IndicatorKind = 'sma' | 'ema'

const INDICATORS: Array<{ value: IndicatorKind; label: string }> = [
  { value: 'sma', label: 'SMA 简单移动平均' },
  { value: 'ema', label: 'EMA 指数移动平均' },
]

const PERIODS = [2, 3, 5, 10] as const

/**
 * 指标分析面板（L428 组件化数据分析界面）：
 * 选代码 / 指标 / 周期 → 纯 TS 序列（口径对齐 packages/core calculate_sma/calculate_ema）
 * → 全序列对表。真值以 wasm 引擎为准，见 WasmProbe；图表化渲染归 L429。
 */
export function IndicatorPanel() {
  const [symbol, setSymbol] = useState(demoQuotes[0]!.symbol)
  const [indicator, setIndicator] = useState<IndicatorKind>('sma')
  const [period, setPeriod] = useState<number>(3)

  const quote = demoQuotes.find((q) => q.symbol === symbol) ?? demoQuotes[0]!
  const series = useMemo(
    () => (indicator === 'sma' ? sma : ema)(quote.closes, period),
    [quote, indicator, period],
  )
  const last = series[series.length - 1]

  return (
    <section>
      <h2>指标分析</h2>
      <p>
        <label>
          代码{' '}
          <select value={symbol} onChange={(e) => setSymbol(e.target.value)}>
            {demoQuotes.map((q) => (
              <option key={q.symbol} value={q.symbol}>
                {q.symbol} {q.name}
              </option>
            ))}
          </select>
        </label>{' '}
        <label>
          指标{' '}
          <select
            value={indicator}
            onChange={(e) => setIndicator(e.target.value as IndicatorKind)}
          >
            {INDICATORS.map((it) => (
              <option key={it.value} value={it.value}>
                {it.label}
              </option>
            ))}
          </select>
        </label>{' '}
        <label>
          周期{' '}
          <select value={period} onChange={(e) => setPeriod(Number(e.target.value))}>
            {PERIODS.map((p) => (
              <option key={p} value={p}>
                {p}
              </option>
            ))}
          </select>
        </label>
      </p>
      <p>
        末值：<b>{fmtPrice(last ?? NaN)}</b>
        {indicator === 'sma' && '（SMA 前 period-1 位为 0.0 占位，非真实值）'}
      </p>
      <table>
        <thead>
          <tr>
            <th>#</th>
            <th>收盘</th>
            <th>{indicator.toUpperCase()}({period})</th>
          </tr>
        </thead>
        <tbody>
          {quote.closes.map((close, i) => (
            <tr key={i}>
              <td>{i}</td>
              <td>{fmtPrice(close)}</td>
              <td>{series[i] ?? '—'}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </section>
  )
}
