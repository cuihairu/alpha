package com.alpha.finance.mobile

import android.content.Context
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json

/**
 * 离线数据壳层执行面（L390，docs/mobile-offline.md §5）：kv 落盘/恢复 + 备份
 * 与同步编排。**决策全在 Rust 侧**（授权开关默认关闭/开启须范围/未授权不出
 * 同步工作项——FFI 层已强制），本文件只做存储执行与第二道防线。
 *
 * **术语红线**（文档 §1，不得混用）：
 * - **备份（backup）** = 本地快照落盘，**不出设备**——`OfflineStore` /
 *   `OfflineSyncManager.backupNow()` / `Snapshot` 载荷；
 * - **同步（sync）** = 与远端增量对齐，**出设备**——`OfflineSyncManager.planSync()`
 *   / `SyncDelta` 载荷；骨架期只出「是否需要对齐」的判断，不执行网络。
 */

/** kv 存储抽象：后写覆盖、删除幂等（语义对齐 alpha-core `platform::KeyValueStore`） */
interface KeyValueStore {
    /** 读键（不存在返回 null） */
    fun get(key: String): String?

    /** 写键（后写覆盖） */
    fun put(key: String, value: String)

    /** 删键（幂等） */
    fun delete(key: String)

    /** 现存键列表（诊断/测试用） */
    fun keys(): List<String>
}

/** 内存实现（JVM 单测；进程内生命周期，不落盘） */
class InMemoryKeyValueStore : KeyValueStore {
    private val map = LinkedHashMap<String, String>()

    override fun get(key: String): String? = map[key]
    override fun put(key: String, value: String) {
        map[key] = value
    }

    override fun delete(key: String) {
        map.remove(key)
    }

    override fun keys(): List<String> = map.keys.toList()
}

/**
 * SharedPreferences 实现（真机实现位；骨架不实例化——Context 依赖随设置页
 * TODO 一起接，见 docs/mobile-offline.md §8⑦）
 */
class SharedPreferencesKeyValueStore(context: Context) : KeyValueStore {
    private val prefs = context.getSharedPreferences("alpha_offline", Context.MODE_PRIVATE)

    override fun get(key: String): String? = prefs.getString(key, null)
    override fun put(key: String, value: String) {
        prefs.edit().putString(key, value).apply()
    }

    override fun delete(key: String) {
        prefs.edit().remove(key).apply()
    }

    override fun keys(): List<String> = prefs.all.keys.toList()
}

/**
 * 备份面存储：快照整体单键落盘（沿桌面 L116「kv 快照」口径——骨架期快照小，
 * 分键/条目级 diff 归后续，文档 §8⑥）。
 *
 * **红线第二道防线**：`enabled=false` 的载荷拒收——Rust 侧未开启根本不产出
 * 快照（FFI 已拦），此处兜底防壳层伪造/误传未授权载荷。
 */
class OfflineStore(private val kv: KeyValueStore) {
    private val json = Json { ignoreUnknownKeys = true }

    /** 快照落盘键（整快照单键，§8⑥） */
    companion object {
        const val KEY_SNAPSHOT = "alpha_offline_snapshot"
    }

    /** 快照落盘（备份面）；未授权载荷（enabled=false）拒收 */
    fun saveSnapshot(snapshot: OfflineSnapshotPayload) {
        check(snapshot.enabled) { "未授权的离线载荷不可落盘（红线①：须显式开启）" }
        kv.put(KEY_SNAPSHOT, json.encodeToString(snapshot))
    }

    /** 读回快照（备份面）；无快照返回 null */
    fun loadSnapshot(): OfflineSnapshotPayload? =
        kv.get(KEY_SNAPSHOT)?.let { json.decodeFromString<OfflineSnapshotPayload>(it) }

    /** 清除快照（用户撤权时的数据清除位） */
    fun clear() {
        kv.delete(KEY_SNAPSHOT)
    }
}

/**
 * 离线编排依赖面（沿 `RefreshGateway` 模式：真机由 [AlphaBridge] 实现，JVM 单测
 * 注入 fake——JVM 测试环境无 `.so` 不可构造 AlphaBridge）
 */
interface OfflineGateway {
    /** 取本地快照（备份面；未开启由 Rust 侧抛错） */
    suspend fun offlineSnapshot(): OfflineSnapshotPayload

    /** 同步增量判断（同步面；未授权 → needed=false） */
    suspend fun offlineSyncDelta(sinceFingerprint: ULong): SyncDeltaPayload
}

/**
 * 离线编排：备份（落盘）与同步（增量判断，不执行网络）。
 *
 * - [backupNow]：快照 → 落盘（备份面，不出设备）；
 * - [planSync]：以已落盘快照的指纹为基线出增量判断（同步面）；无落盘基线时
 *   以 0 起步。**不发起网络请求**——真实对齐执行归远端接入 TODO（文档 §8④）。
 *
 * 异常不捕获：`AlphaBridge.Error` 冒泡给 UI 按既有口径降级（L301 纪律）。
 */
class OfflineSyncManager(
    private val gateway: OfflineGateway,
    private val store: OfflineStore,
) {
    /** 备份：取快照并落盘，返回已落盘快照（供 UI 展示条目数/时刻） */
    suspend fun backupNow(): OfflineSnapshotPayload {
        val snapshot = gateway.offlineSnapshot()
        store.saveSnapshot(snapshot)
        return snapshot
    }

    /** 同步计划：只出「是否需要对齐」的判断（needed=false 不产生任何外发动作） */
    suspend fun planSync(): SyncDeltaPayload =
        gateway.offlineSyncDelta(store.loadSnapshot()?.fingerprint ?: 0UL)
}

/**
 * 红线③明示面：把授权范围翻成用户可读文案（设置页展示位；scope 为封闭枚举，
 * 未知值不会从 Rust 侧过来，此处兜底「未知范围」不发明语义）
 */
fun dataScopeSummary(scopes: List<String>): String {
    if (scopes.isEmpty()) return "未授权任何数据（离线功能关闭）"
    return scopes.joinToString("、") { scope ->
        when (scope) {
            "quotes" -> "行情快照（代码、价格、成交量、买卖一档）"
            else -> "未知范围: $scope"
        }
    }
}
