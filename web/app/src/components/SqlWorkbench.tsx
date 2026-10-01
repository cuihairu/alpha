import { useState } from 'react'
import { demoCandles } from '../lib/demoWalk'
import { loadDuckDb, seedSql, tableToRows, type DuckDbEngine } from '../lib/duckdb'
import { capRows, type ResultTable } from '../lib/resultTable'
import { ResultGrid } from './ResultGrid'

type EngineState = 'idle' | 'loading' | 'ready' | 'error'

const RESULT_CAP = 100

const DEFAULT_SQL = 'SELECT * FROM demo_quotes ORDER BY change_pct DESC'

/**
 * SQL 工作台（L430 跨平台 SQL 查询编辑器和结果可视化）：
 * DuckDB-WASM 引擎（浏览器/桌面同源可复用——Tauri 壳接入归 L427 §6 选项 A），
 * 编辑器最小实现（textarea，等宽字体；语法高亮/补全留后续增强），
 * 结果可视化 = ResultGrid 网格 + 行列/耗时摘要。引擎资产未拷入时明示降级。
 */
export function SqlWorkbench() {
  const [state, setState] = useState<EngineState>('idle')
  const [error, setError] = useState<string>('')
  const [sql, setSql] = useState(DEFAULT_SQL)
  const [running, setRunning] = useState(false)
  const [result, setResult] = useState<ResultTable | null>(null)
  const [elapsedMs, setElapsedMs] = useState(0)

  async function boot() {
    setState('loading')
    try {
      const engine: DuckDbEngine = await loadDuckDb()
      await engine.query(seedSql(demoCandles)) // 多语句初始化演示表
      setState('ready')
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
      setState('error')
    }
  }

  async function run() {
    setRunning(true)
    try {
      const engine: DuckDbEngine = await loadDuckDb()
      const t0 = performance.now()
      const table = await engine.query(sql)
      setElapsedMs(performance.now() - t0)
      const { columns, rows } = tableToRows(table)
      setResult(capRows(columns, rows, RESULT_CAP))
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
      setState('error')
    } finally {
      setRunning(false)
    }
  }

  return (
    <section>
      <h2>SQL 工作台（DuckDB-WASM）</h2>
      {state === 'idle' && (
        <p>
          引擎未启用。先在 <code>web/</code> 执行 <code>npm run prepare:vendors</code>，再把{' '}
          <code>web/vendor/duckdb/</code> 拷入 <code>web/app/public/vendor/duckdb/</code>，
          然后加载。<button onClick={boot}>加载 SQL 引擎</button>
        </p>
      )}
      {state === 'loading' && <p>引擎加载中…</p>}
      {state === 'error' && (
        <p>
          引擎/查询错误：<code>{error}</code>{' '}
          <button onClick={boot}>重试加载</button>
        </p>
      )}
      {state === 'ready' && (
        <div>
          <p>引擎就绪（演示表：demo_quotes / demo_candles）。编辑 SQL 后执行：</p>
          <textarea
            value={sql}
            onChange={(e) => setSql(e.target.value)}
            rows={6}
            spellCheck={false}
            style={{ width: '100%', fontFamily: 'monospace' }}
          />
          <p>
            <button onClick={run} disabled={running}>
              {running ? '执行中…' : '执行查询'}
            </button>
          </p>
          {result && <ResultGrid table={result} elapsedMs={elapsedMs} />}
        </div>
      )}
    </section>
  )
}
