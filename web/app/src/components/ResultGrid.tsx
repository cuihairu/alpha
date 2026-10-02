import type { ResultTable } from '../lib/resultTable'
import { useLocale } from '../hooks/useLocale'

/** 查询结果网格（L430 结果可视化：表格 + 行数/截断/耗时摘要） */
export function ResultGrid({ table, elapsedMs }: { table: ResultTable; elapsedMs: number }) {
  const { trf, tag } = useLocale()
  return (
    <div>
      <p>
        {trf('sql.summary', {
          total: table.total.toLocaleString(tag),
          cols: table.columns.length,
          ms: elapsedMs.toFixed(1),
        })}
        {table.truncated && trf('sql.truncated', { n: table.rows.length.toLocaleString(tag) })}
      </p>
      <table>
        <thead>
          <tr>
            {table.columns.map((c) => (
              <th key={c}>{c}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {table.rows.map((row, i) => (
            <tr key={i}>
              {row.map((cell, j) => (
                <td key={j}>{cell}</td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}
