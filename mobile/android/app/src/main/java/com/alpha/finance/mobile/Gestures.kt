package com.alpha.finance.mobile

/**
 * 触屏手势与交互编排（L367，docs/mobile-gestures.md）：手势识别在 Compose 层
 * （MainActivity 的 modifier 挂接），业务编排只调 [AlphaBridge] 既有 FFI 方法——
 * **不新增 FFI、不复制 Rust 侧决策**（观察列表/闸门/去重仍在核心库，架构文档 §7
 * 与 mobile-push-sync.md §1 同一纪律）。本文件是纯逻辑（零 Compose/uniffi 依赖），
 * 因此 JVM 单测可直接覆盖（GestureMappingTest.kt）。
 */

/**
 * 手势动作（壳层「手势 → 该调哪个 FFI」的翻译源；枚举值对应 docs/mobile-gestures.md §2 表）
 */
enum class GestureAction {
    /** 下拉刷新：重拉全列表快照 + 出 Manual 同步计划 + 标记已同步 */
    PullToRefresh,

    /** 长按行情行：触发该标的技术分析 */
    LongPressAnalyze,

    /** 双击状态头：折叠/展开 api_url 详情行（纯 UI，不触 FFI） */
    DoubleTapHeader,
}

/**
 * 动作 ↔ bridge 方法契约（`target_bridge_method` 是壳层「手势→FFI」翻译面的
 * 契约源，JVM 单测逐分支点名、`android_shell_contract.rs` 守门）
 *
 * - [GestureAction.PullToRefresh] → `quotes + syncPlan + markSynced`（编排见 [refreshWithManualSync]）
 * - [GestureAction.LongPressAnalyze] → `analyze`
 * - [GestureAction.DoubleTapHeader] → `local-only`（纯 UI 状态，不触 FFI）
 */
fun GestureAction.targetBridgeMethod(): String = when (this) {
    GestureAction.PullToRefresh -> "quotes+syncPlan+markSynced"
    GestureAction.LongPressAnalyze -> "analyze"
    GestureAction.DoubleTapHeader -> "local-only"
}

/**
 * 刷新编排的依赖面（[AlphaBridge] 实现；抽接口便于 JVM 单测注入 fake——真机
 * `AlphaBridge` 持 uniffi `MobileCore`，JVM 测试环境无 `.so` 不可构造，见 §7⑤）
 */
interface RefreshGateway {
    /** 逐标的重拉快照（观察列表内；列表外按项降级） */
    suspend fun quotes(symbols: List<String>): List<QuotePayload>

    /** 出同步计划（刷新固定用 [AlphaBridge.TRIGGER_MANUAL]——Manual 不受间隔闸门） */
    suspend fun syncPlan(trigger: String): SyncPlanPayload

    /** 标记已同步（Rust 侧记时刻 + 重算指纹） */
    suspend fun markSynced(): SyncStatusPayload

    /** 同步状态（`plan.due=false` 时的回退读取） */
    suspend fun syncStatus(): SyncStatusPayload
}

/** 刷新编排结果（UI 层一次性应用三个载荷，不拆三次写状态） */
data class RefreshOutcome(
    val quotes: List<QuotePayload>,
    val plan: SyncPlanPayload,
    val status: SyncStatusPayload,
)

/**
 * 下拉刷新编排（docs/mobile-gestures.md §4）：
 *
 * 1. `quotes(symbols)` 重拉快照；
 * 2. `syncPlan(Manual)` 出计划——Manual 恒 `due=true`（L337 闸门口径，壳层不
 *    重复判断间隔）；
 * 3. `plan.due` 则 `markSynced()` 否则 `syncStatus()`（安全网：未来触发口径变化
 *    时不误标）。
 *
 * 真实取数归 L339——骨架期 `markSynced` 以演示行情重算指纹
 * （mobile-push-sync.md §8④）。异常不捕获：由调用方按既有 `AlphaBridge.Error`
 * 口径统一降级，不发明第二套错误处理（§5）。
 */
suspend fun refreshWithManualSync(
    gateway: RefreshGateway,
    symbols: List<String>,
): RefreshOutcome {
    val refreshed = gateway.quotes(symbols)
    val plan = gateway.syncPlan(AlphaBridge.TRIGGER_MANUAL)
    val status = if (plan.due) gateway.markSynced() else gateway.syncStatus()
    return RefreshOutcome(refreshed, plan, status)
}
