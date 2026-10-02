package com.alpha.finance.mobile

import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * 账户与数据同步客户端面单测（L476 Android 侧）。
 *
 * 语义与 `packages/core/src/account.rs` / `web/app/src/lib/account.ts`
 * 三实现对齐——每条判定在本文件有一处锁：墓碑传播、baseRev 不自增、
 * 并列取服务端收敛、缺键非删除、水位回退忽略、wire snake_case 对齐。
 */
class AccountSyncTest {

    private fun live(key: String, rev: Long, payload: JsonElement, at: Long = 100L) =
        liveRecord(key, rev, at, payload)

    private fun obj(vararg pairs: Pair<String, Long>) = JsonObjectBuilder().apply {
        pairs.forEach { (k, v) -> put(k, JsonPrimitive(v)) }
    }.build()

    /** 最小 JsonObject 构造（避免测试里堆 kotlinx DSL） */
    private class JsonObjectBuilder {
        private val map = linkedMapOf<String, JsonElement>()
        fun put(key: String, value: JsonElement) { map[key] = value }
        fun build(): JsonElement = JsonObject(map)
    }

    // ---- 校验 ----

    @Test
    fun `记录校验覆盖键形状 墓碑载荷与 rev`() {
        assertNull(validateRecord(live("workspace:a", 1, obj("n" to 1))))
        assertNull(validateRecord(tombstone("workspace:a", 2, 100)))
        assertNotNull(validateRecord(live("nope", 1, JsonNull)))
        assertNotNull(validateRecord(live("Workspace:a", 1, JsonNull)))
        assertNotNull(validateRecord(live("workspace:a", 0, JsonNull)))
        assertNotNull(
            validateRecord(tombstone("workspace:a", 2, 100).copy(payload = obj("x" to 1))),
        )
    }

    @Test
    fun `同内容判定忽略 rev`() {
        assertTrue(sameContent(live("k", 1, obj("n" to 1)), live("k", 5, obj("n" to 1))))
        assertTrue(!sameContent(live("k", 1, obj("n" to 1)), live("k", 5, obj("n" to 2))))
        assertTrue(!sameContent(live("k", 1, obj("n" to 1)), tombstone("k", 5, 1)))
    }

    @Test
    fun `超大载荷在客户端就拦（与服务端 64 KiB 上限一致）`() {
        val big = live("workspace:a", 1, JsonPrimitive("x".repeat(MAX_PAYLOAD_BYTES)))
        assertTrue(validateRecord(big)!!.contains("载荷超长"))
    }

    // ---- 三方合并 ----

    @Test
    fun `单侧改动是普通增量不是冲突`() {
        val up = planSync(mapOf("workspace:a" to live("workspace:a", 1, obj("n" to 1))), emptyMap(), emptyMap())
        assertEquals(1, up.push.size)
        assertTrue(up.conflicts.isEmpty())

        val down = planSync(
            emptyMap(),
            mapOf("workspace:a" to live("workspace:a", 2, obj("n" to 2))),
            mapOf("workspace:a" to 1L),
        )
        assertEquals(1, down.pull.size)
        assertTrue(down.conflicts.isEmpty())
    }

    @Test
    fun `并发同改同值取 rev 高者推进基线且不记冲突`() {
        val plan = planSync(
            mapOf("workspace:a" to live("workspace:a", 2, obj("n" to 7))),
            mapOf("workspace:a" to live("workspace:a", 3, obj("n" to 7))),
            mapOf("workspace:a" to 1L),
        )
        assertTrue(plan.push.isEmpty())
        assertEquals(3L, plan.pull.single().rev)
        assertTrue(plan.conflicts.isEmpty())
    }

    @Test
    fun `newestWins 按时间戳 且并列取服务端（两端收敛前提）`() {
        val local = mapOf("workspace:a" to live("workspace:a", 2, JsonPrimitive("L"), at = 300))
        val remote = mapOf("workspace:a" to live("workspace:a", 2, JsonPrimitive("R"), at = 200))
        val newer = planSync(local, remote, mapOf("workspace:a" to 1L))
        assertEquals(1, newer.push.size)
        assertEquals(ConflictWinner.Local, newer.conflicts.single().winner)

        val tie = planSync(
            local,
            mapOf("workspace:a" to live("workspace:a", 2, JsonPrimitive("R"), at = 300)),
            mapOf("workspace:a" to 1L),
        )
        assertEquals(1, tie.pull.size)
        assertEquals(ConflictWinner.Remote, tie.conflicts.single().winner)
    }

