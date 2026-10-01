# 移动端触屏手势与 UI 交互优化（TODO L367）

口径：**设计文档 + 最小可编译骨架 + 单测**（沿 L118/L301/L337 既定口径）——触屏手势是
**平台壳职责**（架构文档 §13 行 273 已定性，核心库不参与），故本轮 **Rust 侧零改动**：
手势全部落在 Kotlin/Compose 壳层，触发的业务编排复用 L337 既有的 FFI 六方法与
L118 三方法（构造器与全部 FFI 签名原样——L119 iOS 壳在用），**不新增 FFI 方法**。
真机手势实际响应（触摸延迟、惯性滚动、PullToRefresh 视觉）仍是登记的验收边界（§8）。

上游依托：docs/mobile-core-architecture.md（FFI 面/线程模型/壳层纪律）、
docs/mobile-push-sync.md（sync_plan/mark_synced 编排面）、L301 Android 壳骨架。

## 1. 职责分层（一句话）

**手势识别与交互编排全在壳层（Kotlin/Compose），业务决策仍只经 FFI 落在 Rust。**
壳层把手势 event 翻译成「该调哪个 FFI 方法」的编排，不复制闸门/去重/校验逻辑——
与「观察列表是核心库唯一业务判断」（架构文档 §7）、「推送/同步决策在核心库」
（mobile-push-sync.md §1）同一纪律。

```
┌─ 平台壳（android，Compose 手势识别 + 交互编排）─────────────────┐
│  PullToRefreshBox / combinedClickable(onLongClick/onDoubleClick)  │
│  → GestureAction → RefreshGateway 编排（调现有 FFI）              │
└───────────────▲──────────────────────────────────────────────────┘
                │ quotes() / syncPlan("manual") / markSynced() / analyze()
┌─ alpha-mobile（L118 三方法 + L337 六方法，本轮零改动）────────────┐
│  MobileCore：观察列表校验 + 分析引擎 + 推送/同步决策              │
└──────────────────────────────────────────────────────────────────┘
```

## 2. 手势集与触发动作（假设，本环境自行判定）

| 手势 | 触发动作 | Compose API | FFI/bridge 编排 |
|---|---|---|---|
| **下拉刷新** | 重拉全列表快照 + 出 Manual 同步计划 + 标记已同步 | `PullToRefreshBox`（material3 1.3.x，`ExperimentalMaterial3Api`） | `quotes(symbols)` → `syncPlan("manual")` → `plan.due` 则 `markSynced()` 否则 `syncStatus()` |
| **长按行情行** | 触发该标的技术分析（替代/补充既有「分析」按钮） | `Modifier.combinedClickable(onLongClick=)`（foundation，`ExperimentalFoundationApi`） | `analyze(symbol)` |
| **双击状态头** | 折叠/展开状态行详情（纯 UI 状态，不触 FFI） | `Modifier.combinedClickable(onDoubleClick=)` | （local-only） |

三个手势覆盖「触屏手势」（下拉/长按/双击三种触控模式）与「UI 交互优化」
（下拉刷新占位、状态头折叠、长按替代按钮减少误触）。横向滑动（swipe-to-dismiss）
**不引入**：观察列表删除超出核心库能力（symbols 构造期传入，壳层无删除 FFI），
自造删除语义会绕过观察列表边界（架构文档 §7），故不造。

## 3. 页面范围与交互优化

沿用 L301 既有单页（`MainActivity` 的 `MarketScreen`：状态头 + 行情 `LazyColumn` +
行内分析摘要）。本轮交互优化：

1. **状态头**：双击折叠/展开 `api_url` 详情行（默认展开，双击收起节省垂直空间——
   小屏首屏优先给行情列表）。
2. **行情行**：长按触发分析（与既有「分析」按钮并存——按钮保留存档兼容，长按是
   触屏自然路径）；分析结果仍展在列表下方摘要区。
3. **整列表**：`PullToRefreshBox` 包裹 `LazyColumn`，下拉触发刷新编排（§4）+ 顶部
   占位指示器；刷新失败沿用 `AlphaBridge.Error` 转文案不崩壳。

页面范围**不扩**：本轮不新增二级页/导航（导航骨架归后续 TODO，见 §9）。

## 4. 决策与编排（Rust 零改动，FFI 复用）

壳层手势编排落在 `Gestures.kt`，分两件可测的纯逻辑：

1. **`GestureAction` 枚举 + `targetBridgeMethod()` 契约映射**——把每个手势点名到它
   该调的 bridge 方法（或 `local-only`），是壳层「手势→FFI」翻译面的契约源；JVM 单测
   逐分支点名（§6）。
2. **`RefreshGateway` 接口 + `refreshWithManualSync()` 编排**——下拉刷新的 FFI 调用
   序列。抽成接口便于 JVM 单测注入 fake（真机由 `AlphaBridge` 实现，签名与既有方法
   一致——`AlphaBridge` `implement RefreshGateway` 不破坏既有 API）：

