package com.alpha.finance.mobile

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
}
