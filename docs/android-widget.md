# Android 桌面小组件与快捷方式

两件：**行情小卡**（`HomeWidget.kt` + `AlphaQuoteWidgetProvider`，经典
RemoteViews 免新依赖）与**静态快捷方式**（`shortcuts.xml` 深链预选分析）。

## 1. 数据面（widget 不发请求）

无 INTERNET 权限（L301 红线）是本项的设计前提：widget 自身不发起任何
数据获取，`updatePeriodMillis` 30min 系统轮询只做占位重绘；真实数据由
应用刷新路径驱动——`MarketScreen` 首载/下拉刷新拿到行情后
`publishWidgetQuote`（首只标的 → `WidgetStateStore` 单键 JSON 落盘 →
全量 widget 重绘）。应用是 widget 的唯一数据源。

## 2. 可测性切面（沿 L512 口径）

纯函数 JVM 锁定（`WidgetTest` 6 例）：状态往返/损坏 fail-safe 返 null、
涨跌幅现算（相对开盘，开盘价缺省/非正 → null 渲染平盘 "--"）、渲染
格式化（两位小数 Locale.ROOT 防小数点漂移、`%+.2f%%` 涨跌符号）、方向
判定（UP/DOWN/FLAT/NO_DATA，色彩映射归 Provider 设备路径）、过期灰显
（120s 阈值与 docs/ios-live-activities.md staleDate 同口径；时间戳解析
失败按过期 fail-safe）。RemoteViews/PendingIntent/AppWidgetManager 只
在设备路径，单测不触框架类。实测 `:app:testPlayDebugUnitTest` 39/39
（L509 时点；现测试目录共 54 例）。

## 3. 深链契约（widget 点击与快捷方式同一入口）

`MainActivity.EXTRA_SYMBOL = "alpha_extra_symbol"`：widget 点击带入其
当前标的；静态快捷方式两条（演示观察列表内的贵州茅台/平安银行）。
MainActivity onCreate 读 extra → `MarketScreen(initialSymbol)` → 自动
触发该标的技术分析。标准 launchMode 下每次新建实例走 onCreate（进程内
热路由归单实例改造 TODO）；动态 shortcuts（随用户观察列表生成）归设置
页 TODO。

## 4. 渲染与配色

单行卡（约 2×1 格，minWidth 180dp）：标的（加粗，超长省略）/价格/涨跌
/平盘占位四列，红涨绿跌（`#C0392B`/`#27AE60`，与三端一致）经
`setTextColor` 按方向动态上色；无数据时显示「Alpha 行情 --」。骨架期
浅色底（widget 深色适配归主题项后续）。

## 5. 权限与合规

零新增权限（widget/shortcut 均不需要）；receiver `exported=false`
（官方口径，系统特权投递 APPWIDGET_UPDATE）。L520 注册表无对应 planned
条目，无需迁移。

## 6. 与相邻项

- **L301/L390**：数据通道与 kv 抽象复用（`KeyValueStore`）；
- **L511/L510**：配色语义三端一致；stale 口径与 iOS Live Activities
  对齐；
- **L337**：告警触发时的 widget 主动刷新位（届时在通知路径追加
  `publishWidgetQuote` 调用即可）。
