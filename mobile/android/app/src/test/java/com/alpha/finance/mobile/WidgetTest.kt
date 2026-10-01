package com.alpha.finance.mobile

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * 桌面小组件纯逻辑单测（JVM 级，L509）：状态持久化往返与损坏 fail-safe、
 * 涨跌幅现算、渲染格式化与方向判定、过期灰显语义。RemoteViews/
 * AppWidgetManager 设备路径不在此覆盖（无 Android 框架类）。
 */
class WidgetTest {

    private fun quote(
        symbol: String = "600519",
        price: Double = 90.5,
        open: Double? = null,
    ) = QuotePayload(
        symbol = symbol,
        timestamp = "2026-10-02T09:30:00Z",
        price = price,
        volume = 1000,
        open = open,
    )

    // ---------- WidgetStateStore ----------

    @Test
    fun `状态往返与损坏返null`() {
        val kv = InMemoryKeyValueStore()
        val store = WidgetStateStore(kv)
        assertNull("未写入返回 null", store.load())
        store.save(WidgetQuote(symbol = "600519", price = 90.5, changePct = 1.23, updatedAt = "2026-10-02T09:30:00Z"))
        assertEquals(
            WidgetQuote(symbol = "600519", price = 90.5, changePct = 1.23, updatedAt = "2026-10-02T09:30:00Z"),
            store.load(),
        )
        kv.put(WidgetStateStore.KEY, "{not json")
        assertNull("损坏 JSON fail-safe 返 null", store.load())
    }

    // ---------- buildWidgetQuote ----------

    @Test
    fun `取首只标的且由开盘价现算涨跌幅`() {
        val quotes = listOf(
            quote(symbol = "600519", price = 105.0, open = 100.0),
            quote(symbol = "000001", price = 12.0, open = 10.0),
        )
        val state = buildWidgetQuote(quotes)!!
        assertEquals("600519", state.symbol)
        assertEquals(105.0, state.price, 1e-9)
        assertEquals(5.0, state.changePct!!, 1e-9)
        // 空列表 → 不产出状态（widget 留占位）
        assertNull(buildWidgetQuote(emptyList()))
    }

    @Test
    fun `无开盘价或开盘价非正时涨跌幅为null`() {
        assertNull(buildWidgetQuote(listOf(quote(price = 90.5, open = null)))!!.changePct)
        assertNull(buildWidgetQuote(listOf(quote(price = 90.5, open = 0.0)))!!.changePct)
    }

    // ---------- WidgetContent.from ----------

    @Test
    fun `渲染格式化两位小数与涨跌符号`() {
        val c = WidgetContent.from(
            WidgetQuote(symbol = "600519", price = 90.5, changePct = 1.234, updatedAt = "2026-10-02T09:30:00Z"),
            nowMs = 0,
            staleAfterMs = Long.MAX_VALUE,
        )
        assertEquals("600519", c.symbol)
        assertEquals("90.50", c.priceText)
        assertEquals("+1.23%", c.changeText)
        assertEquals(WidgetDirection.UP, c.direction)
        assertFalse(c.stale)

        val down = WidgetContent.from(
            WidgetQuote(symbol = "600519", price = 90.5, changePct = -2.0, updatedAt = "2026-10-02T09:30:00Z"),
            nowMs = 0,
            staleAfterMs = Long.MAX_VALUE,
        )
        assertEquals("-2.00%", down.changeText)
        assertEquals(WidgetDirection.DOWN, down.direction)

        val flat = WidgetContent.from(
            WidgetQuote(symbol = "600519", price = 90.5, changePct = null, updatedAt = "2026-10-02T09:30:00Z"),
            nowMs = 0,
            staleAfterMs = Long.MAX_VALUE,
        )
        assertEquals("--", flat.changeText)
        assertEquals(WidgetDirection.FLAT, flat.direction)
    }

    @Test
    fun `null状态渲染占位`() {
        val c = WidgetContent.from(null, nowMs = 0)
        assertNull(c.symbol)
        assertNull(c.priceText)
        assertEquals(WidgetDirection.NO_DATA, c.direction)
        assertTrue("占位按灰显处理", c.stale)
    }

    @Test
    fun `过期语义——超阈值灰显且时间戳解析失败按过期`() {
        val fresh = WidgetContent.from(
            WidgetQuote(symbol = "600519", price = 90.5, updatedAt = "2026-10-02T09:30:00Z"),
            nowMs = 30_000L + Instant_epochMs("2026-10-02T09:30:00Z"),
        )
        assertFalse("30s 内未过期", fresh.stale)
        val stale = WidgetContent.from(
            WidgetQuote(symbol = "600519", price = 90.5, updatedAt = "2026-10-02T09:30:00Z"),
            nowMs = 121_000L + Instant_epochMs("2026-10-02T09:30:00Z"),
        )
        assertTrue("超 120s 过期", stale.stale)
        val garbage = WidgetContent.from(
            WidgetQuote(symbol = "600519", price = 90.5, updatedAt = "not-a-time"),
            nowMs = 0,
        )
        assertTrue("解析失败 fail-safe 灰显", garbage.stale)
    }

    private fun Instant_epochMs(iso: String): Long =
        WidgetContent.parseTimestampMs(iso)!!
}
