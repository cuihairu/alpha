import { demoQuotes } from './demoData'
import { IndicatorPanel } from './components/IndicatorPanel'
import { LiveQuoteBoard } from './components/LiveQuoteBoard'
import { PriceChart } from './components/PriceChart'
import { PrivacyPanel } from './components/PrivacyPanel'
import { QuoteTable } from './components/QuoteTable'
import { SqlWorkbench } from './components/SqlWorkbench'
import { ThemeToggle } from './components/ThemeToggle'
import { WasmProbe } from './components/WasmProbe'
import { WorkspaceTabs } from './components/WorkspaceTabs'
import { useWorkspaces } from './hooks/useWorkspaces'

/**
 * L427 骨架 → L428 组件化 → L429 高性能图表 → L430 SQL 工作台 → L431 响应式 →
 * L432 统一主题与个性化配置（docs/web-framework-selection.md §3）→ L499 实时行情看板 →
 * L508 多标签页工作区：标签条切换命名工作区，活动工作区的自选标的驱动
 * 实时行情看板（lib/workspaces.ts 纯归约 + localStorage 持久化）→
 * L488 数据与隐私面板（导出/清除本机应用数据）。
 * 可选资产（pkg/vendor）未拷入时逐面板降级可跑。
 */
export default function App() {
  const fallbackSymbols = demoQuotes.map((q) => ({ symbol: q.symbol, name: q.name }))
  const ws = useWorkspaces(
    fallbackSymbols.map((s) => s.symbol),
  )
  // 活动工作区标的 → 看板符号集（名称查演示映射，无名称只展示代码）
  const boardSymbols = ws.active.symbols.map((symbol) => ({
    symbol,
    name: fallbackSymbols.find((s) => s.symbol === symbol)?.name,
    base: demoQuotes.find((q) => q.symbol === symbol)?.price ?? 100,
  }))

  return (
    <main>
      <header className="site-header">
        <h1>Alpha Finance · 数据分析界面</h1>
        <ThemeToggle />
      </header>
      <p>
        TODO L427 选型 + L428 组件化 + L429 图表库 + L430 SQL 工作台 + L431 响应式 +
        L432 主题系统 + L499 实时看板 + L508 多标签工作区。旧演示页（web/index.html 等）零改动并存。
      </p>
      <WorkspaceTabs ws={ws} />
      <section>
        <h2>实时行情（L499：/ws 订阅，不可达降级模拟盘；标的集 = 活动工作区自选）</h2>
        <LiveQuoteBoard symbols={boardSymbols} />
      </section>
      <PrivacyPanel />
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