    @Test
    fun `显式策略覆盖时序`() {
        val local = mapOf("workspace:a" to live("workspace:a", 2, JsonPrimitive("L"), at = 500))
        val remote = mapOf("workspace:a" to live("workspace:a", 2, JsonPrimitive("R"), at = 100))
        assertEquals(
            1,
            planSync(local, remote, mapOf("workspace:a" to 1L), ConflictPolicy.RemoteWins).pull.size,
        )
        assertEquals(
            1,
            planSync(local, remote, mapOf("workspace:a" to 1L), ConflictPolicy.LocalWins).push.size,
        )
    }

    @Test
    fun `墓碑传播且缺键只是无意见`() {
        val localTomb = mapOf("workspace:a" to tombstone("workspace:a", 4, 500))
        val remoteLive = mapOf("workspace:a" to live("workspace:a", 3, obj("n" to 1), at = 100))
        val plan = planSync(localTomb, remoteLive, mapOf("workspace:a" to 3L))
        assertTrue(plan.push.single().deleted)
        assertTrue(plan.conflicts.isEmpty())

        // 服务端删了、本地无该键 → 无传输（下一轮按 cursor 补墓碑）
        assertTrue(planSync(emptyMap(), emptyMap(), mapOf("workspace:gone" to 2L)).push.isEmpty())
        assertTrue(planSync(emptyMap(), emptyMap(), mapOf("workspace:gone" to 2L)).pull.isEmpty())
    }

    @Test
    fun `双端并发收敛 A 推送后 B 拉到权威副本视图一致`() {
        val aView = mapOf("workspace:a" to live("workspace:a", 2, JsonPrimitive("A"), at = 400))
        val bView = mapOf("workspace:a" to live("workspace:a", 2, JsonPrimitive("B"), at = 100))
        val base = mapOf("workspace:a" to 1L)

        val aPlan = planSync(aView, bView, base)
        assertEquals(1, aPlan.push.size)

        val server = mapOf("workspace:a" to live("workspace:a", 3, JsonPrimitive("A"), at = 400))
        val aAfter = applyToView(aView, server.values.toList())
        val bPlan = planSync(bView, server, base)
        val bAfter = applyToView(bView, bPlan.pull)
        assertEquals(aAfter, bAfter)
        // B 的本地改动被覆盖 = 真冲突留痕
        assertEquals(1, bPlan.conflicts.size)
        assertEquals(ConflictWinner.Remote, bPlan.conflicts.single().winner)
    }

    // ---- 发件箱与下行应用 ----

    @Test
    fun `发件箱同键后写覆盖且条数受上限约束`() {
        var state = SyncState()
        for (i in 0 until MAX_PUSHES_PER_REQUEST + 5) {
            state = noteLocalChange(state, "workspace:%03d".format(i), obj("i" to i.toLong()), 100L + i)
        }
        assertEquals(MAX_PUSHES_PER_REQUEST, pendingPushes(state).size)
        state = noteLocalChange(state, "workspace:000", obj("i" to 999), 999)
        assertEquals(MAX_PUSHES_PER_REQUEST + 5, state.outbox.size)
        assertEquals(obj("i" to 999), state.outbox.getValue("workspace:000").payload)
    }

    @Test
    fun `请求携带基线 rev 快照（客户端不自增 rev）`() {
        var state = SyncState(base = mapOf("workspace:a" to 4))
        state = noteLocalChange(state, "workspace:a", obj("n" to 1), 10)
        state = noteLocalDelete(state, "workspace:b", 11)
        val req = buildRequest(state)
        assertEquals(4L, req.base["workspace:a"])
        val a = req.pushes.first { it.key == "workspace:a" }
        assertEquals(4L, a.baseRev)
        assertFalse(a.deleted)
        val b = req.pushes.first { it.key == "workspace:b" }
        assertTrue(b.deleted)
        assertEquals(JsonNull, b.payload)
    }

