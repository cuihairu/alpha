import { demoQuotes } from './demoData'
import { IndicatorPanel } from './components/IndicatorPanel'
import { PriceChart } from './components/PriceChart'
import { QuoteTable } from './components/QuoteTable'
import { SqlWorkbench } from './components/SqlWorkbench'
import { WasmProbe } from './components/WasmProbe'

/**
 * L427 骨架 → L428 组件化 → L429 高性能图表 → L430 SQL 工作台
 * （docs/web-framework-selection.md §3）：组件拆分 QuoteTable / IndicatorPanel /
 * PriceChart / SqlWorkbench / WasmProbe，指标纯 TS 口径对齐
 * packages/core/src/indicators.rs；可选资产（pkg/vendor）未拷入时逐面板降级可跑。
 */
export default function App() {
  return (
    <main>
      <h1>Alpha Finance · 数据分析界面</h1>
      <p>
        TODO L427 选型 + L428 组件化 + L429 图表库 + L430 SQL 工作台。
        旧演示页（web/index.html 等）零改动并存。
      </p>
      <section>
        <h2>行情演示（内置静态样本）</h2>
        <QuoteTable quotes={demoQuotes} />
      </section>
      <IndicatorPanel />
      <PriceChart />
      <SqlWorkbench />
      <WasmProbe />
    </main>
  )
}
