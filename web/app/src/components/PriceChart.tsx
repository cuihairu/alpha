import { useEffect, useRef } from 'react'
import {
  CandlestickSeries,
  LineSeries,
  createChart,
  type CandlestickData,
  type LineData,
  type Time,
} from 'lightweight-charts'
import { demoCandles } from '../lib/demoWalk'
import { useLocale } from '../hooks/useLocale'
import { sma } from '../lib/indicators'

const SMA_PERIOD = 5

/**
 * K 线图组件（L429）：lightweight-charts（TradingView，Canvas 原生渲染 +
 * 增量重绘/虚拟化内建）+ SMA 叠加线（与指标面板同口径，跳过前导 0.0 占位段）。
 * A股配色：红涨绿跌。图表库选型理由见 docs/web-framework-selection.md §3。
 */
export function PriceChart() {
  const { trf, tr } = useLocale()
  const containerRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const el = containerRef.current
    if (!el) return
    const chart = createChart(el, { autoSize: true, height: 340 })
    const candles = chart.addSeries(CandlestickSeries, {
      upColor: '#c0392b',
      downColor: '#27ae60',
      wickUpColor: '#c0392b',
      wickDownColor: '#27ae60',
      borderVisible: false,
    })
    candles.setData(demoCandles as CandlestickData<Time>[])

    // SMA 叠加线：跳过前导 0.0 占位段（与 lib 口径一致，从 period-1 起对齐时间轴）
    const smaValues = sma(
      demoCandles.map((c) => c.close),
      SMA_PERIOD,
    )
    const line: LineData<Time>[] = demoCandles.slice(SMA_PERIOD - 1).map((c, i) => ({
      time: c.time,
      value: smaValues[SMA_PERIOD - 1 + i]!,
    }))
    const lineSeries = chart.addSeries(LineSeries, { color: '#2980b9', lineWidth: 2 })
    lineSeries.setData(line)

    chart.timeScale().fitContent()
    return () => chart.remove()
  }, [])

  return (
    <section>
      <h2>{tr('chart.title')}</h2>
      {/* 图表库无内建读屏语义：容器给 role=img + 描述（标题下方的
          caption 文本兼作可访问名——内容口径一致，不另造文案） */}
      <div ref={containerRef} role="img" aria-label={tr('chart.title')} />
      <p>{trf('chart.caption', { n: SMA_PERIOD })}</p>
    </section>
  )
}
