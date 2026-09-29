// Alpha Finance Web 应用主逻辑

// 全局变量
let realtimeSocket = null;
const realtimeQuotes = {}; // symbol → 最近一条实时报价（WebSocket 推送）
const realtimeSyncSnapshots = {}; // symbol → 本地合并态快照（Sync Delta 帧增量合入）

// ===== 后端接入配置 =====
// 默认直连服务（data-engine 的 CORS 默认开启，浏览器跨源可直连）；
// 也可改指 api-gateway（注意其默认端口同为 8080，与 web 静态服务器同机同跑时需错开）：
//   REST → http://127.0.0.1:8080/api/v1   WS → ws://127.0.0.1:8080/ws
// 修改后点「应用连接配置」持久化到 localStorage。
let apiConfig = {
    restBase: localStorage.getItem('alpha.restBase') || 'http://127.0.0.1:8081',
    wsUrl: localStorage.getItem('alpha.wsUrl') || 'ws://127.0.0.1:8082/ws',
};

function initApiConfigInputs() {
    document.getElementById('rest-base').value = apiConfig.restBase;
    document.getElementById('ws-url').value = apiConfig.wsUrl;
}

function applyApiConfig() {
    const rest = document.getElementById('rest-base').value.trim().replace(/\/+$/, '');
    const ws = document.getElementById('ws-url').value.trim();
    if (rest) apiConfig.restBase = rest;
    if (ws) apiConfig.wsUrl = ws;
    localStorage.setItem('alpha.restBase', apiConfig.restBase);
    localStorage.setItem('alpha.wsUrl', apiConfig.wsUrl);
    showStatus('api-config-status', 'success', `✅ 已应用：REST ${apiConfig.restBase} / WS ${apiConfig.wsUrl}`);
}

// 拉取真实历史（data-engine /stocks/:symbol/history），映射为分析器所需数据形状。
// 响应数据项：{timestamp, price, volume, metadata:{bid,ask,open,high,low}}
async function fetchStockHistory(symbol, days = 90) {
    const url = `${apiConfig.restBase}/stocks/${encodeURIComponent(symbol)}/history?days=${days}`;
    const response = await fetch(url);
    if (!response.ok) {
        throw new Error(`data-engine HTTP ${response.status}`);
    }
    const payload = await response.json();
    if (!payload.success) {
        throw new Error(payload.error || '查询失败');
    }
    return (payload.data || []).map(point => ({
        symbol: symbol,
        timestamp: point.timestamp,
        price: point.price,
        volume: point.volume || 0,
        bid: point.metadata ? point.metadata.bid : null,
        ask: point.metadata ? point.metadata.ask : null,
        open: point.metadata ? point.metadata.open : null,
        high: point.metadata ? point.metadata.high : null,
        low: point.metadata ? point.metadata.low : null,
    }));
}

document.addEventListener('DOMContentLoaded', initApiConfigInputs);

// 分析股票
async function analyzeStock() {
    if (!window.analyzer) {
        showStatus('analysis-status', 'error', '❌ 分析引擎未初始化');
        return;
    }

    const symbol = document.getElementById('symbol').value.trim();
    if (!symbol) {
        showStatus('analysis-status', 'error', '❌ 请输入股票代码');
        return;
    }

    showStatus('analysis-status', 'loading', '🔄 正在分析 ' + symbol + '...');

    try {
        // 真实历史优先（data-engine），不可达时回退演示数据并在状态中注明
        let marketData;
        let dataSource;
        try {
            marketData = await fetchStockHistory(symbol, 252); // 一年的交易日
            if (marketData.length === 0) {
                throw new Error('后端无该股票历史数据');
            }
            dataSource = `data-engine（${marketData.length} 个真实数据点）`;
        } catch (backendError) {
            console.warn('data-engine 不可达，回退演示数据:', backendError);
            marketData = generateMockData(symbol, 252);
            dataSource = '演示数据（后端不可达）';
        }
        window.lastHistorySeries = marketData;

        // 执行分析
        const result = await window.analyzer.analyzeSymbol(symbol, marketData);

        // 显示分析结果
        displayAnalysisResults(symbol, result);
        showStatus('analysis-status', 'success', `✅ ${symbol} 分析完成（${dataSource}）`);

        // 同时计算技术指标
        calculateIndicatorsForData(marketData);

    } catch (error) {
        console.error('分析失败:', error);
        showStatus('analysis-status', 'error', '❌ 分析失败: ' + error.message);
    }
}

