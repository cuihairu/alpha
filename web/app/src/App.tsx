import { demoQuotes } from './demoData'
import { IndicatorPanel } from './components/IndicatorPanel'
import { LiveQuoteBoard } from './components/LiveQuoteBoard'
import { PriceChart } from './components/PriceChart'
import { QuoteTable } from './components/QuoteTable'
import { SqlWorkbench } from './components/SqlWorkbench'
import { ThemeToggle } from './components/ThemeToggle'
import { WasmProbe } from './components/WasmProbe'

/**
 * L427 骨架 → L428 组件化 → L429 高性能图表 → L430 SQL 工作台 → L431 响应式 →
 * L432 统一主题与个性化配置（docs/web-framework-selection.md §3）+ L499 实时行情看板：组件拆分
 * QuoteTable / IndicatorPanel / PriceChart / SqlWorkbench / WasmProbe /
 * ThemeToggle，指标纯 TS 口径对齐 packages/core/src/indicators.rs；
 * 可选资产（pkg/vendor）未拷入时逐面板降级可跑。
 */
export default function App() {
  return (
    <main>
      <header className="site-header">
        <h1>Alpha Finance · 数据分析界面</h1>
        <ThemeToggle />
      </header>
      <p>
        TODO L427 选型 + L428 组件化 + L429 图表库 + L430 SQL 工作台 + L431 响应式 +
        L432 主题系统。旧演示页（web/index.html 等）零改动并存。
      </p>
      <section>
        <h2>实时行情（L499：/ws 订阅，不可达降级模拟盘）</h2>
        <LiveQuoteBoard />
      </section>
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
