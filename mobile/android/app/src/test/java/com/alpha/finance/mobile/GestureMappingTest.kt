package com.alpha.finance.mobile

import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * 手势契约与刷新编排单测（JVM 级，无需设备/.so，docs/mobile-gestures.md §6）：
 * 锁定 [GestureAction.targetBridgeMethod] 逐分支契约与 [refreshWithManualSync]
 * 的 FFI 调用序列——fake [RefreshGateway] 断言调用顺序与 `plan.due` 分支。
 * Compose 手势实际响应（触摸延迟/阈值视觉）归真机边界（§8）。
 */
class GestureMappingTest {

    /** 手势 → bridge 方法契约（契约源；android_shell_contract.rs 同款断言守 CI） */
    @Test
    fun gesture_actions_map_to_bridge_methods() {
        assertEquals("quotes+syncPlan+markSynced", GestureAction.PullToRefresh.targetBridgeMethod())
        assertEquals("analyze", GestureAction.LongPressAnalyze.targetBridgeMethod())
        assertEquals("local-only", GestureAction.DoubleTapHeader.targetBridgeMethod())
    }

    /** fake gateway：记录调用序列与入参，按脚本返回载荷 */
    private class FakeGateway(
        val planDue: Boolean,
        val quotedSymbols: List<String> = emptyList(),
    ) : RefreshGateway {
        val calls = mutableListOf<String>()
        var receivedSymbols: List<String> = emptyList()

        override suspend fun quotes(symbols: List<String>): List<QuotePayload> {
            calls += "quotes"
            receivedSymbols = symbols
            return quotedSymbols.map {
                QuotePayload(
                    symbol = it,
                    timestamp = "2026-10-01T00:00:00Z",
                    price = 91.0,
                    volume = 1_000,
                )
            }
        }

        override suspend fun syncPlan(trigger: String): SyncPlanPayload {
            calls += "syncPlan:$trigger"
            return SyncPlanPayload(
                due = planDue,
                trigger = trigger,
                symbols = if (planDue) receivedSymbols else emptyList(),
                sinceFingerprint = 0u,
                intervalSecs = 300u,
                reason = if (planDue) null else "interval_not_elapsed",
            )
        }

        override suspend fun markSynced(): SyncStatusPayload {
            calls += "markSynced"
            return SyncStatusPayload(
                lastSync = "2026-10-01T00:00:00Z",
                fingerprint = 42u,
                intervalSecs = 300u,
                due = false,
            )
        }

        override suspend fun syncStatus(): SyncStatusPayload {
            calls += "syncStatus"
            return SyncStatusPayload(
                lastSync = null,
                fingerprint = 0u,
                intervalSecs = 300u,
                due = true,
            )
        }
    }

    /** 刷新编排（due 分支）：quotes → syncPlan(manual) → markSynced，序列与载荷一次断言 */
    @Test
    fun refresh_runs_full_sequence_when_plan_due() = runBlocking {
        val fake = FakeGateway(planDue = true, quotedSymbols = listOf("600519"))
        val outcome = refreshWithManualSync(fake, listOf("600519", "000001"))

        assertEquals(
            "调用序列固定为 quotes → syncPlan(manual) → markSynced",
            listOf("quotes", "syncPlan:manual", "markSynced"),
            fake.calls,
        )
        assertEquals("观察列表透传", listOf("600519", "000001"), fake.receivedSymbols)
        assertEquals(listOf("600519"), outcome.quotes.map { it.symbol })
        assertEquals("manual", outcome.plan.trigger)
        assertEquals("markSynced 后状态不 due", false, outcome.status.due)
    }

    /** 刷新编排（未到期分支）：markSynced 回退为 syncStatus（安全网，§4③） */
    @Test
    fun refresh_falls_back_to_status_when_plan_not_due() = runBlocking {
        val fake = FakeGateway(planDue = false, quotedSymbols = listOf("600519"))
        val outcome = refreshWithManualSync(fake, listOf("600519"))

        assertEquals(
            "due=false 时不标记已同步，只读状态",
            listOf("quotes", "syncPlan:manual", "syncStatus"),
            fake.calls,
        )
        assertEquals("回退读取的状态未被标记", null, outcome.status.lastSync)
    }
}