// 显示分析结果
function displayAnalysisResults(symbol, result) {
    const container = document.getElementById('analysis-results');

    const html = `
        <div style="margin-top: 20px;">
            <h4>📊 ${symbol} 分析结果</h4>
            <div class="indicator-value">
                推荐: <span style="color: ${getSignalColor(result.recommendation)}">${getSignalText(result.recommendation)}</span>
            </div>
            <div style="margin: 16px 0;">
                <strong>置信度:</strong> ${result.confidence ? result.confidence.toFixed(1) + '%' : 'N/A'}
                <div style="width: 100%; background: #e2e8f0; border-radius: 4px; height: 8px; margin-top: 4px;">
                    <div style="width: ${result.confidence || 0}%; background: ${getSignalColor(result.recommendation)}; height: 100%; border-radius: 4px;"></div>
                </div>
            </div>

            <div style="display: grid; grid-template-columns: 1fr 1fr; gap: 12px; margin-top: 16px;">
                <div>
                    <strong>波动率:</strong><br>
                    <span style="font-size: 1.2rem;">${result.riskMetrics ? result.riskMetrics.volatility.toFixed(2) + '%' : 'N/A'}</span>
                </div>
                <div>
                    <strong>最大回撤:</strong><br>
                    <span style="font-size: 1.2rem; color: #ef4444;">${result.riskMetrics ? (result.riskMetrics.maxDrawdown * 100).toFixed(2) + '%' : 'N/A'}</span>
                </div>
                <div>
                    <strong>夏普比率:</strong><br>
                    <span style="font-size: 1.2rem; color: #10b981;">${result.riskMetrics && result.riskMetrics.sharpeRatio ? result.riskMetrics.sharpeRatio.toFixed(2) : 'N/A'}</span>
                </div>
                <div>
                    <strong>分析时间:</strong><br>
                    <span style="font-size: 0.9rem;">${new Date(result.analyzedAt).toLocaleString()}</span>
                </div>
            </div>

            <div style="margin-top: 16px;">
                <strong>计算指标:</strong>
                <div style="display: flex; flex-wrap: wrap; gap: 8px; margin-top: 8px;">
                    ${result.indicators ? result.indicators.map(ind =>
                        `<span style="background: #f3f4f6; padding: 4px 8px; border-radius: 4px; font-size: 0.85rem;">${ind.name}</span>`
                    ).join('') : '无指标数据'}
                </div>
            </div>
        </div>
    `;

    container.innerHTML = html;
}

// 获取信号颜色
function getSignalColor(signal) {
    switch (signal) {
        case 'BUY': return '#10b981';
        case 'SELL': return '#ef4444';
        default: return '#6b7280';
    }
}

// 获取信号文本
function getSignalText(signal) {
    switch (signal) {
        case 'BUY': return '买入 📈';
        case 'SELL': return '卖出 📉';
        default: return '持有 ➡️';
    }
}

