// Alpha Finance 演示数据生成器

class DemoDataGenerator {
    constructor() {
        this.stocks = [
            'AAPL', 'GOOGL', 'MSFT', 'AMZN', 'TSLA', 'META', 'NVDA', 'JPM',
            'JNJ', 'V', 'PG', 'UNH', 'HD', 'MA', 'BAC', 'XOM', 'CVX',
            'LLY', 'ABBV', 'PFE', 'T', 'CRM', 'ACN', 'MRK', 'COST',
            'CMCSA', 'LIN', 'NKE', 'DIS', 'WMT', 'NFLX', 'ADBE', 'PYPL',
            'INTC', 'CSCO', 'PEP', 'KO', 'MDT', 'HON', 'TXN', 'NEE',
            'DHR', 'QCOM', 'UPS', 'TMO', 'ABT', 'CVS', 'D', 'AMGN'
        ];

        this.priceRanges = {
            'AAPL': { min: 120, max: 200 },
            'GOOGL': { min: 80, max: 150 },
            'MSFT': { min: 200, max: 400 },
            'AMZN': { min: 80, max: 180 },
            'TSLA': { min: 150, max: 300 },
            'META': { min: 200, max: 400 },
            'NVDA': { min: 200, max: 600 },
            'JPM': { min: 100, max: 170 },
            'default': { min: 50, max: 200 }
        };
    }

    // 生成随机价格数据
    generatePriceData(symbol, days = 252, startPrice = null) {
        const range = this.priceRanges[symbol] || this.priceRanges['default'];

        // 如果没有指定起始价格，使用范围的中间值
        if (startPrice === null) {
            startPrice = (range.min + range.max) / 2;
        }

        const data = [];
        let currentPrice = startPrice;
        const volatility = 0.02; // 2% 日波动率

        for (let i = 0; i < days; i++) {
            const date = new Date();
            date.setDate(date.getDate() - (days - i - 1));

            // 生成随机价格变动
            const randomChange = (Math.random() - 0.5) * 2 * volatility;
            currentPrice = currentPrice * (1 + randomChange);

            // 确保价格在合理范围内
            currentPrice = Math.max(range.min, Math.min(range.max, currentPrice));

            // 生成 OHLC 数据
            const dailyVolatility = currentPrice * 0.03 * Math.random();
            const open = currentPrice + (Math.random() - 0.5) * dailyVolatility;
            const high = Math.max(currentPrice, open) + Math.random() * dailyVolatility;
            const low = Math.min(currentPrice, open) - Math.random() * dailyVolatility;
            const close = currentPrice;

            // 生成成交量 (基于市值)
            const baseVolume = Math.random() * 1000000 + 500000;
            const volume = Math.floor(baseVolume * (1 + (Math.random() - 0.5) * 0.5));

            data.push({
                symbol: symbol,
                timestamp: date.toISOString(),
                price: parseFloat(close.toFixed(2)),
                volume: volume,
                bid: parseFloat((close - 0.05).toFixed(2)),
                ask: parseFloat((close + 0.05).toFixed(2)),
                open: parseFloat(open.toFixed(2)),
                high: parseFloat(high.toFixed(2)),
                low: parseFloat(low.toFixed(2))
            });
        }

        return data;
    }

    // 生成实时数据快照
    generateRealTimeData(symbol, previousPrice = null) {
        const range = this.priceRanges[symbol] || this.priceRanges['default'];

        if (previousPrice === null) {
            previousPrice = (range.min + range.max) / 2;
        }

        const changePercent = (Math.random() - 0.5) * 10; // -5% 到 +5%
        const currentPrice = previousPrice * (1 + changePercent / 100);

        return {
            symbol: symbol,
            price: parseFloat(currentPrice.toFixed(2)),
            change: parseFloat((currentPrice - previousPrice).toFixed(2)),
            changePercent: parseFloat(changePercent.toFixed(2)),
            volume: Math.floor(Math.random() * 1000000) + 100000,
            timestamp: new Date().toISOString()
        };
    }

    // 生成市场新闻 (模拟)
    generateMarketNews(count = 5) {
        const newsTemplates = [
            "{symbol} 股价大涨 {percent}%，分析师看好后市",
            "{symbol} 发布强劲财报，营收增长 {growth}%",
            "市场分析师上调 {symbol} 目标价至 ${price}",
            "{symbol} 宣布新产品计划，投资者反应积极",
            "宏观经济因素影响 {symbol} 股价表现",
            "{symbol} CEO 发表乐观展望，股价应声上涨",
            "行业趋势利好 {symbol}，机构投资者增持",
            "{symbol} 技术突破创新高，技术面看涨",
        ];

        const news = [];
        for (let i = 0; i < count; i++) {
            const symbol = this.stocks[Math.floor(Math.random() * this.stocks.length)];
            const template = newsTemplates[Math.floor(Math.random() * newsTemplates.length)];
            const percent = (Math.random() * 5 + 1).toFixed(1);
            const growth = (Math.random() * 20 + 5).toFixed(1);
            const price = (Math.random() * 100 + 100).toFixed(2);

            news.push({
                title: template
                    .replace('{symbol}', symbol)
                    .replace('{percent}', percent)
                    .replace('{growth}', growth)
                    .replace('{price}', price),
                symbol: symbol,
                timestamp: new Date(Date.now() - Math.random() * 86400000).toISOString(),
                sentiment: Math.random() > 0.5 ? 'positive' : 'neutral'
            });
        }

        return news.sort((a, b) => new Date(b.timestamp) - new Date(a.timestamp));
    }

    // 生成随机股票组合
    generatePortfolio(size = 5) {
        const selectedStocks = [];
        const usedStocks = new Set();

        while (selectedStocks.length < size && selectedStocks.length < this.stocks.length) {
            const stock = this.stocks[Math.floor(Math.random() * this.stocks.length)];
            if (!usedStocks.has(stock)) {
                usedStocks.add(stock);
                selectedStocks.push(stock);
            }
        }

        return selectedStocks;
    }

    // 生成指数数据
    generateIndexData(indexName, days = 252) {
        const indexRanges = {
            'S&P 500': { min: 3500, max: 5000 },
            'NASDAQ': { min: 12000, max: 18000 },
            'DOW JONES': { min: 30000, max: 40000 },
            'CSI 300': { min: 3500, max: 4500 }
        };

        const range = indexRanges[indexName] || indexRanges['S&P 500'];
        const data = this.generatePriceData(indexName, days, (range.min + range.max) / 2);

        return data.map(item => ({
            ...item,
            symbol: indexName,
            type: 'index'
        }));
    }
}

// 全局实例
window.demoDataGenerator = new DemoDataGenerator();

// 暴露便捷函数
window.generateMockData = (symbol, days) => {
    return window.demoDataGenerator.generatePriceData(symbol, days);
};

window.generateRealTimeData = (symbol, previousPrice) => {
    return window.demoDataGenerator.generateRealTimeData(symbol, previousPrice);
};

window.generateMarketNews = (count) => {
    return window.demoDataGenerator.generateMarketNews(count);
};

window.generatePortfolio = (size) => {
    return window.demoDataGenerator.generatePortfolio(size);
};