# 跨平台 UI 框架选型决策

口径：**选型先定论、骨架再落地、边界先登记**。本单三件：① 选型决策（本文档）；
② Web 端 React 骨架落地（`web/app/` + 门禁，旧演示页零改动）；③ Desktop/Mobile
只登记接入边界（§6），不在本单实现。

## 1. 定论（TL;DR）

| 平台 | 选型 | 状态 |
|---|---|---|
| **Web** | **React 18 + TypeScript + Vite** | ✅ 本单落地骨架（`web/app/`） |
| **Desktop** | Tauri 1.5（维持现状） | 已落地 `desktop/`；前端产物接入策略登记 §6，本单不改 `tauri.conf.json` |
| **Mobile** | Native（维持现状） | 已落地 Android Kotlin（L118/L337/L367/L390）+ iOS Swift 骨架（L119）；不引入 React Native，见 §6 |
| **Yew/Leptos** | **不选**作主 UI 框架 | wasm-analyzer 维持 cdylib + JS 绑定的既有消费模式（§3） |

## 2. 候选对比（React vs Vue vs Yew/Leptos）

评分维度取自本仓实际约束：金融分析 UI 的图表/表格/编辑器依赖、与既有
Rust wasm 引擎（`wasm-analyzer` cdylib）的集成面、CI 门禁可维护性。

| 维度 | React 18 + Vite | Vue 3 + Vite | Yew / Leptos |
|---|---|---|---|
| 金融级组件（图表/虚拟表格/SQL 编辑器） | lightweight-charts、AG Grid/react-window、Monaco editor 均有 React 一等封装，三件直接可用 | 中：echarts 成熟，交易级图表与 Monaco 封装偏薄 | 仅零散实验性绑定，图表/虚拟表格/编辑器三件均需自行开发或经 JS 桥回借 React 组件，等于双重成本 |
| TypeScript | 一等（ts-jest/vitest/模板成熟） | 一等 | Rust 侧类型即文档，但 UI 层丢掉 TS 工具链 |
| 与 wasm 集成面 | **保持现状**：`wasm-pack` 产 JS 绑定，import 即用（`wasm-demo.html` 已验证该模式） | 同 React | UI 与引擎同语言、类型可贯通（理论上顺）；但 L0 wasm 门禁（check-cross-platform.sh）会把 UI 重编译卷进 Rust 门禁周期 |
| 生产案例 / 招聘 / AI 辅助 | 最大 | 大 | 小（生产案例少，出问题排查成本高） |
| 本仓既有投入 | 零（web/ 为 vanilla JS，无迁移负担） | 零 | 零（TODO L428 仅是设想项，无一行代码） |
| 后续 TODO 项契合度 | L428 组件化界面 / L429 图表库 / L430 SQL 编辑器均有现成 React 路线 | L430 需自封装 | 全部需自行开发 |

**结论**：Web 主框架选 **React + TS + Vite**。Vue 各项均可胜任但金融终端向
生态略逊；Yew/Leptos 的「同语言」收益抵不过生态空缺——图表、虚拟行情表、
SQL 编辑器（L429/L430 直接依赖项）在 React 生态全是现成生产件，在
Yew/Leptos 是从零自行开发。

## 3. 对「Yew/Leptos 路线」的处置与影响面

- **本单影响**：新增 `web/app/`（Vite + React + TS），**不改动** `web/` 既有
  任何文件——旧演示页（`index.html`/`app.js`/`wasm-demo.html`/`server.js`）
  行为零回退，并由 `scripts/check-web.sh` 第 1 步在场守门 + `node --check`
  语法冒烟兜底。
- **TODO L428**（「基于 Yew/Leptos 开发 Web 端组件化数据分析界面」）：✅
  **已按本定论改写为 React 组件化路线并落地**（L428 轮：QuoteTable /
  IndicatorPanel / WasmProbe 组件拆分 + SMA/EMA 指标分析面板，指标纯 TS 口径
  对齐 packages/core），不引入 Yew/Leptos。
- **TODO L429**（图表库 D3 + Canvas）：✅ **已定论 lightweight-charts 5.2 并落地**
  （TradingView 出品金融图表库：Canvas 原生渲染 + 增量重绘/缩放虚拟化内建，
  gzip 增量 ~57KB；K 线组件 PriceChart + SMA 叠加线 + 确定性合成行情数据源）。
  D3 定位为**底层可视化原语**（scale/shape 拼装需自建渲染循环与交互），与
  「集成高性能图表库」目标不符，不直接引入；ECharts 对行情主图过重（gzip
  ~300KB+）落备选。TODO 原文「D3.js」按此定论解释落实。
- **wasm 引擎消费**：维持 `wasm-analyzer` → `web/pkg/`（wasm-pack, target web）
  现模式，React 侧经动态 import 探针接缝（`web/app/src/App.tsx`），**不**把
  UI 构建卷进 Rust workspace 门禁（`check-cross-platform.sh` 四步语义不变）。
- **指标口径纪律**：前端不另立一套指标语义。骨架示例的纯 TS `sma` 逐项对齐
  `packages/core/src/indicators.rs` 的输出契约（等长 / 前导 `0.0` 占位 /
  样本不足全 `0.0` / 4 位小数取整，见 §4），并由 vitest 以 Rust 侧同名单测的
  样本向量锁定；两处偏差（截断窗口 vs 等长占位、不取整 vs 4 位）即视为回退。

