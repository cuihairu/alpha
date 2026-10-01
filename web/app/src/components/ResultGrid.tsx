import type { ResultTable } from '../lib/resultTable'

/** 查询结果网格（L430 结果可视化：表格 + 行数/截断/耗时摘要） */
export function ResultGrid({ table, elapsedMs }: { table: ResultTable; elapsedMs: number }) {
  return (
    <div>
      <p>
        {table.total} 行 × {table.columns.length} 列 · {elapsedMs.toFixed(1)} ms
        {table.truncated && `（仅显示前 ${table.rows.length} 行）`}
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