// 计算技术指标
async function calculateIndicators() {
    if (!window.analyzer) {
        showStatus('indicators-status', 'error', '❌ 分析引擎未初始化');
        return;
    }

    showStatus('indicators-status', 'loading', '🔄 正在计算技术指标...');

    try {
        // 真实历史优先（复用分析卡片输入的股票代码），不可达时回退演示数据
        const symbol = document.getElementById('symbol').value.trim() || 'DEMO';
        let marketData;
        try {
            marketData = await fetchStockHistory(symbol, 100);
            if (marketData.length === 0) {
                throw new Error('后端无该股票历史数据');
            }
        } catch (backendError) {
            console.warn('data-engine 不可达，回退演示数据:', backendError);
            marketData = generateMockData(symbol, 100);
        }
        const prices = marketData.map(d => d.price);

        const rsiPeriod = parseInt(document.getElementById('rsi-period').value);
        const smaShort = parseInt(document.getElementById('sma-short').value);
        const smaLong = parseInt(document.getElementById('sma-long').value);

        // 计算所有指标
        const indicators = window.analyzer.calculateAllIndicators(
            new Float64Array(prices),
            rsiPeriod,
            smaShort,
            smaLong,
            12, 26, 9 // MACD 默认参数
        );

        displayIndicatorResults(indicators, prices);
        showStatus('indicators-status', 'success', '✅ 技术指标计算完成');

    } catch (error) {
        console.error('指标计算失败:', error);
        showStatus('indicators-status', 'error', '❌ 计算失败: ' + error.message);
    }
}

// 为特定数据计算指标
async function calculateIndicatorsForData(mockData) {
    if (!window.analyzer) return;

    try {
        const prices = mockData.map(d => d.price);
        const indicators = window.analyzer.calculateAllIndicators(
            new Float64Array(prices),
            14, 20, 50, 12, 26, 9
        );

        displayIndicatorResults(indicators, prices, false);

    } catch (error) {
        console.error('指标计算失败:', error);
    }
}

// 显示指标结果
function displayIndicatorResults(indicators, prices, showDetails = true) {
    const container = document.getElementById('indicators-results');

    const currentRSI = indicators.rsi[indicators.rsi.length - 1] || 0;
    const currentMACD = indicators.macd.line[indicators.macd.line.length - 1] || 0;
    const currentSignal = indicators.macd.signal[indicators.macd.signal.length - 1] || 0;
    const currentPrice = prices[prices.length - 1] || 0;
    const currentSMA = indicators.sma_short[indicators.sma_short.length - 1] || 0;

    let html = `
        <div style="margin-top: 20px;">
            <h4>📈 技术指标结果</h4>
            <div style="display: grid; grid-template-columns: 1fr 1fr; gap: 16px; margin-top: 16px;">
                <div>
                    <strong>RSI (14):</strong><br>
                    <span style="font-size: 1.5rem; color: ${getRSIColor(currentRSI)}">${currentRSI.toFixed(2)}</span>
                    <div style="font-size: 0.85rem; color: #6b7280; margin-top: 4px;">
                        ${getRSIStatus(currentRSI)}
                    </div>
                </div>
                <div>
                    <strong>MACD:</strong><br>
                    <span style="font-size: 1.2rem; color: ${currentMACD > currentSignal ? '#10b981' : '#ef4444'}">${currentMACD.toFixed(3)}</span>
                    <div style="font-size: 0.85rem; color: #6b7280; margin-top: 4px;">
                        信号: ${currentSignal.toFixed(3)}
                    </div>
                </div>
                <div>
                    <strong>SMA (20):</strong><br>
                    <span style="font-size: 1.3rem;">$${currentSMA.toFixed(2)}</span>
                    <div style="font-size: 0.85rem; color: #6b7280; margin-top: 4px;">
                        当前价格: $${currentPrice.toFixed(2)}
                    </div>
                </div>
                <div>
                    <strong>价格相对均线:</strong><br>
                    <span style="font-size: 1.3rem; color: ${currentPrice > currentSMA ? '#10b981' : '#ef4444'}">
                        ${currentPrice > currentSMA ? '↑' : '↓'} ${Math.abs(((currentPrice - currentSMA) / currentSMA) * 100).toFixed(2)}%
                    </span>
                </div>
            </div>
    `;

    if (showDetails) {
        html += `
            <div style="margin-top: 20px;">
                <button class="btn" onclick="drawPriceChart()" style="font-size: 0.9rem; padding: 8px 16px;">
                    📊 绘制图表
                </button>
                <canvas id="price-chart" style="display: none; margin-top: 16px; width: 100%; height: 300px;"></canvas>
            </div>
        `;
    }

    container.innerHTML = html;
}

