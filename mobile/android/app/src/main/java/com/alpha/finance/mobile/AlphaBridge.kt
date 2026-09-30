package com.alpha.finance.mobile

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.decodeFromString
import kotlinx.serialization.json.Json
import uniffi.alpha_mobile.MobileCore
import uniffi.alpha_mobile.MobileException

/**
 * Rust 核心库消费面（L301）：持有 UniFFI 生成的 [MobileCore]，是壳层接触
 * Rust 的唯一入口。所有 FFI 调用一律切 [Dispatchers.IO]——绑定本身无平台线程
 * 约束，但 `analyze_json` 走整段 K 线计算，主线程直调会卡帧（架构文档 §6
 * 线程模型）。生成绑定的 [MobileException] 在 [translate] 处就地翻译成
 * [Error]——UI 层只认 Kotlin 侧错误面，不 import 任何 uniffi 类型。
 */
class AlphaBridge(symbols: List<String>, apiUrl: String) : AutoCloseable {
    /** Kotlin 侧错误面：Rust 语义不在 UI 层重复实现，只做类型搬运与文案兜底 */
    sealed class Error(message: String) : Exception(message) {
        /** 观察列表外（核心库唯一业务判断的镜像） */
        class InvalidSymbol(val symbol: String) : Error("标的 $symbol 不在观察列表")

        /** 其余核心错误（Rust Display 全文透传） */
        class Failed(message: String) : Error(message)
    }

    private val core = MobileCore(symbols, apiUrl)
    private val json = Json { ignoreUnknownKeys = true }

    /** [MobileException] → [Error]：除类型搬运不加任何逻辑 */
    private inline fun <T> translate(block: () -> T): T =
        try {
            block()
        } catch (e: MobileException.InvalidSymbol) {
            throw Error.InvalidSymbol(e.symbol)
        } catch (e: MobileException.Failed) {
            throw Error.Failed(e.detail)
        } catch (e: MobileException) {
            throw Error.Failed(e.message ?: "alpha-mobile: 未知错误")
        }

    /** 核心库状态（版本 + 观察列表 + 后端地址） */
    suspend fun status(): StatusPayload = withContext(Dispatchers.IO) {
        translate { json.decodeFromString(core.statusJson()) }
    }

    /** 观察列表内标的的快照行情；列表外抛 [Error.InvalidSymbol] */
    suspend fun quote(symbol: String): QuotePayload = withContext(Dispatchers.IO) {
        translate { json.decodeFromString(core.quoteJson(symbol)) }
    }

    /** 观察列表内标的的技术分析；列表外抛 [Error.InvalidSymbol] */
    suspend fun analyze(symbol: String): AnalysisPayload = withContext(Dispatchers.IO) {
        translate { json.decodeFromString(core.analyzeJson(symbol)) }
    }

    /** 逐标的快照；单项失败（列表外/核心错误）按项降级，不拖垮整屏 */
    suspend fun quotes(symbols: List<String>): List<QuotePayload> = symbols.mapNotNull { symbol ->
        try {
            quote(symbol)
        } catch (ignored: Error) {
            null
        }
    }

    /** 释放 Rust 侧 Arc；Activity 销毁时调用（泄漏面见生成绑定文档） */
    override fun close() = core.destroy()
}
