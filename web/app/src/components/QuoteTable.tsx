import type { QuoteSample } from '../demoData'
import { fmtPrice, sma } from '../lib/indicators'

/** 行情演示表（L427 提取为组件，L428 起 props 驱动可复用） */
export function QuoteTable({ quotes }: { quotes: QuoteSample[] }) {
  return (
    <table>
      <thead>
        <tr>
          <th>代码</th>
          <th>名称</th>
          <th>价格</th>
          <th>涨跌%</th>
          <th>SMA(3) 末值（纯 TS）</th>
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