    @Test
    fun `应用响应出队推基线且墓碑落地即从视图移除`() {
        var state = SyncState()
        state = noteLocalChange(state, "workspace:a", obj("n" to 1), 10)
        state = noteLocalChange(state, "workspace:b", obj("n" to 2), 10)
        val view = mapOf("workspace:old" to live("workspace:old", 8, obj("n" to 0)))

        val response = SyncResponse(
            cursor = 42,
            accepted = listOf(live("workspace:a", 1, obj("n" to 1), at = 20)),
            rejected = listOf(
                SyncRejection(
                    "workspace:b",
                    RejectionReason.Conflict,
                    live("workspace:b", 9, JsonPrimitive("server"), at = 5),
                ),
            ),
            changes = listOf(
                live("workspace:z", 7, obj("n" to 3), at = 30),
                tombstone("workspace:old", 8, 31),
            ),
        )
        val (next, outcome) = applyResponse(state, response)
        assertEquals(42L, next.cursor)
        assertEquals(1L, next.base["workspace:a"])
        assertEquals(listOf("workspace:a"), outcome.acked)
        assertEquals(3, outcome.applied.size)
        assertEquals(9L, outcome.conflicts.single().serverRecord!!.rev)
        // 冲突项留队待裁决
        assertNotNull(next.outbox["workspace:b"])

        val after = applyToView(view, outcome.applied)
        assertFalse(after.containsKey("workspace:old"))
        assertTrue(after.containsKey("workspace:z"))
    }

    @Test
    fun `水位回退被忽略（服务端重启重发旧增量不得被当新数据）`() {
        val state = SyncState(cursor = 100)
        val (next, outcome) = applyResponse(
            state,
            SyncResponse(cursor = 5, changes = listOf(live("workspace:a", 1, JsonPrimitive(true)))),
        )
        assertTrue(outcome.applied.isEmpty())
        assertEquals(100L, next.cursor)
    }

    // ---- wire 与持久化 ----

    @Test
    fun `wire 编解码对齐服务端 snake_case 字段`() {
        val request = SyncRequest(
            cursor = 3,
            base = mapOf("workspace:a" to 2),
            pushes = listOf(SyncPush("workspace:a", 2, false, obj("name" to 1), 100)),
        )
        val text = SyncCodec.encodeRequest(request)
        assertTrue("请求体用 base_rev: $text", text.contains("\"base_rev\":2"))
        assertTrue("请求体用 updated_at_ms: $text", text.contains("\"updated_at_ms\":100"))

        val responseJson = """
            {"cursor":7,
             "accepted":[{"key":"workspace:a","rev":2,"updated_at_ms":5,"deleted":false,"payload":{"n":1}}],
             "rejected":[{"key":"workspace:b","reason":"conflict","server":{"key":"workspace:b","rev":9,"updated_at_ms":6,"deleted":false,"payload":null}}],
             "changes":[{"key":"workspace:z","rev":3,"updated_at_ms":6,"deleted":true,"payload":null}]}
        """.trimIndent()
        val decoded = SyncCodec.decodeResponse(responseJson)
        assertEquals(7L, decoded.cursor)
        assertEquals("workspace:a", decoded.accepted.single().key)
        assertEquals(RejectionReason.Conflict, decoded.rejected.single().reason)
        assertEquals(9L, decoded.rejected.single().serverRecord!!.rev)
        assertTrue(decoded.changes.single().deleted)
    }

    @Test
    fun `同步状态持久化往返且损坏 fail-safe 回初态`() {
        val store = InMemoryKeyValueStore()
        val stateStore = SyncStateStore(store)
        var state = SyncState(base = mapOf("workspace:a" to 3), cursor = 12)
        state = noteLocalChange(state, "workspace:a", obj("n" to 1), 20)
        stateStore.save(state)

        val loaded = stateStore.load()
        assertEquals(12L, loaded.cursor)
        assertEquals(3L, loaded.base["workspace:a"])
        assertEquals(obj("n" to 1), loaded.outbox.getValue("workspace:a").payload)

        // 损坏载荷不得卡死同步面：回初态即可（丢一次待推项好过整面挂掉）
        store.put(SyncStateStore.KEY, "{broken")
        assertEquals(SyncState(), stateStore.load())
    }

    }