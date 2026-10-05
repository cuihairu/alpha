# 跨平台 UX 一致性

四端（Web / Desktop / Android / iOS）同一产品语义的对账表：已对齐项给落点，
分歧项给口径裁决，缺口登记在 §4。

## 1. 语义对账表（实测/源码锁定）

表中 Web 列为 React 工程（`web/app/`，CI 覆盖、未部署）；线上/桌面用户面
是 `web/dist` 构建的原生页（`web/` 根 `index.html`/`app.js`）。色板已统一
（2026-10 收敛为 `#c0392b` 涨 / `#27ae60` 跌）；动效仍不同——React 端有
0.8s flash，原生页仅方向色。

| 语义 | Web（React 工程） | Desktop | Android | iOS | 裁决 |
|---|---|---|---|---|---|
| 涨跌色板 | `#c0392b` 涨 / `#27ae60` 跌（`web/app/src/styles.css` `.flash-up/down`） | 窗口装 web/dist 原生页，同值同向：`#c0392b` 涨 / `#27ae60` 跌（`app.js:447`；兜底壳 `--up/--down` 同套） | `widget_up #C0392B` / `widget_down #27AE60`（`colors.xml`） | 灵动岛 `.red`/`.green`（`docs/ios-live-activities.md` §4） | ✅ 四端同值红涨绿跌（2026-10 收敛，原生页 5 处语义色对齐；RSI 超买/超卖与 success/error 为警示语义不计入） |
| 零涨跌（平盘）归属 | `pct >= 0` → 红色（含等号，`LiveQuoteBoard.tsx:89`） | 兜底壳 `price >= open` 归涨侧（`desktop-shell.js:80`，语义等价） | 曾为 `> 0` → FLAT，**本项已改 `>= 0` → UP**（`HomeWidget.kt`，A 股平盘红惯例） | `>= 0 ? .red`（设计文档 §4） | ✅ 本项收敛：四端 0 归涨侧 |
| 数字格式 | 价格原值、`pct.toFixed(2)`、`volume.toLocaleString()`、缺值 `—` | 价/量原值输出、无千分位（`desktop-shell.js:83`），缺值占位同形 | `Locale.ROOT` `"%.2f"` / `"%+.2f%%"` / `"--"`（防本地化小数点漂移） | Decimal 两位（设计文档 §2） | ✅ 两位小数 + 缺值占位符同形；桌面量值无本地化格式（登记） |
| 主题三态 | system/light/dark，缺省跟随系统（`ThemeToggle.tsx` + `matchMedia` 监听） | tauri v1 `theme` 仅 Light/Dark 无 system 档——窗口 chrome 留空走 OS，前端三态照常（`docs/theme-adaptation.md`） | `ThemePreference` SYSTEM/LIGHT/DARK，缺省 SYSTEM（`Theme.kt`，L511） | L119 交付面不动，跟随系统（设计文档登记） | ✅ 缺省口径统一：跟随系统 |
| 行情过期语义 | feed 重试 2 次不可达 → 确定性模拟盘降级（L499） | 无过期语义（Rust 合成行情常新；原生页断线停更、兜底壳无 stale 逻辑） | 120s `STALE_AFTER_MS` 灰显（`WidgetContent.from`） | staleDate 120s（设计文档 §3） | ⚠️ Android/iOS 同 120s；web 为降级、桌面无过期——形态分歧登记 |
| 跳变反馈 | 整行 key 重挂载重放 0.8s flash 动画（`flash-up-kf/down-kf`） | 无动画（壳层 `.up/.down` 静态色，`web/dist/index.html:60`） | widget 无动画（RemoteViews 能力上限，方向色即反馈） | 灵动岛更新节流 60s（设计文档 §3） | ⚠️ 能力上限/部署面导致的形态分歧，语义（方向色）一致，登记不强求 |

## 2. 交互模式

- **导航**：web 单页看板 + 工作区标签条（L508）；Android 单 Activity（深链 `EXTRA_SYMBOL` 直达标的分析）；桌面窗口装 `web/dist` 原生页；iOS 以灵动岛为 glance 面（设计）。
- **手势/键盘**：web 工作区标签 Enter/Escape 可达（L508）；移动端手势见 `docs/mobile-gestures.md`；桌面键盘快捷键沿 web。
- **离线**：web SW 壳缓存 + 模拟盘降级（L507/L499）；Android 见 `docs/mobile-offline.md`；widget 120s 灰显即离线表达。
- **隐私姿态**：三端 opt-in（生物识别/通知/小组件均为用户显式开启），见 `docs/mobile-privacy.md`、`docs/data-privacy.md`。

## 3. 本项改动

`HomeWidget.kt` 平盘归属 `> 0` → `>= 0`（UP），JVM 测试加零值断言
（`"+0.00%"` + UP）。其余表内项此前各增量已分别交付，本项只做对账，
无新分歧引入。

## 4. 缺口登记

- 桌面过期语义与跳变动效缺失：桌面无 stale 灰显与 flash，仅方向色——随
  兜底壳演进或接真实后端时补（登记）；
- 平板/折叠屏断点：web 响应式未设平板专用断点，Android 无平板布局——待首个平板用户反馈再立项；
- 无障碍：React 端已横扫（2026-10：交互控件 aria-label、错误 role=alert、
  图表容器 role=img、标签条 role=tablist/tab、隐私面板 role=status；
  表格均有 thead/th 结构可读）。
