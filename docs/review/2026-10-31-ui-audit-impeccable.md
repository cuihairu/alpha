# Alpha Finance Web UI Audit — impeccable (2026-10-31)

审核面：`web/app`（React 18 + TS + Vite 单页应用，L427–L508 骨架/组件化/图表/SQL 工作台/响应式/主题/实时看板/多标签工作区）。
方式：本地 `npm run dev`（:5176）真实渲染，Playwright/Chromium 截图；桌面 1440×900、移动 390×844，浅/深/英文三态。
截图目录：`~/.hermes/cache/scratch/ui-audit-screenshots/alpha/`（`main-light-desktop.png`、`main-dark-desktop.png`、`main-narrow.png`、`main-en-desktop.png`、`sql-error-state.png`、`fix-a1-light.png`、`fix-a1-dark.png`、`fix-a4-narrow.png`）。
依据：无 `DESIGN.md` / `PRODUCT.md`（已按要求记录缺失），本次按"对现有代码的 scoped audit"执行，不引入新视觉方向、不动品牌色。

## 整体 critique（不留情面）

这不是一个"给用户看的产品界面"，而是一个**仍处于开发期的内部工具页**：用户打开页面第一眼看到的就是一整行内部里程碑备忘（"TODO L427 选型 + L428 组件化 + …"）当作产品介绍，区块标题里混着 `L499：`、`L488：` 这类开发编号，连"地址覆盖：?feedWs=…"和"先在 web/ 执行 npm run prepare-vendors"这种工程师提示都直接写在界面上。把它当产品交付，等于把开发笔记当说明书发给用户。

可访问性也欠账：涨跌语义色在亮/暗两档各有一边不达标（亮档绿色 2.87:1、暗档红色 3.28:1），深色模式主按钮白字只有 2.97:1（比 WCAG 大字号下限还低），浅色 muted 文本 4.38:1 刚好卡在 AA 线下方——这些都是 WCAG AA 的硬性失败，不是风格偏好。响应式上，移动端一条 `button{width:100%}` 规则把工作区标签条拆成了整行堆叠（布局破碎）。

好在底子是干净的：单列内容优先、断点（≤720/721–1024/>1024）有明确意图、动效有 `prefers-reduced-motion` 降级、SQL/WASM 面板在资产缺失时能优雅降级并给出启用步骤、深色令牌体系完整。把这些当"骨架"而非"成品"，方向是对的；但成品化之前，上面的硬伤必须清掉。

## 问题清单（按严重度）

### 1. [严重 · UX 文案] 用户可见的介绍文案是开发 TODO 备忘
- **现象**：页面标题下方直接渲染 `TODO L427 选型 + L428 组件化 + L429 图表库 + L430 SQL 工作台 + L431 响应式 + L432 主题系统 + L499 实时看板 + L508 多标签工作区。旧演示页（web/index.html 等）零改动并存。`
- **截图**：`main-light-desktop.png`（页首）、`main-dark-desktop.png`、`main-en-desktop.png`（英文版同为 TODO 清单）。
- **定位**：`src/lib/i18n.ts` `app.intro`（zh 23–24 / en 157–158），渲染于 `src/App.tsx:85` `<p>{tr('app.intro')}</p>`。
- **建议**：需产品提供正式一句话介绍，替换为面向用户的文案（如"自选标的实时行情 + SQL 工作台 + 指标分析"）。本次**未代拟**——避免虚构产品声明；该条列入 TODO 待产品拍板。

### 2. [高 · a11y 对比度] 涨跌语义色在两种主题下各有一档不达标 ✅已修
- **现象**：A 股口径红涨绿跌。原 `#c0392b` 红在暗底 3.28:1（<4.5），原 `#27ae60` 绿在白底 2.87:1（<4.5）。两处均为**正文数据文本**（涨跌 % 单元格），必须 ≥4.5:1。
- **截图**：`main-light-desktop.png`（行情演示表 涨跌% 列）、`main-dark-desktop.png`（暗档红 +1.23 明显发暗）。
- **定位**：`src/components/LiveQuoteBoard.tsx`（`style={{ color: pct >= 0 ? '#c0392b' : '#27ae60' }}`）、`src/components/QuoteTable.tsx`（同）、`src/styles.css` `.flash-up/.flash-down`、`.live-pill-feed`（白字 on #27ae60 仅 2.87:1）。
- **修复**（commit `1768625`）：引入主题令牌 `--up`/`--down`（亮 `#b3261e`/`#1e8449`，暗 `#ff6b6b`/`#4caf7d`），内联色与 CSS 一律改用 var；`--up` 亮 6.54:1、`--down` 亮 4.72:1、`--up` 暗 6.42:1、`--down` 暗 6.57:1 全部达标；feed pill 绿底改 `#1e8449`（白字 4.72:1）。

### 3. [高 · a11y 对比度] 深色档主按钮白字对比不足 ✅已修
- **现象**：深色档 `--accent: #3d9be9`（浅蓝）上白字 2.97:1——**低于 WCAG 大字号 3:1 下限**，"加载 SQL 引擎""导出我的数据 (JSON)" 等按钮几乎看不清。
- **截图**：`main-dark-desktop.png`（按钮区）。
- **定位**：`src/styles.css` `:root[data-theme='dark']` `--accent`；按钮规则 `button { color:#fff; background: var(--accent) }`。
- **修复**（commit `db2315d`）：深色档 accent 微降明度至 `#2577b3`（白字 4.81:1），**同色相、非换品牌色**；浅色档品牌 `#2980b9` 未动（见 #6）。

