package com.alpha.finance.mobile

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull

/**
 * 账户与跨端数据同步的客户端面（L476，Android 侧）。
 *
 * **协议对齐** `packages/core/src/account.rs`（服务端权威面
 * `services/api-gateway/src/account.rs`，端点 `/api/v1/account` 前缀），
 * 语义与 `web/app/src/lib/account.ts` 同源三实现——同一份语义写三处
 * 是本项最大风险，所以每条判定都有对应单测锁住：
 * - 墓碑位 `deleted`：删除必须留痕，否则删除传不到其他端；
 * - 客户端只提交 `baseRev`（上次见到的服务端 rev），**不自增 rev**——
 *   否则两台设备会写出同一个 rev，服务端乐观并发比对直接失效；
 * - 三方合并基线 = 上次同步的 rev 快照；缺键 = 「无意见」而非删除；
 * - `newestWins` 时间戳并列取服务端（两端同规则才能算出同一裁决）。
 *
 * 本文件是**纯逻辑面**（无网络、无时钟读取、无 Context）：网络执行在
 * 调用方（Android 无 INTERNET 权限，见 L301 红线——同步开关打开时随
 * 设置页一并申请），持久化走 [KeyValueStore]，时间戳全部显式入参，
 * JVM 单测可完整覆盖。
 */

/** 单条载荷上限（字节；与服务端 64 KiB 一致） */
const val MAX_PAYLOAD_BYTES = 64 * 1024

/** 单次请求推送条数上限（超出留队下一轮——弱网下同步包不能无限大） */
const val MAX_PUSHES_PER_REQUEST = 100

/** 认证关闭时的本机账户 id（服务端同一常量） */
const val LOCAL_ACCOUNT_ID = "local"

/** 键形状：命名空间小写起 ≤16 字符，本地 id ≤48 字符，键总长 ≤64 */
private val KEY_PATTERN = Regex("^[a-z][a-z0-9_-]{0,15}:[A-Za-z0-9._-]{1,48}$")

/** 同步记录（`key` 形如 `workspace:9f2c`） */
@Serializable
data class SyncRecord(
    val key: String,
    /** 服务端权威版本（客户端只读回，不自增） */
    val rev: Long,
    /** 记录修改时间（毫秒 epoch；`newestWins` 裁决的比较量） */
    val updatedAtMs: Long,
    /** 墓碑位：删除必须留痕 */
    val deleted: Boolean = false,
    val payload: JsonElement = JsonNull,
)

/** 上行推送项 */
@Serializable
data class SyncPush(
    val key: String,
    /** 客户端最后见到的服务端 rev（0 = 新建） */
    val baseRev: Long = 0,
    val deleted: Boolean = false,
    val payload: JsonElement = JsonNull,
    val updatedAtMs: Long = 0,
)

/** 拒绝原因（服务端判别式，snake_case） */
enum class RejectionReason(val wire: String) {
    Conflict("conflict"),
    InvalidKey("invalid_key"),
    PayloadTooLarge("payload_too_large"),
    TombstonePayloadNotNull("tombstone_payload_not_null"),
}

/** 拒绝项（`serverRecord` 为服务端权威副本，冲突时在位） */
data class SyncRejection(
    val key: String,
    val reason: RejectionReason,
    val serverRecord: SyncRecord? = null,
)

/** 上行请求（字段名 wire 化在 [SyncCodec]） */
data class SyncRequest(
    val cursor: Long = 0,
    val base: Map<String, Long> = emptyMap(),
    val pushes: List<SyncPush> = emptyList(),
)

/** 下行响应 */
data class SyncResponse(
    val cursor: Long = 0,
    val accepted: List<SyncRecord> = emptyList(),
    val rejected: List<SyncRejection> = emptyList(),
    val changes: List<SyncRecord> = emptyList(),
)

/** 冲突裁决策略 */
enum class ConflictPolicy { NewestWins, LocalWins, RemoteWins }

/** 冲突裁决胜方 */
enum class ConflictWinner { Local, Remote }

/** 一次冲突裁决的留痕（可展示「这条被覆盖了」） */
data class SyncConflict(
    val key: String,
    val localRev: Long,
    val remoteRev: Long,
    val winner: ConflictWinner,
)

/** 三方合并计划 */
data class SyncPlan(
    val push: List<SyncRecord> = emptyList(),
    val pull: List<SyncRecord> = emptyList(),
    val conflicts: List<SyncConflict> = emptyList(),
)

/** 一次下行应用的结果 */
data class SyncOutcome(
    val applied: List<SyncRecord> = emptyList(),
    val acked: List<String> = emptyList(),
    val conflicts: List<SyncRejection> = emptyList(),
)

/** 记录校验（与服务端同口径；返回 null = 合法） */
fun validateRecord(record: SyncRecord): String? {
    if (!KEY_PATTERN.matches(record.key)) return "同步键非法: ${record.key}"
    if (record.key.length > 64) return "同步键超长: ${record.key}"
    if (record.rev < 1) return "rev 非法: ${record.rev}"
    if (record.deleted) {
        return if (record.payload is JsonNull) null else "墓碑记录必须携带空载荷"
    }
    val bytes = record.payload.toString().toByteArray(Charsets.UTF_8).size
    return if (bytes > MAX_PAYLOAD_BYTES) "载荷超长（$bytes > $MAX_PAYLOAD_BYTES 字节）" else null
}

