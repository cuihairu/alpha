package com.alpha.finance.mobile

import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * 载荷字段契约测试（JVM 级，不需要设备/.so）：锁定 [MarketModels] 与
 * alpha-core serde 契约一致——样本字段与 mobile/src/state.rs 单测、
 * web/desktop 同一 serde 输出口径。真机 FFI 全链路归验收边界（README）。
 */
class PayloadParsingTest {
    private val json = Json { ignoreUnknownKeys = true }

    @Test
    fun quote_payload_parses_market_data_contract() {
        val payload = json.decodeFromString<QuotePayload>(
            """{"symbol":"600519","timestamp":"2026-09-30T10:00:00Z","price":91.28,""" +
                """"volume":12345,"bid":91.19,"ask":91.37,"open":90.37,"high":91.46,"low":91.1}""",
        )
        assertEquals("600519", payload.symbol)
        assertEquals(91.28, payload.price, 1e-9)
        assertEquals(12345L, payload.volume)
        assertTrue(payload.high!! > payload.low!!)
        assertTrue(payload.bid!! < payload.ask!!)
    }

    @Test
    fun analysis_payload_parses_analysis_result_contract() {
        val payload = json.decodeFromString<AnalysisPayload>(
            """{"symbol":"600519","analyzed_at":"2026-09-30T10:00:00Z",""" +
                """"indicators":[{"name":"RSI","timestamps":["2026-09-30T09:59:00Z"],""" +
                """"values":[51.5],"signals":["None"]}],""" +
                """"risk_metrics":{"volatility":0.02,"sharpe_ratio":null,""" +
                """"max_drawdown":0.1,"beta":null},"recommendation":"Hold","confidence":0.9}""",
        )
        assertEquals("600519", payload.symbol)
        assertEquals("Hold", payload.recommendation)
        assertEquals("RSI", payload.indicators.first().name)
        assertEquals(listOf("None"), payload.indicators.first().signals)
        assertEquals(0.02, payload.riskMetrics.volatility, 1e-9)
        assertEquals(0.1, payload.riskMetrics.maxDrawdown, 1e-9)
        assertEquals(null, payload.riskMetrics.sharpeRatio)
        assertEquals(0.9, payload.confidence, 1e-9)
    }

    @Test
    fun status_payload_parses_config_slot() {
        val payload = json.decodeFromString<StatusPayload>(
            """{"version":"0.1.0","symbols":["600519","000001"],""" +
                """"api_url":"http://localhost:8080"}""",
        )
        assertEquals(listOf("600519", "000001"), payload.symbols)
        assertEquals("http://localhost:8080", payload.apiUrl)
        assertEquals("0.1.0", payload.version)
    }

    /** 未知字段宽容（Rust 侧加字段不崩壳）；已知字段仍须严格解析 */
    @Test
    fun unknown_fields_are_ignored_not_fatal() {
        val payload = json.decodeFromString<StatusPayload>(
            """{"version":"0.1.0","symbols":[],"api_url":"","future_field":42}""",
        )
        assertEquals(0, payload.symbols.size)
    }

    // ── L337 推送/同步载荷契约（docs/mobile-push-sync.md §4）──

    /** 通知载荷六字段（样本为 Rust 侧 NotificationSpec serde 输出口径） */
    @Test
    fun notification_spec_payload_parses_with_dedup_id() {
        val payload = json.decodeFromString<NotificationSpecPayload>(
            """{"id":"600519|above|4042000000000000","kind":"PriceAlert","symbol":"600519",""" +
                """"title":"价格提醒 600519","body":"600519 现价 91.28 已突破目标价 90.00",""" +
                """"created_at":"2026-09-30T10:00:00Z"}""",
        )
        assertEquals("PriceAlert", payload.kind)
        assertEquals("600519", payload.symbol)
        assertTrue("去重键=规则键派生: ${payload.id}", payload.id.startsWith("600519|above|"))
    }

    /** 告警评估报告：fired 数组 + pending 计数（u64 → ULong） */
    @Test
    fun alerts_report_payload_parses_fired_and_pending() {
        val report = json.decodeFromString<AlertsReportPayload>(
            """{"fired":[{"id":"k","kind":"PriceAlert","symbol":"600519","title":"t",""" +
                """"body":"b","created_at":"2026-09-30T10:00:00Z"}],"pending":2}""",
        )
        assertEquals(1, report.fired.size)
        assertEquals(2UL, report.pending)
    }

    /** 待送达队列：taken 取走即空语义由 Rust 侧保证，壳层只解包 */
    @Test
    fun taken_payload_parses_empty_queue() {
        val taken = json.decodeFromString<TakenPayload>("""{"taken":[]}""")
        assertEquals(0, taken.taken.size)
    }

    /** 同步状态四字段；从未同步 last_sync 为 null */
    @Test
    fun sync_status_payload_parses_with_null_last_sync() {
        val status = json.decodeFromString<SyncStatusPayload>(
            """{"last_sync":null,"fingerprint":0,"interval_secs":300,"due":true}""",
        )
        assertEquals(null, status.lastSync)
        assertEquals(0UL, status.fingerprint)
        assertEquals(300UL, status.intervalSecs)
        assertEquals(true, status.due)
    }

    /** 同步计划：未到期带 reason 不带工作项；到期带观察列表 */
    @Test
    fun sync_plan_payload_parses_both_shapes() {
        val gated = json.decodeFromString<SyncPlanPayload>(
            """{"due":false,"trigger":"periodic","symbols":[],"since_fingerprint":42,""" +
                """"interval_secs":300,"reason":"interval_not_elapsed"}""",
        )
        assertEquals(false, gated.due)
        assertEquals("interval_not_elapsed", gated.reason)
        assertEquals(42UL, gated.sinceFingerprint)

        val due = json.decodeFromString<SyncPlanPayload>(
            """{"due":true,"trigger":"manual","symbols":["600519"],"since_fingerprint":42,""" +
                """"interval_secs":300}""",
        )
        assertEquals(listOf("600519"), due.symbols)
        assertEquals(null, due.reason)
    }

    /** 告警规则编码往返：壳层 → JSON → Rust serde 契约（target_price snake_case） */
    @Test
    fun alert_rule_payload_encodes_rust_serde_contract() {
        val rules: List<AlertRulePayload> =
            listOf(AlertRulePayload("600519", 90.5, above = true))
        assertEquals(
            """[{"symbol":"600519","target_price":90.5,"above":true}]""",
            json.encodeToString(rules),
        )
    }
}