// 获取 RSI 颜色
function getRSIColor(rsi) {
    if (rsi > 70) return '#ef4444'; // 超买
    if (rsi < 30) return '#10b981'; // 超卖
    return '#6b7280'; // 中性
}

// 获取 RSI 状态
function getRSIStatus(rsi) {
    if (rsi > 70) return '超买 - 可能回调';
    if (rsi < 30) return '超卖 - 可能反弹';
    if (rsi > 50) return '偏强势';
    return '偏弱势';
}

// 开始实时监控
function startRealTime() {
    if (!window.analyzer) {
        showStatus('realtime-status', 'error', '❌ 分析引擎未初始化');
        return;
    }

    if (window.isRealTimeRunning) {
        stopRealTime();
        return;
    }

    const watchlistInput = document.getElementById('watchlist').value.trim();
    if (!watchlistInput) {
        showStatus('realtime-status', 'error', '❌ 请输入观察列表');
        return;
    }

    const watchlist = watchlistInput.split(',').map(s => s.trim().toUpperCase()).filter(s => s);
    if (watchlist.length === 0) {
        showStatus('realtime-status', 'error', '❌ 无效的股票代码');
        return;
    }

    window.isRealTimeRunning = true;
    window.realtimeWatchlist = watchlist;
    document.querySelector('.btn[onclick="startRealTime()"]').textContent = '停止监控';

    showStatus('realtime-status', 'loading', `🔄 连接 real-time-feed（${apiConfig.wsUrl}）...`);
    connectRealtimeSocket(watchlist);
}

// 连接 real-time-feed 的 /ws，接收 real_time_quotes 通道推送。
// 消息协议（实测；serde tag，变体名未 rename 为小写）：
//   Sync 帧（版本化增量，服务端默认）：{"type":"Sync","channel":"real_time_quotes","seq":N,
//     "op":"Full|Delta","data":{...},"timestamp":ms}
//     - Full：全量快照（首帧 / Resync 恢复基线）；Delta：相对上一帧的变化字段
//     - 前端把 Delta 合入 realtimeSyncSnapshots[symbol]，渲染以合并态为准
//   Data 帧（旧版兼容保留）：{"type":"Data","channel":"real_time_quotes","data":{...},"timestamp":ms}
// data-engine 会把 normalized 转发回 quotes.normalized，同一条行情可能推两帧，渲染按 symbol 覆盖去重。
function connectRealtimeSocket(watchlist) {
    let socket;
    try {
        socket = new WebSocket(apiConfig.wsUrl);
    } catch (error) {
        finishRealtimeStopped();
        showStatus('realtime-status', 'error', '❌ WS 地址无效: ' + error.message);
        return;
    }
    realtimeSocket = socket;

    socket.onopen = () => {
        if (!window.isRealTimeRunning) return;
        showStatus('realtime-status', 'success', `✅ 已连接 real-time-feed，等待 ${watchlist.length} 只股票的行情推送...`);
        // 先渲染「等待推送」占位，避免空白
        renderRealtimeQuotes();
    };

    socket.onmessage = (event) => {
        if (!window.isRealTimeRunning) return;
        let message;
        try {
            message = JSON.parse(event.data);
        } catch (error) {
            return; // 非JSON帧（如欢迎语）忽略
        }
        // 版本化同步帧（{"type":"Sync","channel":..,"seq":N,"op":"Full|Delta","data":..}）：
        // Full = 全量快照（首帧/Resync 恢复），Delta = 相对上一帧的变化字段（增量合入本地快照）。
        // 服务端保证通道内 seq 单调递增；前端以本地合并态为准渲染，无需关心 seq。
        if (String(message.type || '').toLowerCase() === 'sync') {
            if (message.channel !== 'real_time_quotes') return;
            const quote = message.data;
            if (!quote || !quote.symbol) return;
            const symbol = String(quote.symbol).toUpperCase();
            if (!watchlist.includes(symbol)) return;
            const prev = realtimeSyncSnapshots[symbol] || {};
            realtimeSyncSnapshots[symbol] = { ...prev, ...quote };
            realtimeQuotes[symbol] = realtimeSyncSnapshots[symbol];
            renderRealtimeQuotes();
            return;
        }
        // 注意：服务端 serde 变体名未 rename，线上实际是 "Data"（PascalCase），大小写不敏感匹配
        if (String(message.type || '').toLowerCase() !== 'data') return;
        if (message.channel !== 'real_time_quotes') return;
        const quote = message.data;
        if (!quote || !quote.symbol) return;
        const symbol = String(quote.symbol).toUpperCase();
        if (!watchlist.includes(symbol)) return;
        // 旧版 Data 帧即全量，直接作为本地合并态基线
        realtimeSyncSnapshots[symbol] = quote;
        realtimeQuotes[symbol] = quote;
        renderRealtimeQuotes();
    };

    socket.onerror = () => {
        if (window.isRealTimeRunning) {
            showStatus('realtime-status', 'error', '❌ WebSocket 连接失败，请确认 real-time-feed 已启动且 WS 地址正确');
        }
    };

    socket.onclose = () => {
        if (window.isRealTimeRunning) {
            finishRealtimeStopped();
            showStatus('realtime-status', 'error', '❌ 连接已断开，实时监控停止');
        }
    };
}

