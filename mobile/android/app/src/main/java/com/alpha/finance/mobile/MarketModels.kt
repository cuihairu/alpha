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
