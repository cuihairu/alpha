import type { QuoteSample } from '../demoData'
import { useLocale } from '../hooks/useLocale'
import { fmtPrice, sma } from '../lib/indicators'

/** 行情演示表（L427 提取为组件，L428 起 props 驱动可复用） */
export function QuoteTable({ quotes }: { quotes: QuoteSample[] }) {
  const { tr } = useLocale()
  return (
    <table>
      <thead>
        <tr>
          <th>{tr('feed.colSymbol')}</th>
          <th>{tr('feed.colName')}</th>
          <th>{tr('feed.colPrice')}</th>
          <th>{tr('feed.colChange')}</th>
          <th>{tr('feed.colSma3')}</th>
        </tr>
      </thead>
      <tbody>
        {quotes.map((q) => (
          <tr key={q.symbol}>
            <td>{q.symbol}</td>
            <td>{q.name}</td>
            <td>{fmtPrice(q.price)}</td>
            <td style={{ color: q.changePct >= 0 ? '#c0392b' : '#27ae60' }}>
              {q.changePct >= 0 ? '+' : ''}
              {q.changePct.toFixed(2)}
            </td>
            <td>{fmtPrice(sma(q.closes, 3).at(-1) ?? NaN)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  )
}
