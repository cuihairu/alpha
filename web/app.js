// Alpha Finance Web 应用主逻辑

// 全局变量
let realtimeInterval = null;
let chartInstances = {};

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
        // 生成模拟数据
        const mockData = generateMockData(symbol, 252); // 一年的交易日

        // 执行分析
        const result = await window.analyzer.analyzeSymbol(symbol, mockData);

        // 显示分析结果
        displayAnalysisResults(symbol, result);
        showStatus('analysis-status', 'success', '✅ ' + symbol + ' 分析完成');

        // 同时计算技术指标
        calculateIndicatorsForData(mockData);

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
        // 生成模拟价格数据
        const symbol = 'DEMO';
        const mockData = generateMockData(symbol, 100);
        const prices = mockData.map(d => d.price);

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
    document.querySelector('.btn[onclick="startRealTime()"]').textContent = '停止监控';

    showStatus('realtime-status', 'loading', `🔄 开始监控 ${watchlist.length} 只股票...`);

    // 立即更新一次
    updateRealTimeData(watchlist);

    // 设置定时更新 (每5秒)
    realtimeInterval = setInterval(() => {
        updateRealTimeData(watchlist);
    }, 5000);
}

// 停止实时监控
function stopRealTime() {
    window.isRealTimeRunning = false;
    document.querySelector('.btn[onclick="startRealTime()"]').textContent = '开始实时监控';

    if (realtimeInterval) {
        clearInterval(realtimeInterval);
        realtimeInterval = null;
    }

    showStatus('realtime-status', 'success', '⏹️ 实时监控已停止');
}

// 更新实时数据
function updateRealTimeData(watchlist) {
    const container = document.getElementById('realtime-results');
    const timestamp = new Date().toLocaleTimeString();

    let html = `
        <div style="margin-top: 20px;">
            <h5>🕐 最后更新: ${timestamp}</h5>
            <div style="display: grid; gap: 12px; margin-top: 12px;">
    `;

    watchlist.forEach(symbol => {
        const mockData = generateMockData(symbol, 1);
        const currentPrice = mockData[0].price;
        const previousPrice = currentPrice + (Math.random() - 0.5) * 5;
        const change = ((currentPrice - previousPrice) / previousPrice) * 100;
        const isPositive = change >= 0;

        html += `
            <div style="display: flex; justify-content: space-between; align-items: center; padding: 12px; background: #f9fafb; border-radius: 8px;">
                <div>
                    <strong>${symbol}</strong>
                    <div style="font-size: 1.2rem; margin: 4px 0;">$${currentPrice.toFixed(2)}</div>
                </div>
                <div style="text-align: right; color: ${isPositive ? '#10b981' : '#ef4444'};">
                    <div style="font-size: 1.1rem;">
                        ${isPositive ? '↑' : '↓'} ${Math.abs(change).toFixed(2)}%
                    </div>
                    <div style="font-size: 0.85rem;">
                        ${isPositive ? '+$' : '-$'}${Math.abs(currentPrice - previousPrice).toFixed(2)}
                    </div>
                </div>
            </div>
        `;
    });

    html += '</div></div>';
    container.innerHTML = html;
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

    // 生成一些示例数据
    const prices = [];
    let basePrice = 100;
    for (let i = 0; i < 50; i++) {
        basePrice += (Math.random() - 0.5) * 2;
        prices.push(basePrice);
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
    if (realtimeInterval) {
        clearInterval(realtimeInterval);
    }
});