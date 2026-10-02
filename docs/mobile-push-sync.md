# 移动端推送通知与后台同步架构

口径：**设计文档 + 最小可编译骨架 + 单测**（沿 L118/L301 既定口径）——不引入新
付费云服务，推送走**本地通知 + 可插拔通道抽象**；真机/模拟器运行（通知实际弹出、
WorkManager/BGTaskScheduler 实际调度、Doze 降级）仍是登记的验收边界（§10）。
上游依托：docs/mobile-core-architecture.md（FFI 面/错误映射/线程模型/观察列表）；
桌面同源口径：L116 离线同步的**内容指纹增量**（`content_seq(price, volume)`）。

## 1. 职责分层（一句话）

**核心库只做决策（推什么/何时同步），壳层只做送达与调度（怎么弹/何时醒来）。**
业务判断不外溢到壳层——与观察列表是核心库唯一业务判断（架构文档 §7）同一纪律。

```
┌─ 平台壳（android / ios，调度与送达在平台 API 上）────────────────┐
│  Android: NotificationManager(送达) + WorkManager(周期/条件唤醒)   │
│  iOS:     UNUserNotificationCenter(送达) + BGTaskScheduler(唤醒)   │
│  两者都只调 FFI 六方法，不复制决策逻辑                              │
└───────────────▲──────────────────────────────▲──────────────────┘
                │ take_pending_json()          │ sync_plan_json(trigger)
                │ check_alerts_json()          │ sync_status_json()
                │ set_alert_rules_json()       │ mark_synced_json()
┌─ alpha-mobile（决策与状态）───────────────────────────────────────┐
│  notify.rs：AlertRule 评估 + 触发去重 + 通道发送（trait 可插拔）    │
│  sync.rs：  SyncTrigger 四种触发时机 + 间隔闸门 + 内容指纹          │
└──────────────────────────────────────────────────────────────────┘
```

## 2. 推送通道选型

| 通道 | 取舍 | 结论 |
|---|---|---|
| **本地通知**（壳层 NotificationManager / UNUserNotificationCenter） | 零依赖、零成本、离线可用；App 在前台时由壳层转为应用内提示 | **默认通道**（本轮落地） |
| 远程推送 FCM / APNs | FCM 免费但需 Play services 与 Google 项目；APNs 需 Apple 证书与签名链 | **不引入**（付费/账号/签名依赖）；留作 `NotificationChannel` 的后续实现（§6 可插拔点） |
| 自建推送（长连接 + 服务端） | 本仓已有 real-time-feed WS 面，但移动端需自管心跳/重连 | 归后续 TODO；不属「推送通道」骨架范围 |

Rust 侧抽象：`trait NotificationChannel`（`send(&NotificationSpec)`）——
`LocalQueueChannel`（入队，壳层经 `take_pending_json` 取走送平台 API）是默认
实现；单测用 `RecordingChannel` 证明可插拔（换实现不改决策逻辑）。
与 alpha-core `platform::UserNotification` 的关系：后者是四端通用的**语义面**
（async、`title+body`、零平台类型）；本 trait 是移动端**送达面**（同步、结构化
载荷含去重键），壳层送达成功后即完成 UserNotification 的移动端语义——两者不
互相依赖，避免把 async 梯子拖进 FFI 面。

## 3. 后台同步触发时机（假设，本环境自行判定）

| 触发 | 谁感知 | 间隔闸门 | 壳层映射 |
|---|---|---|---|
| `Periodic` | 壳层定时器 | **生效**（默认 300s，到期才 due） | Android `PeriodicWorkRequest`（系统钳制最短 15min，见假设③）/ iOS `BGAppRefreshTask` |
| `AppForeground` | 壳层生命周期 | 不生效（回前台立即查） | Android `Lifecycle` / iOS `scenePhase` |
| `ConnectivityRestored` | 壳层网络回调 | 不生效 | Android `NetworkCallback` / iOS `NWPathMonitor` |
| `Manual` | 用户手势 | 不生效 | 下拉刷新 |

决策规则（`BackgroundSync::decide_at`，时间可注入便于单测）：

1. `Periodic`：`now - last_sync >= interval` 才出计划，否则 `due=false` + `reason`；
2. 其余三种触发无条件出计划（回前台/恢复网络/手动刷新没有理由拒绝）；
3. 计划携带观察列表 + `since_fingerprint`（上次同步的内容指纹，增量语义起点）；
4. 壳层执行平台侧取数后调 `mark_synced_json()`：核心库以**当前演示行情**重算
   指纹并记 `last_sync=now`——指纹算法不外泄，壳层只透传。

指纹口径沿 L116：`content_seq(price, volume)` 的 FNV-1a，逐标的聚合；真实数据
接入后（L339 离线存储）由远端水位替代——骨架期演示行情确定性，指纹变化测试
用注入数据驱动（§8）。

## 4. 数据契约（JSON 载荷，单测逐字段点名）

FFI 六方法（**只增不改**，L118 三方法与构造器签名原样——L119 iOS 壳在用）：

| 方法 | 入参 | 载荷 |
|---|---|---|
| `set_alert_rules_json` | `[{symbol,target_price,above}]` | 返回规则条数；列表外 `InvalidSymbol`；`target_price<=0` 拒 |
| `check_alerts_json` | — | `{fired:[NotificationSpec], pending:usize}`（评估并入队） |
| `take_pending_json` | — | `{taken:[NotificationSpec]}`（壳层送达队列，取走即空） |
| `sync_status_json` | — | `{last_sync,fingerprint,interval_secs,due}` |
| `sync_plan_json` | trigger 字符串 | `{due,trigger,symbols,since_fingerprint,interval_secs,reason?}` |
| `mark_synced_json` | — | 同 status（`last_sync=now`、指纹重算） |