/** 内容等价（忽略 rev）：载荷 + 墓碑位一致即两端已收敛到同一份数据 */
fun sameContent(a: SyncRecord, b: SyncRecord): Boolean =
    a.deleted == b.deleted && a.payload == b.payload

/** 构造存活记录 */
fun liveRecord(key: String, rev: Long, updatedAtMs: Long, payload: JsonElement): SyncRecord =
    SyncRecord(key = key, rev = rev, updatedAtMs = updatedAtMs, deleted = false, payload = payload)

/** 构造墓碑（载荷恒为 JsonNull） */
fun tombstone(key: String, rev: Long, updatedAtMs: Long): SyncRecord =
    SyncRecord(key = key, rev = rev, updatedAtMs = updatedAtMs, deleted = true, payload = JsonNull)

/**
 * 三方合并：本地 vs 服务端，基线为上次同步后各键的 rev 快照。
 *
 * 与 Rust/TS 两侧逐条对齐：单侧改动 = 普通增量不算冲突；双侧同改同值按
 * rev 高者下发推进基线；双侧异值才裁冲突，`NewestWins` 并列取服务端。
 */
fun planSync(
    local: Map<String, SyncRecord>,
    remote: Map<String, SyncRecord>,
    base: Map<String, Long>,
    policy: ConflictPolicy = ConflictPolicy.NewestWins,
): SyncPlan {
    val push = mutableListOf<SyncRecord>()
    val pull = mutableListOf<SyncRecord>()
    val conflicts = mutableListOf<SyncConflict>()
    val keys = (local.keys + remote.keys + base.keys).toSortedSet()

    for (key in keys) {
        val baseRev = base[key] ?: 0L
        val l = local[key]
        val r = remote[key]
        val localChanged = l != null && l.rev > baseRev
        val remoteChanged = r != null && r.rev > baseRev

        if (l != null && r != null) {
            when {
                localChanged && !remoteChanged -> push += l
                !localChanged && remoteChanged -> pull += r
                sameContent(l, r) -> pull += if (l.rev >= r.rev) l else r
                else -> {
                    val localWins = when (policy) {
                        ConflictPolicy.LocalWins -> true
                        ConflictPolicy.RemoteWins -> false
                        // 并列取服务端：两端同规则才能收敛到同一份数据
                        ConflictPolicy.NewestWins -> l.updatedAtMs > r.updatedAtMs
                    }
                    conflicts += SyncConflict(
                        key = key,
                        localRev = l.rev,
                        remoteRev = r.rev,
                        winner = if (localWins) ConflictWinner.Local else ConflictWinner.Remote,
                    )
                    if (localWins) push += l else pull += r
                }
            }
        } else if (l != null) {
            if (localChanged) push += l
        } else if (r != null) {
            if (remoteChanged) pull += r
        }
        // 双端皆无但基线有过：服务端删除后本地尚未拉增量，无需传输
    }
    return SyncPlan(push, pull, conflicts)
}

/** 客户端同步状态：发件箱 + 水位 + 基线（纯数据类，归约走伴生函数） */
data class SyncState(
    val cursor: Long = 0,
    val base: Map<String, Long> = emptyMap(),
    val outbox: Map<String, SyncPush> = emptyMap(),
)

/** 本地改动入队（同键后写覆盖——连续编辑不堆条目） */
fun noteLocalChange(
    state: SyncState,
    key: String,
    payload: JsonElement,
    nowMs: Long,
): SyncState = state.copy(
    outbox = state.outbox + (key to SyncPush(
        key = key,
        baseRev = state.base[key] ?: 0,
        deleted = false,
        payload = payload,
        updatedAtMs = nowMs,
    )),
)

/** 本地删除入队（墓碑：删除必须传播） */
fun noteLocalDelete(state: SyncState, key: String, nowMs: Long): SyncState = state.copy(
    outbox = state.outbox + (key to SyncPush(
        key = key,
        baseRev = state.base[key] ?: 0,
        deleted = true,
        payload = JsonNull,
        updatedAtMs = nowMs,
    )),
)

/** 待推送项（按键序稳定，条数受 [MAX_PUSHES_PER_REQUEST] 限制） */
fun pendingPushes(state: SyncState): List<SyncPush> =
    state.outbox.keys.sorted().take(MAX_PUSHES_PER_REQUEST).map { state.outbox.getValue(it) }

/** 构造上行请求 */
fun buildRequest(state: SyncState): SyncRequest =
    SyncRequest(cursor = state.cursor, base = state.base, pushes = pendingPushes(state))

/**
 * 应用下行响应：接受项出队并推进基线、增量落基线、拒绝项转为冲突。
 *
 * 水位单调前进——**回退水位视为协议破坏直接忽略**（服务端重启可能从低
 * 水位重发，旧增量不得被当新数据应用）。
 */
