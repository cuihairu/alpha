import { useState } from 'react'
import { demoQuotes } from '../demoData'
import { fmtPrice, sma } from '../lib/indicators'

/** wasm-pack --target web 产物的最小消费面（与 wasm-analyzer 导出对齐） */
interface WasmModule {
  default: () => Promise<void>
  WasmAnalyzer: new () => {
    calculateSMA(prices: Float64Array, period: number): Float64Array
  }
}

const WASM_URL = '/pkg/alpha_wasm_analyzer.js'

type ProbeState = 'idle' | 'loading' | 'loaded' | 'missing' | 'error'

/**
 * 探针判定：wasm 与纯 TS 的 SMA 全量对账（等长 + 逐位 1e-9 容差）。
 * 两侧同样按 4 位小数取整且滑动累加顺序一致，理论逐位相等，留容差兜底浮点差。
 */
function sameSeries(a: ArrayLike<number>, b: number[]): boolean {
  if (a.length !== b.length) return false
  for (let i = 0; i < b.length; i++) {
    if (Math.abs((a[i] ?? NaN) - (b[i] ?? NaN)) > 1e-9) return false
  }
  return true
}

/** WASM 引擎探针（L427 引入，L428 提取为组件）：动态 import 产物并把 wasm SMA 与纯 TS 口径对账 */
export function WasmProbe() {
  const [state, setState] = useState<ProbeState>('idle')
  const [wasmSma, setWasmSma] = useState<string>('')
  const [detail, setDetail] = useState<string>('')
  const [matches, setMatches] = useState<boolean | null>(null)

  async function probe() {
    setState('loading')
    try {
      const mod = (await import(/* @vite-ignore */ WASM_URL)) as WasmModule
      await mod.default()
      const closes = demoQuotes[0]!.closes
      const got = new mod.WasmAnalyzer().calculateSMA(new Float64Array(closes), 3)
      const ts = sma(closes, 3)
      setWasmSma(fmtPrice(got[got.length - 1] ?? NaN))
      setMatches(sameSeries(got, ts))
      setDetail(
        `wasm 输出 ${got.length} 位 / 纯 TS 输出 ${ts.length} 位（口径：等长、前导 0.0 占位、4 位小数取整）`,
      )
      setState('loaded')
    } catch {
      // 产物未拷入 public/pkg（或路径不可达）：明示启用步骤，不掩盖
      setState('missing')
    }
  }

  return (
    <section>
      <h2>Rust WASM 引擎探针</h2>
      {state === 'idle' && (
        <p>
          未加载。<button onClick={probe}>加载 WASM 引擎</button>
        </p>
      )}
      {state === 'loading' && <p>加载中…</p>}
      {state === 'loaded' && (
        <p>
          已加载。600519 SMA(3) 末值 = <b>{wasmSma}</b>，
          {matches === true
            ? '与纯 TS 口径逐位一致 ✔'
            : matches === false
              ? '与纯 TS 口径不一致 ✘（请核对指标实现）'
              : ''}
          <br />
          {detail}
        </p>
      )}
      {state === 'missing' && (
        <p>
          未构建：先在 <code>web/</code> 执行 <code>npm run build:wasm</code>，再把{' '}
          <code>web/pkg/*</code> 拷入 <code>web/app/public/pkg/</code> 后重试。
          <button onClick={probe}>重试</button>
        </p>
      )}
    </section>
  )
}
