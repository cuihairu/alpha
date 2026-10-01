package com.alpha.finance.mobile

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * FFI 载荷模型（L301）：alpha-mobile 的 FFI 面（quoteJson/analyzeJson/statusJson）
 * 返回 JSON 字符串，字段契约 = alpha-core 模型的 serde 命名（snake_case）——
 * 与 web/desktop 走同一份 serde 输出，Kotlin 侧用 @SerialName 对齐，不发明
 * 第二套命名。解析策略 ignoreUnknownKeys（Rust 侧加字段不崩壳，减字段靠
 * Rust 侧单测锁定）。
 */

/** 快照行情 ↔ alpha-core `MarketData` */
@Serializable
data class QuotePayload(
    val symbol: String,
    val timestamp: String,
    val price: Double,
    val volume: Long,
    val bid: Double? = null,
    val ask: Double? = null,
    val open: Double? = null,
    val high: Double? = null,
    val low: Double? = null,
)

/** 技术指标 ↔ alpha-core `IndicatorResult`（timestamps/values/signals 三等长序列） */
@Serializable
data class IndicatorPayload(
    val name: String,
    val timestamps: List<String>,
    val values: List<Double>,
    val signals: List<String>,
)

/** 风险指标 ↔ alpha-core `RiskMetrics` */
@Serializable
data class RiskMetricsPayload(
    val volatility: Double,
    @SerialName("sharpe_ratio") val sharpeRatio: Double? = null,
    @SerialName("max_drawdown") val maxDrawdown: Double,
    val beta: Double? = null,
)

/** 分析结果 ↔ alpha-core `AnalysisResult`（recommendation/signals 为 serde 字符串） */
@Serializable
data class AnalysisPayload(
    val symbol: String,
    @SerialName("analyzed_at") val analyzedAt: String,
    val indicators: List<IndicatorPayload>,
    @SerialName("risk_metrics") val riskMetrics: RiskMetricsPayload,
    val recommendation: String,
    val confidence: Double,
)

/** 核心库状态 ↔ `MobileCore.status_json`（版本 + 观察列表 + 后端地址配置槽） */
@Serializable
data class StatusPayload(
    val version: String,
    val symbols: List<String>,
    @SerialName("api_url") val apiUrl: String,
)

// ── L337 推送/同步载荷（docs/mobile-push-sync.md §4；u64/usize → ULong）──

/** 价格告警规则 ↔ alpha-mobile `AlertRule`（整体替换式设置） */
@Serializable
data class AlertRulePayload(
    val symbol: String,
    @SerialName("target_price") val targetPrice: Double,
    val above: Boolean,
)

/** 通知载荷 ↔ alpha-mobile `NotificationSpec`（id 为稳定去重键，kind 目前仅 PriceAlert） */
@Serializable
data class NotificationSpecPayload(
    val id: String,
    val kind: String,
    val symbol: String,
    val title: String,
    val body: String,
    @SerialName("created_at") val createdAt: String,
)

/** 告警评估报告 ↔ `check_alerts_json`（fired=本轮新触发，pending=队列待取条数） */
@Serializable
data class AlertsReportPayload(
    val fired: List<NotificationSpecPayload>,
    val pending: ULong,
)

/** 待送达队列 ↔ `take_pending_json`（取走即空） */
@Serializable
data class TakenPayload(
    val taken: List<NotificationSpecPayload>,
)

/** 同步状态 ↔ `sync_status_json`/`mark_synced_json`（lastSync 为 RFC3339，从未同步为 null） */
@Serializable
data class SyncStatusPayload(
    @SerialName("last_sync") val lastSync: String? = null,
    val fingerprint: ULong,
    @SerialName("interval_secs") val intervalSecs: ULong,
    val due: Boolean,
)

/** 同步计划 ↔ `sync_plan_json`（due=false 时只带 reason 不带工作项） */
@Serializable
data class SyncPlanPayload(
    val due: Boolean,
    val trigger: String,
    val symbols: List<String>,
    @SerialName("since_fingerprint") val sinceFingerprint: ULong,
    @SerialName("interval_secs") val intervalSecs: ULong,
    val reason: String? = null,
)

// ── L390 离线数据载荷（docs/mobile-offline.md §4；术语红线：备份=Snapshot
//    落盘不出设备，同步=SyncDelta 与远端对齐——命名与文案不混用）──

/** 离线授权配置 ↔ alpha-mobile `OfflineSyncConfig`（enabled 默认 false——产品红线①） */
@Serializable
data class OfflineSyncConfigPayload(
    val enabled: Boolean,
    val scopes: List<String>,
)

/** 快照条目 ↔ alpha-mobile `OfflineEntry`（备份面最小单元） */
@Serializable
data class OfflineEntryPayload(
    val key: String,
    @SerialName("payload_json") val payloadJson: String,
    @SerialName("content_seq") val contentSeq: ULong,
)

/** 本地快照 ↔ alpha-mobile `OfflineSnapshot`（备份面载荷；scopes 为明示数据范围） */
@Serializable
data class OfflineSnapshotPayload(
    val version: String,
    val enabled: Boolean,
    val scopes: List<String>,
    @SerialName("captured_at") val capturedAt: String,
    val entries: List<OfflineEntryPayload>,
    val fingerprint: ULong,
)

/** 增量决策 ↔ alpha-mobile `SyncDelta`（同步面载荷；needed 才允许发起对齐） */
@Serializable
data class SyncDeltaPayload(
    val needed: Boolean,
    @SerialName("since_fingerprint") val sinceFingerprint: ULong,
    @SerialName("current_fingerprint") val currentFingerprint: ULong,
    val reason: String? = null,
)
