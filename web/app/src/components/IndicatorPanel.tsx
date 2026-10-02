import { useMemo, useState } from 'react'
import { demoQuotes } from '../demoData'
import { useLocale } from '../hooks/useLocale'
import { ema, fmtPrice, sma } from '../lib/indicators'

type IndicatorKind = 'sma' | 'ema'

const INDICATOR_VALUES: IndicatorKind[] = ['sma', 'ema']

const PERIODS = [2, 3, 5, 10] as const

/**
 * 指标分析面板（L428 组件化数据分析界面）：
 * 选代码 / 指标 / 周期 → 纯 TS 序列（口径对齐 packages/core calculate_sma/calculate_ema）
 * → 全序列对表。真值以 wasm 引擎为准，见 WasmProbe；图表化渲染归 L429。
 */
export function IndicatorPanel() {
  const { tr, trf } = useLocale()
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
      <h2>{tr('ind.title')}</h2>
      <p>
        <label>
          {tr('ind.symbol')}{' '}
          <select value={symbol} onChange={(e) => setSymbol(e.target.value)}>
            {demoQuotes.map((q) => (
              <option key={q.symbol} value={q.symbol}>
                {q.symbol} {q.name}
              </option>
            ))}
          </select>
        </label>{' '}
        <label>
          {tr('ind.indicator')}{' '}
          <select
            value={indicator}
            onChange={(e) => setIndicator(e.target.value as IndicatorKind)}
          >
            {INDICATOR_VALUES.map((v) => (
              <option key={v} value={v}>
                {tr(`ind.${v}`)}
              </option>
            ))}
          </select>
        </label>{' '}
        <label>
          {tr('ind.period')}{' '}
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
        {tr('ind.last')}<b>{fmtPrice(last ?? NaN)}</b>
        {indicator === 'sma' && trf('ind.smaNote', { n: period - 1 })}
      </p>
      <table>
        <thead>
          <tr>
            <th>#</th>
            <th>{tr('ind.colClose')}</th>
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
