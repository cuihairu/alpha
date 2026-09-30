package com.alpha.finance.mobile

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.lightColorScheme
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
 * Jetpack Compose 壳（L301 最小可编译骨架）：加载核心库状态与观察列表快照，
 * 行内可触发技术分析。FFI 全部经 [AlphaBridge]（后台线程）；Rust 侧错误转
 * 提示文案不崩壳。观察列表/后端地址为内置演示值——api-gateway 接入、配置
 * 下行与离线同步归后续 TODO。
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

@Composable
fun MarketScreen(bridge: AlphaBridge) {
    var status by remember { mutableStateOf<StatusPayload?>(null) }
    var quotes by remember { mutableStateOf<List<QuotePayload>>(emptyList()) }
    var analysis by remember { mutableStateOf<AnalysisPayload?>(null) }
    var message by remember { mutableStateOf<String?>(null) }
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

    Column(Modifier.fillMaxSize().padding(16.dp)) {
        Text(
            text = status?.let { "Alpha Mobile v${it.version} · ${it.apiUrl}" } ?: "Alpha Mobile",
            style = MaterialTheme.typography.titleMedium,
        )
        message?.let {
            Text(text = it, color = MaterialTheme.colorScheme.error, fontSize = 12.sp)
        }
        Spacer(Modifier.height(8.dp))
        LazyColumn(Modifier.weight(1f)) {
            items(quotes) { quote ->
                ListItem(
                    headlineContent = { Text("${quote.symbol}  ${quote.price}") },
                    supportingContent = { Text("成交量 ${quote.volume}") },
                    trailingContent = {
                        TextButton(onClick = {
                            scope.launch {
                                try {
                                    analysis = bridge.analyze(quote.symbol)
                                    message = null
                                } catch (e: AlphaBridge.Error) {
                                    message = e.message
                                }
                            }
                        }) { Text("分析") }
                    },
                )
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
