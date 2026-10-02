import { demoQuotes } from '../demoData'
import { useLiveQuotes } from '../hooks/useLiveQuotes'
import { downloadCsv, quotesToCsv } from '../lib/exportCsv'
import { changePct, type LiveQuote } from '../lib/liveFeed'
import { fmtPrice, sma } from '../lib/indicators'

/** 默认符号集：复用演示样本的代码/名称/基准价（模拟盘锚价同源） */
export const DEFAULT_SYMBOLS = demoQuotes.map((q) => ({
  symbol: q.symbol,
  name: q.name,
  base: q.price,
}))

const SOURCE_LABEL: Record<string, string> = {
  connecting: '连接 real-time-feed…',
  feed: '实时已连接',
  simulated: '模拟行情（feed 不可达，确定性序列）',
}

function updateTime(q: LiveQuote): string {
  return q.updatedAt > 0 ? new Date(q.updatedAt).toLocaleTimeString() : '—'
}

/**
 * 实时行情看板（L499）：/ws 订阅 → 降级模拟盘；价格跳变闪烁
 * （A股口径红涨绿跌，与 QuoteTable 一致），SMA(5) 滚动窗口由
 * 纯 TS 指标（口径对齐 packages/core calculate_sma）现场计算。
 * feed 地址可用 `?feedWs=ws://host:port/ws` 覆盖。
 * L508：符号集可由活动工作区驱动（未传时用演示默认集）。
 */
export function LiveQuoteBoard({
  symbols,
}: {
  symbols?: Array<{ symbol: string; name?: string; base: number }>
}) {
  const { quotes, source, seq } = useLiveQuotes(symbols ?? DEFAULT_SYMBOLS)

  /** L477 桌面文件导出：看板快照 → CSV 下载（桌面复用同一 web 应用） */
  const onExport = () => {
    const text = quotesToCsv(
      quotes.map((q) => ({
        symbol: q.symbol,
        name: q.name,
        price: q.price,
        changePct: changePct(q),
        volume: q.volume,
        updatedAt: q.updatedAt,
      })),
    )
    downloadCsv(`quotes-${new Date().toISOString().slice(0, 10)}.csv`, text)
  }

  return (
    <div>
      <p>
        <span className={`live-pill live-pill-${source}`}>{SOURCE_LABEL[source]}</span>
        {seq !== null && <span className="live-seq">sync seq {seq}</span>}
        <span className="live-hint">地址覆盖：?feedWs=ws://host:port/ws</span>
        <button type="button" onClick={onExport} style={{ marginLeft: 12 }}>
          导出 CSV
        </button>
      </p>
      <table>
        <thead>
          <tr>
            <th>代码</th>
            <th>名称</th>
            <th>最新价</th>
            <th>涨跌%</th>
            <th>SMA(5)</th>
            <th>成交量</th>
            <th>更新时间</th>
          </tr>
        </thead>
        <tbody>
          {quotes.map((q) => {
            const pct = changePct(q)
            // key 含价格与跳变方向：值变化重挂载单元格，CSS 动画重放即闪烁
            const flashKey = `${q.symbol}-${q.price}-${q.tickDir}`
            return (
              <tr key={q.symbol}>
                <td>{q.symbol}</td>
                <td>{q.name ?? '—'}</td>
                <td
                  key={flashKey}
                  className={q.tickDir === 1 ? 'flash-up' : q.tickDir === -1 ? 'flash-down' : ''}
                >
                  {Number.isFinite(q.price) ? fmtPrice(q.price) : '—'}
                </td>
                <td style={{ color: (pct ?? 0) >= 0 ? '#c0392b' : '#27ae60' }}>
                  {pct === null ? '—' : `${pct >= 0 ? '+' : ''}${pct.toFixed(2)}`}
                </td>
                <td>
                  {q.closes.length >= 5 ? fmtPrice(sma(q.closes, 5).at(-1) ?? NaN) : '—'}
                </td>
                <td>{q.volume > 0 ? q.volume.toLocaleString() : '—'}</td>
                <td>{updateTime(q)}</td>
              </tr>
            )
          })}
        </tbody>
      </table>
    </div>
  )
}