### 4. [高 · a11y 对比度] 浅色 muted 文本对比轻微不达标 ✅已修
- **现象**：`--fg-muted: #6b7a8d` on 白底 4.38:1 < 4.5:1，而它被用于 12–13px 的 `.live-hint`/`.live-seq`/`.workspace-rename` 等正文小字。
- **截图**：`main-light-desktop.png`（实时行情 提示/序号区）。
- **定位**：`src/styles.css` `:root` `--fg-muted`。
- **修复**（commit `6c9e22f`）：`#66758b`（4.69:1）。

### 5. [高 · 响应式/布局破碎] 移动端工作区标签条被全宽按钮规则拆散 ✅已修
- **现象**：≤720px 的 `button { width:100%; min-height:40px }` 让工作区标签名、关闭 ×、chips 的 ×、"重命名""新建" 全部拉满整行，标签条从"水平芯片条"变成"整行堆叠"，工作区面板明显变形。
- **截图**：`main-narrow.png`（"默认"整行按钮 + "新建"另起一行）。
- **定位**：`src/styles.css` `@media (max-width:720px)` `button` 规则；工作区控件在 `src/components/WorkspaceTabs.tsx`。
- **修复**（commit `334e48e`）：媒体查询内为 `.workspace-tabs button / .workspace-new button / .workspace-chip button / .workspace-rename / .workspace-add` 加 `width:auto; min-height:auto`，全宽规则只留给页面级主操作。验证：`fix-a4-narrow.png` 标签条恢复水平。

### 6. [中 · a11y 对比度] 浅色品牌主按钮白字 4.30:1，轻微低于 AA — 待拍板
- **现象**：浅色档 `--accent: #2980b9`（品牌主色，亦见 `index.html` theme-color）上白字 4.30:1 < 4.5:1。14px 按钮正文。
- **定位**：`src/styles.css` `:root` `--accent`、`index.html` `theme-color`、`public/manifest.webmanifest`。
- **建议**：这是**品牌色 vs 可访问性**的取舍。本次按"不动品牌色"边界**未改**；若要达标，需产品同意把品牌蓝微降明度至 ≥4.5:1（如 `#2674a8`），或按钮文案加粗。**待用户拍板**。

### 7. [中 · UX 文案] 区块标题混入开发里程碑编号 — 待定
- **现象**："实时行情（L499：/ws 订阅，不可达降级模拟盘；标的集 = 活动工作区自选）""数据与隐私（L488：本地数据导出 / 清除，无账号体系）""行情演示（内置静态样本）"等标题带 `L4xx：` 开发编号。
- **定位**：`src/lib/i18n.ts` `app.liveSection` / `app.demoSection` 等。
- **建议**：去掉编号前缀，保留面向用户的语义（"实时行情""行情演示""数据与隐私"）；编号若需留档应移入注释。待定。

### 8. [低 · a11y] 键盘焦点样式依赖浏览器默认 — 待定
- **现象**：`styles.css` 无 `:focus-visible` 规则，Tab 走查依赖 Chromium 默认 outline；自定义按钮（无边框、纯色底）下默认焦点环存在但未系统化。
- **定位**：`src/styles.css`（全局）。
- **建议**：显式 `:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px }`。待定。

### 9. [低 · UX 文案] 工程师向提示直接面向用户 — 待定
- **现象**："地址覆盖：?feedWs=ws://host:port/ws"（`feed.hint`）、SQL 面板"引擎未启用：先在 web/ 执行 npm run prepare-vendors…"、WASM 探针"产物未拷入 public/pkg…"为开发说明；其中 SQL/WASM 属**降级提示**（可保留），`feed.hint` 的 URL 覆盖参数对普通用户无意义。
- **定位**：`src/lib/i18n.ts` `feed.hint`、`src/components/SqlWorkbench.tsx`、`src/components/WasmProbe.tsx`。
- **建议**：`feed.hint` 折叠进开发/调试信息或移除；SQL/WASM 降级提示保留但措辞更用户化。待定。

### 10. [低 · 数据可视化] 图表与指标口径
- **现象**：K 线图（lightweight-charts，红涨绿跌）与指标分析"均值"表渲染正常，序列口径与 `packages/core` 对齐；`prefers-reduced-motion` 下价格闪烁已停用、方向色保留（语义不丢）。无缺陷。

### 11. [信息] 设计权威缺失
- `DESIGN.md` / `PRODUCT.md` 不存在。本次按 scoped audit 处理；后续若要做正式视觉升级，应先走 `impeccable init` 采集产品上下文再定方向。

## 结果

| 项 | 状态 |
| --- | --- |
| 问题总数 | 10（不含信息项） |
| 当场修复 | 4（#2 对比度红绿、#3 深色按钮、#4 muted、#5 移动端标签条） |
| 列入 TODO | 5（#1 介绍文案、#6 品牌蓝拍板、#7 标题编号、#8 焦点样式、#9 提示文案） |
| 数据可视化观察 | 1（#10） |
