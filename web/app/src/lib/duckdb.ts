/**
 * DuckDB-WASM 加载器（L430 SQL 工作台引擎面）。
 * 资产沿用 web/vendor/duckdb（web/scripts/prepare-vendors.mjs 产出，不入 git）：
 * 拷入 public/vendor/duckdb 后按根路径动态加载；未拷入时由 UI 明示启用步骤
 * 并降级（与 WasmProbe 的 public/pkg 模式一致），构建保持 hermetic。
 */

/** vendor duckdb-browser.mjs 的最小消费面（vendor 无类型，刻意宽松） */
type DuckDbModule = {
  AsyncDuckDB: new (logger: unknown, worker: Worker) => DuckDbInstance
  ConsoleLogger: new () => unknown
  createWorker: (url: string) => Promise<Worker>
}

type DuckDbInstance = {
  instantiate: (wasmUrl: string, bundle: null) => Promise<unknown>
  connect: () => Promise<{ query: (sql: string) => Promise<ArrowTable> }>
}

/** apache-arrow Table 的最小消费面 */
export interface ArrowTable {
  numRows: number
  toArray: () => Array<Record<string, unknown>>
  schema: { fields: Array<{ name: string }> }
}

export interface DuckDbEngine {
  query: (sql: string) => Promise<ArrowTable>
}

const VENDOR_BASE = '/vendor/duckdb'

/** 动态加载并实例化 DuckDB-WASM（MVP bundle，与 legacy demo 同资产） */
export async function loadDuckDb(): Promise<DuckDbEngine> {
  const mod = (await import(
    /* @vite-ignore */ `${VENDOR_BASE}/duckdb-browser.mjs`
  )) as unknown as DuckDbModule
  const worker = await mod.createWorker(`${VENDOR_BASE}/duckdb-browser-mvp.worker.js`)
  const db = new mod.AsyncDuckDB(new mod.ConsoleLogger(), worker)
  await db.instantiate(`${VENDOR_BASE}/duckdb-mvp.wasm`, null)
  const conn = await db.connect()
  return { query: (sql) => conn.query(sql) }
}

/** arrow 表 → 纯数据行集（列名取 schema，行值按键取齐） */
export function tableToRows(
  t: ArrowTable,
): { columns: string[]; rows: unknown[][] } {
  const columns = t.schema.fields.map((f) => f.name)
  const rows = t.toArray().map((r) => columns.map((c) => r[c]))
  return { columns, rows }
}

/** 演示表初始化 SQL：行情样本 + 合成日 K（可 GROUP BY / 窗口练习） */
export function seedSql(candles: Array<{ time: string; open: number; high: number; low: number; close: number }>): string {
  const rows = candles
    .map((c) => `('${c.time}',${c.open},${c.high},${c.low},${c.close})`)
    .join(',')
  return `
CREATE OR REPLACE TABLE demo_quotes AS SELECT * FROM (VALUES
  ('600519','贵州茅台',90.50,1.23),
  ('000001','平安银行',10.20,-0.45),
  ('300750','宁德时代',187.60,2.87)
) AS t(symbol, name, price, change_pct);
CREATE OR REPLACE TABLE demo_candles AS SELECT * FROM (VALUES
  ${rows}
) AS t(time, open, high, low, close);
`
}
