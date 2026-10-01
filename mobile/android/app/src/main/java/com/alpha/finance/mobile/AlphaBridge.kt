package com.alpha.finance.mobile

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.decodeFromString
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json
import uniffi.alpha_mobile.MobileCore
import uniffi.alpha_mobile.MobileException

/**
 * Rust 核心库消费面（L301；L337 增推送/同步六透传）：持有 UniFFI 生成的
 * [MobileCore]，是壳层接触 Rust 的唯一入口。所有 FFI 调用一律切
 * [Dispatchers.IO]——绑定本身无平台线程约束，但 `analyze_json` 走整段 K 线
 * 计算，主线程直调会卡帧（架构文档 §6 线程模型）。生成绑定的
 * [MobileException] 在 [translate] 处就地翻译成 [Error]——UI 层只认
 * Kotlin 侧错误面，不 import 任何 uniffi 类型。推送/同步的平台送达与
 * WorkManager 调度接缝见 [PushSyncSeam]。
 */
class AlphaBridge(symbols: List<String>, apiUrl: String) : AutoCloseable, RefreshGateway {
    /** Kotlin 侧错误面：Rust 语义不在 UI 层重复实现，只做类型搬运与文案兜底 */
    sealed class Error(message: String) : Exception(message) {
        /** 观察列表外（核心库唯一业务判断的镜像） */
        class InvalidSymbol(val symbol: String) : Error("标的 $symbol 不在观察列表")

        /** 其余核心错误（Rust Display 全文透传） */
        class Failed(message: String) : Error(message)
    }

    private val core = MobileCore(symbols, apiUrl)
    private val json = Json { ignoreUnknownKeys = true }

    companion object {
        /**
         * 同步触发串（docs/mobile-push-sync.md §3/§8⑥；serde 契约——未知值
         * Rust 侧抛 [Error.Failed]）。
         */
        const val TRIGGER_PERIODIC = "periodic"
        const val TRIGGER_FOREGROUND = "foreground"
        const val TRIGGER_CONNECTIVITY_RESTORED = "connectivity_restored"
        const val TRIGGER_MANUAL = "manual"
    }

    /** [MobileException] → [Error]：除类型搬运不加任何逻辑 */
    private inline fun <T> translate(block: () -> T): T =
        try {
            block()
        } catch (e: MobileException.InvalidSymbol) {
            throw Error.InvalidSymbol(e.symbol)
        } catch (e: MobileException.Failed) {
            throw Error.Failed(e.detail)
        } catch (e: MobileException) {
            throw Error.Failed(e.message ?: "alpha-mobile: 未知错误")
        }

    /** 核心库状态（版本 + 观察列表 + 后端地址） */
    suspend fun status(): StatusPayload = withContext(Dispatchers.IO) {
        translate { json.decodeFromString(core.statusJson()) }
    }

    /** 观察列表内标的的快照行情；列表外抛 [Error.InvalidSymbol] */
    suspend fun quote(symbol: String): QuotePayload = withContext(Dispatchers.IO) {
        translate { json.decodeFromString(core.quoteJson(symbol)) }
    }

    /** 观察列表内标的的技术分析；列表外抛 [Error.InvalidSymbol] */
    suspend fun analyze(symbol: String): AnalysisPayload = withContext(Dispatchers.IO) {
        translate { json.decodeFromString(core.analyzeJson(symbol)) }
    }

    /** 逐标的快照；单项失败（列表外/核心错误）按项降级，不拖垮整屏 */
    override suspend fun quotes(symbols: List<String>): List<QuotePayload> = symbols.mapNotNull { symbol ->
        try {
            quote(symbol)
        } catch (ignored: Error) {
            null
        }
    }

    // ── L337 推送/同步六透传（docs/mobile-push-sync.md §4/§7；只增不改）──
    // 决策全在 Rust 侧，壳层只做 JSON↔载荷搬运与平台送达/调度（PushSyncSeam）。

    /** 设置价格告警规则（整体替换，返回规则条数）；列表外抛 [Error.InvalidSymbol] */
    suspend fun setAlertRules(rules: List<AlertRulePayload>): ULong =
        withContext(Dispatchers.IO) {
            val payload: String = json.encodeToString(rules)
            translate { core.setAlertRulesJson(payload) }
        }

    /** 评估一轮告警（Rust 侧判定触发与去重，入队待取） */
    suspend fun checkAlerts(): AlertsReportPayload = withContext(Dispatchers.IO) {
        translate { json.decodeFromString<AlertsReportPayload>(core.checkAlertsJson()) }
    }

    /** 取走待送达通知（取走即空；平台弹出交 [PushSyncSeam.NotificationDispatcher]） */
    suspend fun takePending(): List<NotificationSpecPayload> = withContext(Dispatchers.IO) {
        translate { json.decodeFromString<TakenPayload>(core.takePendingJson()).taken }
    }

    /** 同步状态（due 按 periodic 口径） */
    override suspend fun syncStatus(): SyncStatusPayload = withContext(Dispatchers.IO) {
        translate { json.decodeFromString<SyncStatusPayload>(core.syncStatusJson()) }
    }

    /**
     * 出同步计划；trigger ∈ `periodic` / `foreground` / `connectivity_restored` /
     * `manual`，未知值抛 [Error.Failed]。壳层拿 due=true 的计划后执行平台取数
     * 并回 [markSynced]（骨架期可空转，取数执行归 L339）。
     */
    override suspend fun syncPlan(trigger: String): SyncPlanPayload = withContext(Dispatchers.IO) {
        translate { json.decodeFromString<SyncPlanPayload>(core.syncPlanJson(trigger)) }
    }

    /** 标记已同步（Rust 侧记时刻 + 重算指纹），返回更新后的状态 */
    override suspend fun markSynced(): SyncStatusPayload = withContext(Dispatchers.IO) {
        translate { json.decodeFromString<SyncStatusPayload>(core.markSyncedJson()) }
    }

    /** 释放 Rust 侧 Arc；Activity 销毁时调用（泄漏面见生成绑定文档） */
    override fun close() = core.destroy()
}
