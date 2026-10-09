# 跨平台 UX 一致性

四端（Web / Desktop / Android / iOS）同一产品语义的对账表：已对齐项给落点，
分歧项给口径裁决，缺口登记在 §4。

## 1. 语义对账表（实测/源码锁定）

表中 Web 列为 React 工程（`web/app/`）；frontendDist 拍板 A（2026-10-10）落地后
桌面窗口装**同一 React 产物**，Desktop 列与 Web 列同源（同组件同值）——壳层时代
的原生页/兜底壳差异不复存在，下表仅存壳层时代注记处已改口径。线上 Web 用户面仍是
`web/` 根原生页（部署面，与桌面无关）。色板已统一（2026-10 收敛为 `#c0392b` 涨 /
`#27ae60` 跌）；跳变动效现四端中 web/桌面同源（0.8s flash），移动端受能力上限。

| 语义 | Web（React 工程） | Desktop | Android | iOS | 裁决 |
|---|---|---|---|---|---|
| 涨跌色板 | `#c0392b` 涨 / `#27ae60` 跌（`web/app/src/styles.css` `.flash-up/down`） | 桌面同源 React 产物（同 `styles.css` 色板；壳层注记 `app.js:447`/`--up/--down` 随 frontendDist 切换退休） | `widget_up #C0392B` / `widget_down #27AE60`（`colors.xml`） | 灵动岛 `.red`/`.green`（`docs/ios-live-activities.md` §4） | ✅ 四端同值红涨绿跌（2026-10 收敛，原生页 5 处语义色对齐；RSI 超买/超卖与 success/error 为警示语义不计入） |
| 零涨跌（平盘）归属 | `pct >= 0` → 红色（含等号，`LiveQuoteBoard.tsx:89`） | 桌面同源 React（同一实现；壳层 `price >= open` 注记随切换退休） | 曾为 `> 0` → FLAT，**本项已改 `>= 0` → UP**（`HomeWidget.kt`，A 股平盘红惯例） | `>= 0 ? .red`（设计文档 §4） | ✅ 本项收敛：四端 0 归涨侧 |
| 数字格式 | 价格原值、`pct.toFixed(2)`、`volume.toLocaleString()`、缺值 `—` | 桌面同源 React（千分位/两位小数与 Web 完全一致——壳层「无千分位」分歧随切换消除） | `Locale.ROOT` `"%.2f"` / `"%+.2f%%"` / `"--"`（防本地化小数点漂移） | Decimal 两位（设计文档 §2） | ✅ 两位小数 + 缺值占位符同形（桌面与 Web 同组件，壳层登记项随切换消除） |
| 主题三态 | system/light/dark，缺省跟随系统（`ThemeToggle.tsx` + `matchMedia` 监听） | tauri v1 `theme` 仅 Light/Dark 无 system 档——窗口 chrome 留空走 OS，前端三态照常（`docs/theme-adaptation.md`） | `ThemePreference` SYSTEM/LIGHT/DARK，缺省 SYSTEM（`Theme.kt`，L511） | L119 交付面不动，跟随系统（设计文档登记） | ✅ 缺省口径统一：跟随系统 |
| 行情过期语义 | feed 重试 2 次不可达 → 确定性模拟盘降级（L499） | 无过期语义（命令面 Rust 合成行情常新；React 端 LiveQuoteBoard 断线停更降级与 Web 同源） | 120s `STALE_AFTER_MS` 灰显（`WidgetContent.from`） | staleDate 120s（设计文档 §3） | ⚠️ Android/iOS 同 120s；web 为降级、桌面命令面无过期——形态分歧登记 |
| 跳变反馈 | 整行 key 重挂载重放 0.8s flash 动画（`flash-up-kf/down-kf`） | 桌面同源 React（同 0.8s flash——壳层静态色注记随切换消除） | widget 无动画（RemoteViews 能力上限，方向色即反馈） | 灵动岛更新节流 60s（设计文档 §3） | ⚠️ 能力上限/部署面导致的形态分歧，语义（方向色）一致，登记不强求 |

## 2. 交互模式

- **导航**：web 单页看板 + 工作区标签条（L508）；Android 单 Activity（深链 `EXTRA_SYMBOL` 直达标的分析）；桌面窗口装 `web/app` React 产物（frontendDist 拍板 A，2026-10-10）；iOS 以灵动岛为 glance 面（设计）。
- **手势/键盘**：web 工作区标签 Enter/Escape 可达（L508）；移动端手势见 `docs/mobile-gestures.md`；桌面键盘快捷键沿 web。
- **离线**：web SW 壳缓存 + 模拟盘降级（L507/L499）；Android 见 `docs/mobile-offline.md`；widget 120s 灰显即离线表达。
- **隐私姿态**：三端 opt-in（生物识别/通知/小组件均为用户显式开启），见 `docs/mobile-privacy.md`、`docs/data-privacy.md`。

## 3. 本项改动

`HomeWidget.kt` 平盘归属 `> 0` → `>= 0`（UP），JVM 测试加零值断言
（`"+0.00%"` + UP）。其余表内项此前各增量已分别交付，本项只做对账，
无新分歧引入。

## 4. 缺口登记

- 桌面过期语义：命令面 Rust 合成行情常新、无 stale 灰显——接真实后端时补
  （登记；跳变动效已随 frontendDist 切换与 Web 同源，2026-10-10）；
- 平板/折叠屏断点：web 响应式未设平板专用断点，Android 无平板布局——待首个平板用户反馈再立项（拍板 2026-10-10 复核：维持，触发条件未到）；
- 无障碍：React 端已横扫（2026-10：交互控件 aria-label、错误 role=alert、
  图表容器 role=img、标签条 role=tablist/tab、隐私面板 role=status；
  表格均有 thead/th 结构可读）。