// 渲染观察列表：已收到推送的显示最新报价，未收到的显示等待占位
function renderRealtimeQuotes() {
    const container = document.getElementById('realtime-results');
    const watchlist = window.realtimeWatchlist || [];
    const timestamp = new Date().toLocaleTimeString();

    const rows = watchlist.map(symbol => {
        const quote = realtimeQuotes[symbol];
        if (!quote) {
            return `
            <div style="display: flex; justify-content: space-between; align-items: center; padding: 12px; background: #f9fafb; border-radius: 8px; color: #6b7280;">
                <div><strong>${symbol}</strong></div>
                <div>等待行情推送…</div>
            </div>`;
        }
        const currentPrice = quote.price;
        const changePercent = quote.change_percent || 0;
        const isPositive = changePercent >= 0;

        return `
            <div style="display: flex; justify-content: space-between; align-items: center; padding: 12px; background: #f9fafb; border-radius: 8px;">
                <div>
                    <strong>${symbol}</strong>
                    <div style="font-size: 1.2rem; margin: 4px 0;">$${Number(currentPrice).toFixed(2)}</div>
                    <div style="font-size: 0.8rem; color: #9ca3af;">成交量 ${formatNumber(quote.volume || 0)}</div>
                </div>
                <div style="text-align: right; color: ${isPositive ? '#10b981' : '#ef4444'};">
                    <div style="font-size: 1.1rem;">
                        ${isPositive ? '↑' : '↓'} ${Math.abs(changePercent).toFixed(2)}%
                    </div>
                    <div style="font-size: 0.85rem;">
                        ${isPositive ? '+' : '-'}$${Math.abs(quote.change || 0).toFixed(2)}
                    </div>
                </div>
            </div>`;
    }).join('');

    container.innerHTML = `
        <div style="margin-top: 20px;">
            <h5>🕐 最后更新: ${timestamp}（WebSocket 推送）</h5>
            <div style="display: grid; gap: 12px; margin-top: 12px;">${rows}</div>
        </div>`;
}

