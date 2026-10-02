# iOS Live Activities 与灵动岛支持——工程设计契约

**边界声明**：`mobile/ios/` 属 L119 会话交付面，本项不动其任何文件。
本文是可直接照做的实现契约（ActivityKit 接入点、类型契约、更新预算、
隐私开关），iOS 目录解封后按 §2–§4 落地即可；服务端推送更新路径见 §5。

## 1. 功能定位

行情锁屏/灵动岛实时小卡：观察列表的代表标的（首只）在锁屏与灵动岛
常驻显示 最新价 + 涨跌% + 更新时间；点击深链回应用行情页。数据源
= 应用内已加载的行情快照（进程内更新为主，推送更新为 §5 的可选增强）。

激活纪律（对齐 L512/L520 隐私姿态）：**opt-in**——设置页显式开启
「实时活动」才 start；默认关；锁定态下 Lock Screen 卡片内容只含
价格数字（无持仓/账户语义数据，无泄漏面）。

## 2. ActivityAttributes 契约（Swift，Widget Extension 与 App 共享）

```swift
// AlphaQuoteActivityAttributes.swift（App 与 Widget Extension 两个 target 共享）
import ActivityKit

struct AlphaQuoteActivityAttributes: ActivityAttributes {
    struct ContentState: Codable, Hashable {
        let price: Decimal          // 精度：Decimal 而非 Double（金额口径）
        let changePct: Double       // 相对开盘的涨跌百分比
        let updatedAt: Date
    }
    // 固定属性：活动生命周期内不变
    let symbol: String              // 如 "600519"
    let name: String                // 如 "贵州茅台"
}
```

Info.plist（App target）：

```xml
<key>NSSupportsLiveActivities</key><true/>
<!-- 可选：允许频率增强更新（超过默认预算的更新会被系统节流） -->
<key>NSSupportsLiveActivitiesFrequentUpdates</key><false/>
```

## 3. 启停与更新（App 侧，AlphaViewModel 扩展点）

```swift
import ActivityKit

extension AlphaViewModel {
    func startActivity(symbol: String, name: String) throws {
        guard ActivityAuthorizationInfo().areActivitiesEnabled else { return }
        let attributes = AlphaQuoteActivityAttributes(symbol: symbol, name: name)
        let state = AlphaQuoteActivityAttributes.ContentState(
            price: latestPrice, changePct: latestChangePct, updatedAt: .now)
        _ = try Activity.request(attributes: attributes,
                                 contentState: state,
                                 pushType: nil)   // §5 接入后改 .token
    }

    func updateActivities(price: Decimal, changePct: Double) async {
        for activity in Activity<AlphaQuoteActivityAttributes>.activities {
            let state = AlphaQuoteActivityAttributes.ContentState(
                price: price, changePct: changePct, updatedAt: .now)
            await activity.update(using: state)
        }
    }

    func endActivities() async {
        for activity in Activity<AlphaQuoteActivityAttributes>.activities {
            await activity.end(dismissalPolicy: .immediate)
        }
    }
}
```

更新接线：行情 tick 到达处（现 AlphaViewModel 的行情刷新路径）调用
`updateActivities`；设置关闭/退出门时 `endActivities`。

**更新预算（实测口径，落地时复核）**：进程内 update 无硬性次数限制，
但系统对高频更新节流（Lock Screen 卡片更新建议 ≥1 分钟级；灵动岛同）。
行情节拍天然 >1s，App 侧做节流：距上次 update \<60s 则只记内存态、
下一个允许窗口合并提交。`staleDate` 设为 `now + 120s`（行情过期灰显）。

## 4. 灵动岛与锁屏 UI（Widget Extension 内）

```swift
// AlphaQuoteLiveActivity.swift（Widget Extension target）
struct AlphaQuoteLiveActivity: Widget {
    var body: some WidgetConfiguration {
        ActivityConfiguration(for: AlphaQuoteActivityAttributes.self) { ctx in
            // 锁屏/灵动岛展开底部卡片
            HStack { Text(ctx.attributes.name)
                     Spacer()
                     Text("\(ctx.state.price)")
                     Text(String(format: "%+.2f%%", ctx.state.changePct))
                       .foregroundColor(ctx.state.changePct >= 0 ? .red : .green) }
        } dynamicIsland: { ctx in
            DynamicIsland {
                DynamicIslandExpandedRegion(.leading) { Text(ctx.attributes.symbol) }
                DynamicIslandExpandedRegion(.trailing) {
                    Text(String(format: "%+.2f%%", ctx.state.changePct)) }
                DynamicIslandExpandedRegion(.center) { Text("\(ctx.state.price)") }
            } compactLeading: { Text(ctx.attributes.symbol) }
            compactTrailing: { Text(String(format: "%+.2f%%", ctx.state.changePct)) }
            minimal: { Text(String(format: "%+.2f%%", ctx.state.changePct)) }
        }
    }
}
```

A股色板（红涨绿跌）与 Web/Android 一致（docs/theme-adaptation.md 语义）。
深链：`.widgetURL(URL(string: "alpha://quote/\(ctx.attributes.symbol)"))`。

## 5. 推送更新（可选增强，服务端接缝）

进程内更新只在应用存活时有效；后台持续更新需 APNs Live Activity 推送
（`pushType: .token`，拿 `activity.pushToken`，服务端经 APNs 以
`content-state` 载荷更新）。接缝分工：

- **客户端**（L119 面）：`pushType: .token` + token 上报（随 L476 账户
  系统的设备注册一起做，与 L337 推送通道同管）；
- **服务端**（届时立项）：alert-webhook/real-time-feed 增加一个
  `live-activity` 出口——把 quote tick 组装成 APNs
  `{"aps": {"timestamp": ..., "content-state": {...}}}` payload。**本项
  不预写服务端代码**（无客户端 token 流，写了也是未验证面）。

## 6. 验收与测试口径（iOS 解封后）

1. 设备（A14+/iOS 16.1+）设置页开启「实时活动」→ 锁屏见卡片；
2. 灵动岛三形态（compact/minimal/expanded）内容正确、红涨绿跌；
3. 更新节流：\<60s 的 tick 合并提交（埋点计数，Xcode Instruments 或日志）；
4. 设置关闭 → `endActivities` 立即移除；应用被杀后卡片 stale 灰显；
5. `xcodebuild test`：ContentState Codable 往返 + 节流纯逻辑单测
   （Swift 侧，随 L119 门禁）。

## 7. 与相邻项

- **L119**：iOS 目录唯一交付面，本文 §2–§4 即其实现清单；
- **L512/L520**：opt-in 纪律与权限台账（Live Activities 无需用户权限
  条目，`NSSupportsLiveActivities` 是能力声明不是权限）；
- **L337**：告警触发 Live Activity 的开始/结束（届时按钮接 §3 API）；
- **L476**：push token 上报通道复用其设备注册。
