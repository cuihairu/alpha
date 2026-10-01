package com.alpha.finance.mobile

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.lightColorScheme
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.launch

/**
 * Jetpack Compose 壳（L301 最小可编译骨架；L367 增触屏手势）：加载核心库状态
 * 与观察列表快照，行内可触发技术分析。FFI 全部经 [AlphaBridge]（后台线程）；
 * Rust 侧错误转提示文案不崩壳。观察列表/后端地址为内置演示值——api-gateway
 * 接入、配置下行与离线同步归后续 TODO。
 *
 * 手势接线（docs/mobile-gestures.md §2/§3）：下拉刷新（PullToRefreshBox →
 * [refreshWithManualSync]）、长按行情行（→ `analyze`）、双击状态头（折叠
 * api_url 详情行，local-only）。手势编排只在 [Gestures.kt]，本文件只做
 * Compose modifier 挂接与状态应用。
 */
class MainActivity : ComponentActivity() {
    private val bridge = AlphaBridge(
        symbols = listOf("600519", "000001"),
        // Android 模拟器约定：10.0.2.2 = 宿主机回环（骨架期仅作配置槽展示）
        apiUrl = "http://10.0.2.2:8080",
    )

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            MaterialTheme(colorScheme = lightColorScheme()) {
                Surface(Modifier.fillMaxSize()) {
                    MarketScreen(bridge)
                }
            }
        }
    }

    override fun onDestroy() {
        bridge.close()
        super.onDestroy()
    }
}

@OptIn(ExperimentalMaterial3Api::class, ExperimentalFoundationApi::class)
@Composable
fun MarketScreen(bridge: AlphaBridge) {
    var status by remember { mutableStateOf<StatusPayload?>(null) }
    var quotes by remember { mutableStateOf<List<QuotePayload>>(emptyList()) }
    var analysis by remember { mutableStateOf<AnalysisPayload?>(null) }
    var message by remember { mutableStateOf<String?>(null) }
    var refreshing by remember { mutableStateOf(false) }
    var headerCollapsed by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()

    LaunchedEffect(Unit) {
        try {
            status = bridge.status()
            quotes = bridge.quotes(status?.symbols.orEmpty())
            message = null
        } catch (e: AlphaBridge.Error) {
            message = e.message
        }
    }

    /** 长按行情行 / 「分析」按钮共用：触发该标的技术分析并展示摘要 */
    fun loadAnalysis(symbol: String) {
        scope.launch {
            try {
                analysis = bridge.analyze(symbol)
                message = null
            } catch (e: AlphaBridge.Error) {
                message = e.message
            }
        }
    }

    Column(Modifier.fillMaxSize().padding(16.dp)) {
        // 状态头：双击折叠/展开 api_url 详情行（GestureAction.DoubleTapHeader，local-only）
        Text(
            text = when {
                headerCollapsed ->
                    status?.let { "Alpha Mobile v${it.version}" } ?: "Alpha Mobile"
                else ->
                    status?.let { "Alpha Mobile v${it.version} · ${it.apiUrl}" } ?: "Alpha Mobile"
            },
            style = MaterialTheme.typography.titleMedium,
            modifier = Modifier.combinedClickable(
                onClick = {},
                onDoubleClick = {
                    headerCollapsed = !headerCollapsed
                },
            ),
        )
        message?.let {
            Text(text = it, color = MaterialTheme.colorScheme.error, fontSize = 12.sp)
        }
        Spacer(Modifier.height(8.dp))
        // 下拉刷新：GestureAction.PullToRefresh → refreshWithManualSync（§4 编排）
        PullToRefreshBox(
            isRefreshing = refreshing,
            onRefresh = {
                scope.launch {
                    refreshing = true
                    try {
                        val outcome = refreshWithManualSync(
                            bridge,
                            status?.symbols.orEmpty(),
                        )
                        quotes = outcome.quotes
                        message = null
                    } catch (e: AlphaBridge.Error) {
                        message = e.message
                    } finally {
                        refreshing = false
                    }
                }
            },
            modifier = Modifier.weight(1f),
        ) {
            LazyColumn {
                items(quotes) { quote ->
                    // 长按行情行：GestureAction.LongPressAnalyze → analyze（与下方按钮并存）
                    ListItem(
                        modifier = Modifier.combinedClickable(
                            onClick = {},
                            onLongClick = { loadAnalysis(quote.symbol) },
                        ),
                        headlineContent = { Text("${quote.symbol}  ${quote.price}") },
                        supportingContent = { Text("成交量 ${quote.volume}") },
                        trailingContent = {
                            TextButton(onClick = { loadAnalysis(quote.symbol) }) {
                                Text("分析")
                            }
                        },
                    )
                }
            }
        }
        analysis?.let { result ->
            HorizontalDivider()
            Text(
                text = "${result.symbol} · ${result.recommendation} · " +
                    "置信度 ${result.confidence} · 波动率 ${result.riskMetrics.volatility}",
                fontSize = 12.sp,
            )
        }
    }
}
