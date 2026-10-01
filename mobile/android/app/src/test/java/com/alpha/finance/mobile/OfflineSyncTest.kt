package com.alpha.finance.mobile

import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * 离线数据壳层执行面单测（JVM 级，无需设备/.so，docs/mobile-offline.md §7）：
 * kv 语义（后写覆盖/删除幂等）、备份落盘恢复往返、红线第二道防线（未授权
 * 载荷拒收）、明示文案、备份↔同步载荷术语分离。红线第一道防线（默认关闭/
 * 开启须范围）在 Rust 侧，由 mobile/src/offline.rs 单测锁定。
 */
class OfflineSyncTest {

    /** 合法快照样本（enabled=true、quotes 授权、两条目） */
    private fun snapshot(
        enabled: Boolean = true,
        fingerprint: ULong = 42UL,
    ): OfflineSnapshotPayload = OfflineSnapshotPayload(
        version = "0.1.0",
        enabled = enabled,
        scopes = listOf("quotes"),
        capturedAt = "2026-10-01T00:00:00Z",
        entries = listOf(
            OfflineEntryPayload("quote:600519", """{"symbol":"600519","price":90.0}""", 1UL),
            OfflineEntryPayload("quote:000001", """{"symbol":"000001","price":10.0}""", 2UL),
        ),
        fingerprint = fingerprint,
    )

    /** kv 语义：后写覆盖、删除幂等、键列表如实 */
    @Test
    fun key_value_store_overwrites_and_deletes_idempotently() {
        val kv = InMemoryKeyValueStore()
        kv.put("k", "v1")
        assertEquals("v1", kv.get("k"))
        kv.put("k", "v2")
        assertEquals("后写覆盖", "v2", kv.get("k"))
        assertEquals(listOf("k"), kv.keys())
        kv.delete("k")
        assertNull(kv.get("k"))
        kv.delete("k")
        assertNull("删除幂等", kv.get("k"))
    }

    /** 备份往返：落盘→读回一致；clear 后为空 */
    @Test
    fun offline_store_roundtrips_snapshot() {
        val store = OfflineStore(InMemoryKeyValueStore())
        val snap = snapshot()
        store.saveSnapshot(snap)
        assertEquals("落盘→读回往返一致", snap, store.loadSnapshot())
        store.clear()
        assertNull(store.loadSnapshot())
    }

    /** 红线第二道防线：enabled=false 的载荷拒收（Rust 侧根本不产出，此处兜底） */
    @Test
    fun offline_store_rejects_unauthorized_payload() {
        val store = OfflineStore(InMemoryKeyValueStore())
        try {
            store.saveSnapshot(snapshot(enabled = false))
            throw AssertionError("未授权载荷应拒收")
        } catch (expected: IllegalStateException) {
            assertTrue(
                "文案须点名红线: ${expected.message}",
                expected.message!!.contains("显式开启"),
            )
        }
        assertNull("被拒载荷不得落盘", store.loadSnapshot())
    }

    /** fake gateway：记录入参、按脚本返回 */
    private class FakeGateway(
        private val snap: OfflineSnapshotPayload,
        private val delta: SyncDeltaPayload,
    ) : OfflineGateway {
        val receivedSince = mutableListOf<ULong>()

        override suspend fun offlineSnapshot(): OfflineSnapshotPayload {
            receivedSince += ULong.MAX_VALUE // 哨兵：标记快照被取过
            return snap
        }

        override suspend fun offlineSyncDelta(sinceFingerprint: ULong): SyncDeltaPayload {
            receivedSince += sinceFingerprint
            return delta
        }
    }

    /** 备份→落盘→同步基线：planSync 用已落盘快照的指纹作 since（编排闭环） */
    @Test
    fun manager_backup_persists_and_plan_sync_uses_stored_baseline() = runBlocking {
        val snap = snapshot(fingerprint = 42UL)
        val gateway = FakeGateway(
            snap = snap,
            delta = SyncDeltaPayload(false, 42UL, 42UL, "unchanged"),
        )
        val store = OfflineStore(InMemoryKeyValueStore())
        val manager = OfflineSyncManager(gateway, store)

        assertNull("备份前无落盘基线", store.loadSnapshot())
        val backed = manager.backupNow()
        assertEquals("备份返回已落盘快照", snap, backed)
        assertEquals("快照已落盘", snap, store.loadSnapshot())

        val delta = manager.planSync()
        assertEquals(false, delta.needed)
        assertEquals("同步基线 = 落盘快照指纹", 42UL, gateway.receivedSince.last())
        assertEquals("备份确实经 gateway 取快照", ULong.MAX_VALUE, gateway.receivedSince.first())
    }

    /** 无落盘基线时 planSync 以 0 起步（增量语义起点，沿 L337 指纹口径） */
    @Test
    fun plan_sync_without_baseline_starts_from_zero() = runBlocking {
        val gateway = FakeGateway(
            snap = snapshot(),
            delta = SyncDeltaPayload(true, 0UL, 42UL, null),
        )
        val manager = OfflineSyncManager(gateway, OfflineStore(InMemoryKeyValueStore()))
        val delta = manager.planSync()
        assertEquals("needed 才允许发起对齐（骨架期仍不执行网络）", true, delta.needed)
        assertEquals("无基线从 0 起步", 0UL, gateway.receivedSince.single())
    }

    /** 红线③明示面：授权范围翻成用户可读文案 */
    @Test
    fun data_scope_summary_names_authorized_data() {
        assertTrue(
            "文案: ${dataScopeSummary(listOf("quotes"))}",
            dataScopeSummary(listOf("quotes")).contains("行情快照"),
        )
        assertTrue(
            "空范围文案: ${dataScopeSummary(emptyList())}",
            dataScopeSummary(emptyList()).contains("未授权"),
        )
        assertTrue(
            "未知范围兜底不发明语义",
            dataScopeSummary(listOf("contacts")).contains("未知范围"),
        )
    }

    /** 术语分离的字段面：备份载荷（captured_at/version/entries）与同步载荷
     *  （since_fingerprint/needed）字段名不交叉（docs/mobile-offline.md §1） */
    @Test
    fun payload_fields_keep_backup_and_sync_terms_distinct() {
        val snapJson = kotlinx.serialization.json.Json.encodeToString(
            OfflineSnapshotPayload.serializer(),
            snapshot(),
        )
        assertTrue("备份载荷用 captured_at", snapJson.contains("captured_at"))
        assertTrue("备份载荷含条目", snapJson.contains("entries"))
        assertTrue("备份载荷不得混入同步术语", !snapJson.contains("since_fingerprint"))

        val deltaJson = kotlinx.serialization.json.Json.encodeToString(
            SyncDeltaPayload.serializer(),
            SyncDeltaPayload(false, 0UL, 42UL, "sync_disabled"),
        )
        assertTrue("同步载荷用 since_fingerprint", deltaJson.contains("since_fingerprint"))
        assertTrue("同步载荷不得混入备份术语", !deltaJson.contains("captured_at"))
    }
}
