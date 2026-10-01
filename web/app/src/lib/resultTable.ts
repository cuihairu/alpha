/**
 * 查询结果表纯函数（L430 结果可视化数据面）：引擎无关（Arrow/任意行集），
 * 截断上限 + 空值口径统一在 UI 渲染前完成，纯函数可测。
 */

/** 结果表（rows 已字符串化，'NULL' 为 SQL 惯例空值呈现） */
export interface ResultTable {
  columns: string[]
  rows: string[][]
  /** 截断前总行数 */
  total: number
  truncated: boolean
}

/** 列名透传 + 行截断 + 值字符串化（null/undefined → 'NULL'） */
export function capRows(columns: string[], rows: unknown[][], cap: number): ResultTable {
  const shown = rows.slice(0, cap)
  return {
    columns,
    rows: shown.map((r) => r.map((v) => (v === null || v === undefined ? 'NULL' : String(v)))),
    total: rows.length,
    truncated: rows.length > cap,
  }
}