## 4. Web 骨架落地（本单 ②）

结构表为 L427 时点快照——`App.tsx` 此后已演进为工作台看板（L428 组件化
→ L432 主题 → L499 实时行情 → L508 工作区标签 → L488 隐私面板）。
另：`web/app` 构建产物不部署、不进桌面 `distDir`，线上与桌面用户面
始终是 `web/` 根原生页；React 端由 CI 测试与构建覆盖。

```
web/app/                    Vite + React 18 + TypeScript（独立工程，不动 web/ 根）
├── index.html              #root 挂载点
├── src/main.tsx            createRoot 挂载
├── src/App.tsx             （L427 时点：品牌头 + 行情演示表 + SMA 展示
│                           + WASM 引擎探针；现为多面板工作台，见上注）
├── src/lib/indicators.ts   纯 TS 指标函数（sma），口径**对齐** Rust 侧
│                           `TechnicalIndicators::calculate_sma`（等长输出/
│                           前导 0.0 占位/样本不足全 0/4 位小数取整），
│                           vitest 单测沿用 Rust 同名样本向量对账
├── public/pkg/             （gitignore，可选）拷入 web/pkg 产物后探针可用
└── package-lock.json       锁定依赖（CI npm ci 确定性安装）
```

- **示例页行为**：纯 TS 计算 SMA 展示（无 wasm 也可运行）+ WASM 探针卡片
  （动态 `import(/* @vite-ignore */ '/pkg/...')`，未构建时明示启用步骤）——
  演示的正是「React 壳 + Rust wasm 引擎」的目标架构接缝。
- **React 18 而非 19**：生态/testing-library 兼容最稳，骨架期不求新。

## 5. 测试门禁（本单 ②）

新脚本 `scripts/check-web.sh`，接入 CI `wasm` 作业（ubuntu-latest 自带 node）：

1. **旧演示页在场守门**（行为不回退：六文件在场 + `node --check` 语法冒烟）
2. `npm ci`（lockfile 锁定）
3. `tsc --noEmit` 类型检查 + `vitest run` 单测
4. `vite build` 产物构建

与既有四道 Rust 门禁并存：`check-lint.sh` / 全仓测试 / `check-cross-platform.sh`
/ `check-desktop.sh` 语义均不变（本单 Rust 零改动，复跑实证 0 失败）。

CI 侧观察（登记，非阻塞）：`web/app` 引入 npm 依赖后，GitHub 托管的
**Dependabot Updates** 作业（平台生成；仓内 `.github/workflows/` 无此文件、
无接缝可改）开始对 npm 生态跑 security 更新扫描，其自有 `Run Dependabot`
步骤失败（伴随 `GITHUB_REGISTRIES_PROXY` 解析警告、npm registry 代理阶段
中断）——平台侧问题，**非必需作业**、不阻塞合并，与「安全审计（报告型）」
同族登记观察，仓内不为其改门禁。

## 6. Desktop / Mobile 接入边界（本单 ③，只登记不实现）

- **Desktop（Tauri 1.5，已落地）**：现 `tauri.conf.json` 以 `web/dist` 为
  front dist（vanilla 演示壳）。接入策略两选项，留后续单定：
  - **A（倾向）**：`frontendDist` 改指 `web/app/dist`，桌面复用 React 产物，
    Tauri API 经 `@tauri-apps/api` 注入；
  - **B**：桌面专属前端壳（仅当桌面交互与 Web 分化到必要时）。
  影响面：`desktop/` 构建脚本与 CI `Desktop Framework` 作业；本单零改动。
- **Mobile（Native，已落地）**：**不引入** React Native/Flutter——移动端共享
  的是 Rust 核心库（UniFFI FFI 面，L118 起只增不改），而非 UI 层；UI 框架
  选型对 mobile 的唯一影响是「设计语义对齐」（React 端组件状态机可作 iOS/
  Android 交互对齐的参考实现），无工程依赖。
- **跨端一致性**：主题/交互模式统一留 UI 体系成形后立项。

## 7. 非交互假设（自行判定，已注明）

1. 记账行号 L427（沿 TODO 行号；行号漂移不作编号依据）。
2. React 18.3 锁版本（非 19）：骨架期生态稳定优先（§4）。
3. vitest 用 node 环境跑纯函数单测（不引 jsdom）：测试门禁到量即可（功能
   优先）——L428 组件化以「纯函数契约测试 + tsc 严格类型 + 构建门禁」覆盖，
   DOM 渲染测试（testing-library）留交互复杂化后再引入。
4. `web/app/` 独立 npm 工程（不复用 `web/package.json`）：旧演示页依赖图
   （duckdb-wasm/vendor 脚本）零污染，两工程演进互不牵制。
5. CI 挂在既有 `wasm` 作业尾部（runner 自带 node，免新增作业/矩阵改动）。
6. 示例页数据为内置静态样本（`src/demoData.ts`）——L427 时点口径；
   L499 起 `LiveQuoteBoard` 已接 `real-time-feed` WebSocket（不可达
   重试两次回退演示数据），其余面板仍走样本。
7. `web/pkg/` 不入 git（既有约定）；`public/pkg/` 同策略，拷入后探针可用。