```kotlin
interface RefreshGateway {
    suspend fun quotes(symbols: List<String>): List<QuotePayload>
    suspend fun syncPlan(trigger: String): SyncPlanPayload
    suspend fun markSynced(): SyncStatusPayload
    suspend fun syncStatus(): SyncStatusPayload
}

suspend fun refreshWithManualSync(
    gateway: RefreshGateway,
    symbols: List<String>,
): RefreshOutcome {
    val refreshed = gateway.quotes(symbols)               // 重拉快照
    val plan = gateway.syncPlan(AlphaBridge.TRIGGER_MANUAL) // Manual 不受间隔闸门
    val status = if (plan.due) gateway.markSynced() else gateway.syncStatus()
    return RefreshOutcome(refreshed, plan, status)
}
```

编排纪律：**Manual 触发无条件出计划**（L337 闸门口径，`sync.rs`），壳层不重复判断
间隔；`plan.due` 在 Manual 下恒为 true（骨架期安全网，防未来触发口径变化）。真实
取数仍归 L339（骨架期 `mark_synced` 用演示行情重算指纹，见 mobile-push-sync.md §8④）。

## 5. 线程与错误

- 手势回调在主线程（Compose 触摸分发），FFI 编排一律切 `Dispatchers.IO`
  （`AlphaBridge` 既有方法已切，`refreshWithManualSync` 经 `gateway` 间接复用）。
- 错误沿用 `AlphaBridge.Error`：刷新/分析失败 → 文案不崩壳（与 L301 既有 `message`
  状态一致）。`refreshWithManualSync` 不捕异常——让调用方（`MarketScreen`）按既有
  `catch` 统一处理，不发明第二套降级口径。

## 6. 测试策略

| 层 | 形式 | 覆盖 | 跑在哪 |
|---|---|---|---|
| JVM 单测 | `GestureMappingTest.kt` | `GestureAction.targetBridgeMethod()` 各分支契约 + `refreshWithManualSync` 编排（fake `RefreshGateway` 断言调用序列与 `due` 分支） | 本机 `gradlew testDebugUnitTest`（CI 无 SDK 靠契约守门） |
| 契约守门 | `mobile/tests/android_shell_contract.rs` | `Gestures.kt` 在 + `GestureAction`/`RefreshGateway`/`refreshWithManualSync` 名字在 + `MainActivity` 挂接手势 modifier（PullToRefresh/combinedClickable）+ 壳层纪律（Gestures.kt 不 import uniffi） | CI workspace 门禁 |
| 真机 | 手势实际响应 | §8 边界 | 不覆盖 |

## 7. 非交互假设（自行判定，已注明）

1. 本轮台账记 L367（沿 TODO 行号；行号漂移不作编号依据）。
2. **Rust 核心库零改动**：手势是平台壳职责（架构文档 §13 行 273），FFI 全复用
   L118/L337 既有面，不新增方法。
3. 手势集三选（下拉刷新/长按分析/双击折叠）覆盖触控三模式且全部可经现有 FFI 编排；
   横向滑动不引入（删除观察列表超出核心库能力，自造会绕过 §7 边界）。
4. 下拉刷新编排调 `syncPlan("manual")` 而非 `periodic`——Manual 不受间隔闸门
   （L337 口径），符合「用户手势立即响应」预期。
5. `RefreshGateway` 接口仅作 JVM 单测 fake 注入面，不改变 `AlphaBridge` 既有公共
   方法签名（`implement` 一等公民，既有调用方零感知）。
6. 页面范围不扩（沿用 L301 单页），导航/二级页归后续 TODO（§9）。
7. `PullToRefreshBox`/`combinedClickable`/`ModalBottomSheet` 等 experimental API
   的 `@OptIn` 标注随文件进库，不全局放开。

## 8. 真机验收边界（本环境不可观察，登记）

1. 触摸延迟与惯性滚动（Compose 手势分发到 `LazyColumn` 的实际手感）。
2. `PullToRefreshBox` 占位指示器的视觉与触发阈值（material3 默认参数在真机表现）。
3. 长按与双击的冲突判定（系统长按阈值、双击间隔窗口在真机的实际边界）。
4. 小屏/大屏适配（折叠后状态头布局、`LazyColumn` 在小屏的滚动表现）。
5. 刷新失败时的占位指示器收起时机（`isRefreshing` 状态在异常路径的复位）。

## 9. 后续 TODO 映射

| TODO 项 | 本文依托 |
|---|---|
| 移动端离线存储与同步（L339） | §4 刷新编排的 `markSynced` 状态；真实取数接入只换 gateway 实现 |
| 跨平台 UI 框架 / 移动 UI 组件化 | §3 单页范围；导航/二级页骨架 |
| 智能告警与个性化推送（L418） | §2 长按分析是告警规则入口的自然位置（长按 → 设规则而非仅看分析） |
