package com.alpha.finance.mobile

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.widget.RemoteViews
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json
import java.time.Instant
import java.time.OffsetDateTime
import java.util.Locale

/**
 * 桌面小组件与快捷方式（L509）：首只观察标的的行情小卡 + 静态快捷方式
 * 深链预选分析。
 *
 * 数据面：**只读应用内已取得的行情快照**——[MarketScreen] 每次刷新经
 * [publishWidgetQuote] 把首只标的写入 [WidgetStateStore]（单键 JSON）并
 * 触发全量 widget 重绘；无 INTERNET 权限（L301 红线），widget 自身不发起
 * 任何数据获取，系统 30min 轮询只做占位重绘。
 *
 * 可测性切面（沿 L512 口径）：状态持久化与渲染语义全部纯函数
 * （[WidgetContent.from]），JVM 单测锁定（WidgetTest）；RemoteViews/
 * PendingIntent/AppWidgetManager 只在 [AlphaQuoteWidgetProvider] 与
 * [publishWidgetQuote] 设备路径出现，单测不触框架类。
 */

/** widget 行情载荷（应用刷新 → 持久化 → widget 读取的唯一数据通道） */
@Serializable
data class WidgetQuote(
    val symbol: String,
    val price: Double,
    /** 相对开盘的涨跌百分比（无开盘价时 null，渲染为平盘 "--"） */
    @SerialName("change_pct") val changePct: Double? = null,
    /** RFC3339（沿用 FFI 载荷时间戳口径） */
    @SerialName("updated_at") val updatedAt: String,
)

/** widget 状态持久化：整卡单键 JSON，损坏 fail-safe 返 null（沿 L512 模式） */
class WidgetStateStore(private val kv: KeyValueStore) {
    private val json = Json { ignoreUnknownKeys = true }

    fun save(quote: WidgetQuote) {
        kv.put(KEY, json.encodeToString(quote))
    }

    fun load(): WidgetQuote? =
        kv.get(KEY)?.let {
            runCatching { json.decodeFromString<WidgetQuote>(it) }.getOrNull()
        }

    companion object {
        const val KEY = "alpha_widget_state"
    }
}

/** 行情快照 → widget 载荷（首只标的；涨跌幅由开盘价现算，不开盘价则 null） */
fun buildWidgetQuote(quotes: List<QuotePayload>): WidgetQuote? {
    val first = quotes.firstOrNull() ?: return null
    val changePct = first.open?.takeIf { it > 0 }?.let { (first.price - it) / it * 100 }
    return WidgetQuote(
        symbol = first.symbol,
        price = first.price,
        changePct = changePct,
        updatedAt = first.timestamp,
    )
}

/** 渲染方向（色彩映射归 Provider 设备路径；纯逻辑只判向） */
enum class WidgetDirection { UP, DOWN, FLAT, NO_DATA }

/**
 * widget 渲染语义（纯函数，单测锁定）：数字格式化、涨跌方向判定、
 * 过期灰显。价格/涨跌两位小数（Locale.ROOT 防本地化小数点漂移）；
 * 时间戳解析失败按已过期处理（fail-safe 灰显）。
 */
data class WidgetContent(
    val symbol: String?,
    val priceText: String?,
    val changeText: String?,
    val direction: WidgetDirection,
    val stale: Boolean,
) {
    companion object {
        /** 行情过期阈值：120s（与 docs/ios-live-activities.md staleDate 同口径） */
        const val STALE_AFTER_MS: Long = 120_000L

        fun from(state: WidgetQuote?, nowMs: Long, staleAfterMs: Long = STALE_AFTER_MS): WidgetContent {
            if (state == null) {
                return WidgetContent(null, null, null, WidgetDirection.NO_DATA, stale = true)
            }
            val direction = when {
                state.changePct == null -> WidgetDirection.FLAT
                state.changePct > 0 -> WidgetDirection.UP
                state.changePct < 0 -> WidgetDirection.DOWN
                else -> WidgetDirection.FLAT
            }
            val changeText = state.changePct?.let {
                String.format(Locale.ROOT, "%+.2f%%", it)
            } ?: "--"
            val updatedAtMs = parseTimestampMs(state.updatedAt)
            val stale = updatedAtMs == null || nowMs - updatedAtMs > staleAfterMs
            return WidgetContent(
                symbol = state.symbol,
                priceText = String.format(Locale.ROOT, "%.2f", state.price),
                changeText = changeText,
                direction = direction,
                stale = stale,
            )
        }

        /** RFC3339 解析收口：Instant（…Z）与带时区偏移两种形态都接 */
        fun parseTimestampMs(raw: String): Long? =
            runCatching { Instant.parse(raw).toEpochMilli() }
                .recoverCatching { OffsetDateTime.parse(raw).toInstant().toEpochMilli() }
                .getOrNull()
    }
}

/**
 * 应用刷新后的 widget 发布位（设备路径）：持久化 + 全量重绘。widget 未添加
 * 时 updateAppWidget 空转，无需存在性判断。
 */
fun publishWidgetQuote(context: Context, quotes: List<QuotePayload>) {
    val state = buildWidgetQuote(quotes) ?: return
    WidgetStateStore(SharedPreferencesKeyValueStore(context)).save(state)
    val manager = AppWidgetManager.getInstance(context)
    val provider = ComponentName(context, AlphaQuoteWidgetProvider::class.java)
    val views = AlphaQuoteWidgetProvider.remoteViewsFor(context)
    manager.updateAppWidget(manager.getAppWidgetIds(provider), views)
}

/**
 * widget 渲染器：读 [WidgetStateStore] 最新态 → RemoteViews。点击卡片深链
 * 进应用并预选该标的（与静态快捷方式同一 extra 契约）。
 */
class AlphaQuoteWidgetProvider : AppWidgetProvider() {
    override fun onUpdate(context: Context, manager: AppWidgetManager, appWidgetIds: IntArray) {
        val views = remoteViewsFor(context)
        for (id in appWidgetIds) {
            manager.updateAppWidget(id, views)
        }
    }

    companion object {
        fun remoteViewsFor(context: Context): RemoteViews {
            val state = WidgetStateStore(SharedPreferencesKeyValueStore(context)).load()
            val content = WidgetContent.from(state, nowMs = System.currentTimeMillis())
            val views = RemoteViews(context.packageName, R.layout.widget_quote)
            views.setTextViewText(
                R.id.widget_symbol,
                content.symbol ?: context.getString(R.string.widget_placeholder),
            )
            views.setTextViewText(
                R.id.widget_price,
                content.priceText ?: context.getString(R.string.widget_no_data),
            )
            views.setTextViewText(R.id.widget_change, content.changeText ?: "--")
            val color = when (content.direction) {
                WidgetDirection.UP -> R.color.widget_up
                WidgetDirection.DOWN -> R.color.widget_down
                WidgetDirection.FLAT, WidgetDirection.NO_DATA -> R.color.widget_flat
            }
            views.setTextColor(R.id.widget_change, context.getColor(color))
            // 点击深链：预选 widget 当前标的（无状态时仅打开应用）
            val intent = Intent(context, MainActivity::class.java).apply {
                action = Intent.ACTION_VIEW
                state?.symbol?.let { putExtra(MainActivity.EXTRA_SYMBOL, it) }
            }
            val pending = PendingIntent.getActivity(
                context,
                0,
                intent,
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )
            views.setOnClickPendingIntent(R.id.widget_root, pending)
            return views
        }
    }
}
