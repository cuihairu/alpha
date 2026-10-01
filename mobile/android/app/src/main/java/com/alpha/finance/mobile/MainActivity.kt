package com.alpha.finance.mobile

import android.os.Bundle
import android.view.WindowManager
import androidx.activity.compose.setContent
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.State
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.fragment.app.FragmentActivity
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
 *
 * 主题接线（L511，docs/theme-adaptation.md）：三态偏好（跟随系统/浅/深）
 * 经 [AlphaTheme] 切 Material3 配色，语义对齐 web 端 ThemeToggle。
 *
 * 隐私接线（L512，docs/mobile-privacy.md）：FLAG_SECURE 防截屏/最近任务
 * 缩略图；生物识别门 [GateLayer]（设置 opt-in + 设备能力可用才激活），
 * 退后台即重锁（[onStop] → [GateStateMachine.onBackground]）。基类为
 * FragmentActivity——androidx.biometric 要求（ComponentActivity 的父类，
 * Compose setContent 不受影响）。
 */
class MainActivity : FragmentActivity() {
    private val bridge = AlphaBridge(
        symbols = listOf("600519", "000001"),
        // Android 模拟器约定：10.0.2.2 = 宿主机回环（骨架期仅作配置槽展示）
        apiUrl = "http://10.0.2.2:8080",
    )

    private val gate = GateStateMachine(PrivacySettings())
    private val gateView = mutableStateOf(LockState.Unlocked to 0)
    private val themeStore by lazy { ThemeSettingsStore(SharedPreferencesKeyValueStore(this)) }

    override fun onCreate(savedInstanceState: Bundle?) {
        val settings = PrivacySettingsStore(SharedPreferencesKeyValueStore(this)).load()
        val themePref = themeStore.load()
        gate.updateSettings(settings)
        gateView.value = gate.state to gate.failedAttempts
        // L509 深链：快捷方式/widget 点击带入预选标的（标准 launchMode 下
        // 每次新建实例走 onCreate；进程内热路由归单实例改造 TODO）
        val deepLinkSymbol = intent?.getStringExtra(EXTRA_SYMBOL)
        // 防截屏/最近任务缩略图（隐私开关默认开；用户可关）
        if (settings.screenshotShield) {
            window.setFlags(
                WindowManager.LayoutParams.FLAG_SECURE,
                WindowManager.LayoutParams.FLAG_SECURE,
            )
        }
        super.onCreate(savedInstanceState)
        setContent {
            // L511：三态主题（缺省跟随系统深色；偏好变更重启生效）
            AlphaTheme(themePref) {
                Surface(Modifier.fillMaxSize()) {
                    GateLayer(
                        gate = gate,
                        view = gateView,
                        settings = settings,
                        capabilities = AndroidBiometricCapabilities(this),
                        onRequestUnlock = { requestUnlock() },
                    ) {
                        MarketScreen(bridge, initialSymbol = deepLinkSymbol)
                    }
                }
            }
        }
    }

    /** 退后台即锁：状态机迁移 + Compose 镜像同步 */
    override fun onStop() {
        gate.onBackground()
        gateView.value = gate.state to gate.failedAttempts
        super.onStop()
    }

    /** 生物识别解锁流：认证成功/失败都同步 Compose 镜像（失败累计计数） */
    private fun requestUnlock() {
        promptBiometricGate(
            activity = this,
            title = "解锁 Alpha Mobile",
            subtitle = "使用生物识别解锁行情与离线数据",
            negativeText = "取消",
            onSuccess = { _ ->
                gate.onAuthSuccess()
                gateView.value = gate.state to gate.failedAttempts
            },
            onFailure = { _ ->
                gate.onAuthFailure()
                gateView.value = gate.state to gate.failedAttempts
            },
        )
    }

    override fun onDestroy() {
        bridge.close()
        super.onDestroy()
    }

    companion object {
        /** 深链 extra：快捷方式与 widget 点击共用的预选标的键（L509） */
        const val EXTRA_SYMBOL = "alpha_extra_symbol"
    }
}

@OptIn(ExperimentalMaterial3Api::class, ExperimentalFoundationApi::class)
@Composable
fun MarketScreen(bridge: AlphaBridge, initialSymbol: String? = null) {
    var status by remember { mutableStateOf<StatusPayload?>(null) }
    var quotes by remember { mutableStateOf<List<QuotePayload>>(emptyList()) }
    var analysis by remember { mutableStateOf<AnalysisPayload?>(null) }
    var message by remember { mutableStateOf<String?>(null) }
    var refreshing by remember { mutableStateOf(false) }
    var headerCollapsed by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val context = androidx.compose.ui.platform.LocalContext.current

    LaunchedEffect(Unit) {
        try {
            status = bridge.status()
            quotes = bridge.quotes(status?.symbols.orEmpty())
            // L509：行情到手即发布 widget（首只标的，应用是唯一数据源）
            publishWidgetQuote(context, quotes)
            message = null
        } catch (e: AlphaBridge.Error) {
            message = e.message
        }
    }

    // L509 深链 effect 置于 loadAnalysis 声明之后（本地函数先声明后引用）

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

    // L509 深链：快捷方式/widget 带入标的 → 直接触发该标的分析
    LaunchedEffect(initialSymbol) {
        initialSymbol?.takeIf { it.isNotEmpty() }?.let { loadAnalysis(it) }
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
                        // L509：下拉刷新同样发布 widget 最新态
                        publishWidgetQuote(context, quotes)
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

/**
 * 门覆盖层（L512）：仅在「设置开启 + 设备生物识别可用 + 状态机为锁」时
 * 渲染锁屏，否则直出内容。能力不可用（无硬件/未录入）时门不激活——
 * 不把用户锁在门外，缺省安全由静态加密承担（docs/mobile-privacy.md §3）。
 * `view` 为 Activity 侧 Compose 镜像（认证回调与 onStop 同步进来）。
 */
@Composable
fun GateLayer(
    gate: GateStateMachine,
    view: State<Pair<LockState, Int>>,
    settings: PrivacySettings,
    capabilities: BiometricCapabilities,
    onRequestUnlock: () -> Unit,
    content: @Composable () -> Unit,
) {
    val available = remember { capabilities.canAuthenticate() }
    val gateActive = settings.biometricEnabled && available == GateAvailability.Available
    val (state, failed) = view.value
    if (!gateActive || state == LockState.Unlocked) {
        content()
    } else {
        LockScreen(failedAttempts = failed, onUnlock = onRequestUnlock)
    }
}

@Composable
fun LockScreen(failedAttempts: Int, onUnlock: () -> Unit) {
    Column(
        modifier = Modifier.fillMaxSize().padding(32.dp),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text("Alpha Mobile 已锁定", style = MaterialTheme.typography.titleLarge)
        Spacer(Modifier.height(8.dp))
        Text(
            text = "生物识别解锁以查看行情与离线数据",
            fontSize = 13.sp,
            color = MaterialTheme.colorScheme.secondary,
        )
        if (failedAttempts > 0) {
            Spacer(Modifier.height(4.dp))
            Text(
                text = "连续失败 $failedAttempts 次",
                fontSize = 12.sp,
                color = MaterialTheme.colorScheme.error,
            )
        }
        Spacer(Modifier.height(24.dp))
        Button(onClick = onUnlock) {
            Text("生物识别解锁")
        }
    }
}