`NotificationSpec`：`{id, kind, symbol, title, body, created_at}`；`id` 稳定可去重
（规则键派生），`kind` 首期仅 `PriceAlert`（`SignalChange` 等智能告警归 L418）。

## 5. 触发去重语义（防通知轰炸）

按规则**穿越一次发一次**：条件成立且未发 → 发并记入 `fired`；条件仍成立不重发；
条件回落 → 出 `fired` 重装（再次穿越可再发）。与桌面 alerts 只持久化不弹的差异
已在 L114 注明（移动端通知是平台能力，桌面归托盘/系统通知 TODO）。

## 6. 线程模型与错误映射

- 决策状态（规则/已发集/同步状态）各挂一把 `Mutex`，`MobileCore` 保持
  `Send + Sync`（uniffi 多线程调用安全）；无运行期句柄、无异步。
- 壳层调用照旧切后台（AlphaBridge `Dispatchers.IO`；主线程直调卡帧是 §12③ 边界）。
- 错误沿用 `MobileError`：观察列表外规则 → `InvalidSymbol`；JSON 解析/未知触发
  字符串/非法阈值 → `Failed{detail}`（Display 全文）。

## 7. 与两壳的接缝

- **Android（本轮实现）**：`AlphaBridge` 加六方法透传（JSON ↔ kotlinx-serialization
  载荷模型）；`PushSyncSeam.kt` 给出接缝面——`NotificationDispatcher` 接口（平台
  送达实现留真机 TODO）+ 同步触发入口注释（WorkManager 映射见 §3）。
  结构由 `mobile/tests/android_shell_contract.rs` 守门（CI 无 Android SDK）。
- **iOS（本轮不实现，文档定接缝）**：L119 壳在其轨道内按同一六方法模式接
  `UNUserNotificationCenter`/`BGTaskScheduler`；接缝 = `AlphaViewModel` 持
  `MobileCore` 直调 + 后台队列（Swift 无强线程约束，遵循架构文档 §6 主线程禁忌）。
  本轮**不动 mobile/ios**（L119 会话的交付面）。

## 8. 非交互假设（自行判定，已注明）

1. 本轮台账记 L337（沿 dispatch 顺序；行号漂移不作编号依据）。
2. **不引入任何云推送**：默认本地通知，远程通道留 trait 实现位（§2）。
3. 默认同步间隔 300s 为骨架值；Android WorkManager 周期任务系统钳制 ≥15min、
   iOS BGTask 窗口由系统调度——**间隔一致性不作保证**，核心库 `interval_secs`
   只作自身闸门，壳层实际周期 ≥ 它即兼容（真机实测归边界 §10）。
4. 同步的「取数」在骨架期无真实远端（api_url 仍为配置槽，取数执行归 L339）；
   计划/闸门/指纹三件套先立契约，壳层拿到计划后调平台网络（骨架期可空转）。
5. 指纹沿 L116 `content_seq(price, volume)` 口径，聚合顺序按**排序后的
   symbol**（与观察列表传入顺序无关，避免壳层排序差异导致指纹漂移）。
6. `SyncTrigger` 字符串枚举（`periodic`/`foreground`/`connectivity_restored`/
   `manual`）而非 FFI 枚举类型——与 JSON 桥纪律一致，未知值走 `Failed`。
7. 评估用当前演示行情（确定性，与 L118 market 模块同源）；真实行情接入只换
   数据源不动决策面。
8. 规则集可整体替换（`set_alert_rules_json` 后写覆盖）；骨架不做规则持久化
   （归 L339 离线存储的 kv 快照）。

## 9. 骨架与单测清单

```
mobile/src/notify.rs   AlertRule / NotificationSpec / NotificationChannel(trait)
                       / LocalQueueChannel / Notifier（+单测 11 例）
mobile/src/sync.rs     SyncTrigger / SyncConfig / BackgroundSync / fingerprint（+单测 10 例）
mobile/src/state.rs    MobileCore 挂 Mutex<Notifier>+Mutex<BackgroundSync>，FFI 六方法（+单测 6 例）
mobile/android/.../    MarketModels 六载荷 + AlphaBridge 六透传（含 TRIGGER_* 常量）+ PushSyncSeam.kt
mobile/tests/          android_shell_contract.rs 追加接缝守门（+1 例，共 10 例）
```

## 10. 真机验收边界（本环境不可观察，登记）

1. 通知实际弹出/静音通道/前台转应用内提示（Android O 通知渠道、iOS 授权流）。
2. WorkManager ≥15min 钳制、Doze/App Standby 降级下的实际触发频率；iOS
   BGTaskScheduler 的执行窗口与 `BGTaskSchedulerPermittedIdentifiers` 配置。
3. `ConnectivityRestored` 在真网络切换下的表现（骨架只有接缝，无真实网络栈）。
4. 远程通道（FCM/APNs）接入时的凭据链与可靠性——归后续 TODO，本轮不引入。
5. 送达失败降级（通道满/权限拒绝）重试策略——骨架只返回错误，不带重试。

## 11. 后续 TODO 映射

| TODO 项 | 本文依托 |
|---|---|
| L339 离线数据存储与同步 | §3 指纹增量 + `mark_synced` 状态；kv 快照沿桌面 L116 |
| L418 智能告警与个性化推送 | §5 去重语义 + `kind` 扩 `SignalChange`（分析建议翻转即推） |
| 发布流水线（Android/iOS） | §10 通知权限/渠道声明随 APK/AAB 配置 |
