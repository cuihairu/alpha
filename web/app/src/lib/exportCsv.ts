/**
 * 看板 CSV 导出（L477 桌面文件导出口径）：纯函数，Node/浏览器同形。
 * 与服务端 `/stocks/:symbol/history.csv`（L504）分工：那边是单标的历史
 * 序列导出，这边是看板当前快照导出（桌面复用同一 web 应用，零壳改动）。
 */

/** 快照行（调用方把 LiveQuote + changePct 拍平后传入） */
export interface QuoteCsvRow {
  symbol: string
  name?: string
  price: number
  changePct: number | null
  volume: number
  updatedAt: number
}

export const QUOTE_CSV_HEADER = 'symbol,name,price,change_pct,volume,updated_at'

/** RFC4180 最小转义：含逗号/引号/换行才加引号，内引号双写 */
export function csvField(value: string): string {
  return /[",\r\n]/.test(value) ? `"${value.replace(/"/g, '""')}"` : value
}

function fmtNum(value: number): string {
  return Number.isFinite(value) ? String(value) : ''
}

/**
 * 快照 → CSV 文本（`\n` 行尾、无 BOM——Excel 中文名乱码由调用方按需加 BOM，
 * 默认不加保持与服务端 history.csv 同形）。
 */
export function quotesToCsv(rows: QuoteCsvRow[]): string {
  const lines = rows.map((r) =>
    [
      csvField(r.symbol),
      csvField(r.name ?? ''),
      fmtNum(r.price),
      r.changePct === null ? '' : fmtNum(Math.round(r.changePct * 100) / 100),
      fmtNum(r.volume),
      r.updatedAt > 0 ? new Date(r.updatedAt).toISOString() : '',
    ].join(','),
  )
  return [QUOTE_CSV_HEADER, ...lines].join('\n') + '\n'
}

/** 浏览器下载（Blob + a.click；非浏览器环境返回 false 供调用方降级） */
export function downloadCsv(filename: string, text: string): boolean {
  try {
    const blob = new Blob([text], { type: 'text/csv;charset=utf-8' })
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = filename
    a.click()
    URL.revokeObjectURL(url)
    return true
  } catch {
    return false
  }
}
