import { useState } from 'react'
import { demoQuotes } from '../demoData'
import { useLocale } from '../hooks/useLocale'
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
  const { tr, trf } = useLocale()
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
      setDetail(trf('wasm.detail', { a: got.length, b: ts.length }))
      setState('loaded')
    } catch {
      // 产物未拷入 public/pkg（或路径不可达）：明示启用步骤，不掩盖
      setState('missing')
    }
  }

  return (
    <section>
      <h2>{tr('wasm.title')}</h2>
      {state === 'idle' && (
        <p>
          {tr('wasm.idle')}<button onClick={probe}>{tr('wasm.load')}</button>
        </p>
      )}
      {state === 'loading' && <p>{tr('wasm.loading')}</p>}
      {state === 'loaded' && (
        <p>
          {tr('wasm.loaded')}<b>{wasmSma}</b>{tr('wasm.sep')}
          {matches === true
            ? tr('wasm.match')
            : matches === false
              ? tr('wasm.mismatch')
              : ''}
          <br />
          {detail}
        </p>
      )}
      {state === 'missing' && (
        <p>
          {tr('wasm.missingA')}<code>web/</code>{tr('wasm.missingB')}
          <code>npm run build:wasm</code>{tr('wasm.missingC')}
          <code>web/pkg/*</code>{tr('wasm.missingD')}
          <code>web/app/public/pkg/</code>{tr('wasm.missingE')}
          <button onClick={probe}>{tr('wasm.retry')}</button>
        </p>
      )}
    </section>
  )
}