fun applyResponse(state: SyncState, response: SyncResponse): Pair<SyncState, SyncOutcome> {
    val base = state.base.toMutableMap()
    val outbox = state.outbox.toMutableMap()
    val applied = mutableListOf<SyncRecord>()
    val acked = mutableListOf<String>()

    for (record in response.accepted) {
        base[record.key] = record.rev
        outbox.remove(record.key)
        acked += record.key
        applied += record
    }
    val regressed = response.cursor < state.cursor
    for (record in response.changes) {
        base[record.key] = record.rev
        if (!regressed) applied += record
    }
    val next = SyncState(
        cursor = if (regressed) state.cursor else response.cursor,
        base = base,
        outbox = outbox,
    )
    return next to SyncOutcome(applied, acked, response.rejected)
}

/** 本地视图应用（墓碑 → 从视图移除；其余 → 写入） */
fun applyToView(
    view: Map<String, SyncRecord>,
    applied: List<SyncRecord>,
): Map<String, SyncRecord> {
    val next = view.toMutableMap()
    for (record in applied) {
        if (record.deleted) next.remove(record.key) else next[record.key] = record
    }
    return next
}

/**
 * wire 编解码（服务端字段 snake_case：Android 侧用 `@SerialName` 对齐，
 * 不做形状转换——转换层就是未来的协议漂移点）。
 */
object SyncCodec {
    private val json = Json { ignoreUnknownKeys = true; encodeDefaults = true }

    @Serializable
    private data class WireRecord(
        val key: String,
        val rev: Long,
        @SerialName("updated_at_ms") val updatedAtMs: Long,
        val deleted: Boolean = false,
        val payload: JsonElement = JsonNull,
    )

    @Serializable
    private data class WirePush(
        val key: String,
        @SerialName("base_rev") val baseRev: Long = 0,
        val deleted: Boolean = false,
        val payload: JsonElement = JsonNull,
        @SerialName("updated_at_ms") val updatedAtMs: Long = 0,
    )

    @Serializable
    private data class WireRejection(
        val key: String,
        val reason: String,
        val server: WireRecord? = null,
    )

    @Serializable
    private data class WireRequest(
        val cursor: Long = 0,
        val base: Map<String, Long> = emptyMap(),
        val pushes: List<WirePush> = emptyList(),
    )

    @Serializable
    private data class WireResponse(
        val cursor: Long = 0,
        val accepted: List<WireRecord> = emptyList(),
        val rejected: List<WireRejection> = emptyList(),
        val changes: List<WireRecord> = emptyList(),
    )

    private fun WireRecord.toRecord() = SyncRecord(key, rev, updatedAtMs, deleted, payload)

    private fun WirePush.toPush() = SyncPush(key, baseRev, deleted, payload, updatedAtMs)

    /** 上行请求编码（POST body） */
    fun encodeRequest(request: SyncRequest): String = json.encodeToString(
        WireRequest(
            cursor = request.cursor,
            base = request.base,
            pushes = request.pushes.map {
                WirePush(it.key, it.baseRev, it.deleted, it.payload, it.updatedAtMs)
            },
        ),
    )

    /** 下行响应解码（非法形状抛异常由调用方兜——网络面本就不可信） */
    fun decodeResponse(raw: String): SyncResponse {
        val wire = json.decodeFromString<WireResponse>(raw)
        return SyncResponse(
            cursor = wire.cursor,
            accepted = wire.accepted.map { it.toRecord() },
            rejected = wire.rejected.map {
                SyncRejection(
                    key = it.key,
                    reason = RejectionReason.entries.firstOrNull { r -> r.wire == it.reason }
                        ?: RejectionReason.Conflict,
                    serverRecord = it.server?.toRecord(),
                )
            },
            changes = wire.changes.map { it.toRecord() },
        )
    }
}

/**
 * 同步状态持久化（单键 JSON；损坏 fail-safe 回初态——本地发件箱损坏时
 * 宁可丢一次待推项，也不能让整个同步面卡死）。
 */
class SyncStateStore(private val store: KeyValueStore) {
    companion object {
        const val KEY = "alpha_account_sync_state"
    }

    /** @Serializable 供 [store] 落盘的内部形状 */
    @Serializable
    private data class Persisted(
        val cursor: Long = 0,
        val base: Map<String, Long> = emptyMap(),
        val outbox: List<SyncPush> = emptyList(),
    )

    fun load(): SyncState {
        val raw = store.get(KEY) ?: return SyncState()
        return try {
            val persisted = Json.decodeFromString<Persisted>(raw)
            SyncState(
                cursor = persisted.cursor,
                base = persisted.base,
                outbox = persisted.outbox.associateBy { it.key },
            )
        } catch (e: Exception) {
            SyncState()
        }
    }

    fun save(state: SyncState) {
        val persisted = Persisted(state.cursor, state.base, state.outbox.values.sortedBy { it.key })
        store.put(KEY, Json.encodeToString(persisted))
    }
}