// 复位按钮与运行状态（连接断开或手动停止共用）
function finishRealtimeStopped() {
    window.isRealTimeRunning = false;
    const btn = document.querySelector('.btn[onclick="startRealTime()"]');
    if (btn) btn.textContent = '开始实时监控';
}

// 停止实时监控
function stopRealTime() {
    finishRealtimeStopped();
    if (realtimeSocket) {
        try { realtimeSocket.close(); } catch (error) { /* 已关闭则忽略 */ }
        realtimeSocket = null;
    }
    showStatus('realtime-status', 'success', '⏹️ 实时监控已停止');
}

// 获取性能指标
function getPerformanceMetrics() {
    if (!window.analyzer) {
        showStatus('analysis-status', 'error', '❌ 分析引擎未初始化');
        return;
    }

    try {
        const metrics = window.analyzer.getPerformanceMetrics();
        const container = document.getElementById('performance-results');

        const html = `
            <div style="margin-top: 20px;">
                <h4>⚡ 系统性能</h4>
                <div style="margin-top: 16px;">
                    <div><strong>加载时间:</strong> ${parseFloat(metrics.timing.now).toFixed(2)}ms</div>
                    <div><strong>时间戳:</strong> ${new Date(metrics.timestamp).toLocaleString()}</div>
                    <div style="margin-top: 12px;">
                        <button class="btn" onclick="window.analyzer.forceGC()" style="font-size: 0.9rem; padding: 8px 16px;">
                            🗑️ 强制垃圾回收
                        </button>
                    </div>
                </div>
            </div>
        `;

        container.innerHTML = html;
    } catch (error) {
        console.error('获取性能指标失败:', error);
    }
}

// 简单的图表绘制 (使用 Canvas)
function drawPriceChart() {
    const canvas = document.getElementById('price-chart');
    if (!canvas) return;

    canvas.style.display = 'block';
    const ctx = canvas.getContext('2d');

    // 优先使用分析时拉取的真实历史收盘价；无数据时回退随机示例
    let prices;
    if (window.lastHistorySeries && window.lastHistorySeries.length > 1) {
        prices = window.lastHistorySeries.map(d => d.price);
    } else {
        prices = [];
        let basePrice = 100;
        for (let i = 0; i < 50; i++) {
            basePrice += (Math.random() - 0.5) * 2;
            prices.push(basePrice);
        }
    }

    // 清除画布
    ctx.clearRect(0, 0, canvas.width, canvas.height);

    // 设置样式
    ctx.strokeStyle = '#667eea';
    ctx.lineWidth = 2;
    ctx.fillStyle = '#667eea';

    // 绘制简单的价格线图
    const width = canvas.width;
    const height = canvas.height;
    const padding = 20;

    const maxPrice = Math.max(...prices);
    const minPrice = Math.min(...prices);
    const priceRange = maxPrice - minPrice;

    ctx.beginPath();
    prices.forEach((price, i) => {
        const x = padding + (i / (prices.length - 1)) * (width - 2 * padding);
        const y = padding + (1 - (price - minPrice) / priceRange) * (height - 2 * padding);

        if (i === 0) {
            ctx.moveTo(x, y);
        } else {
            ctx.lineTo(x, y);
        }

        // 绘制数据点
        ctx.fillRect(x - 2, y - 2, 4, 4);
    });

    ctx.stroke();
}

// 工具函数：格式化数字
function formatNumber(num, decimals = 2) {
    return num.toLocaleString('zh-CN', {
        minimumFractionDigits: decimals,
        maximumFractionDigits: decimals
    });
}

// 工具函数：格式化货币
function formatCurrency(amount) {
    return new Intl.NumberFormat('zh-CN', {
        style: 'currency',
        currency: 'USD'
    }).format(amount);
}

// 页面卸载时清理
window.addEventListener('beforeunload', () => {
    if (realtimeSocket) {
        try { realtimeSocket.close(); } catch (error) { /* 已关闭则忽略 */ }
    }
});