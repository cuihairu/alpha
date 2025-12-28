//! Alpha Collector 使用示例
//!
//! 展示如何使用 A 股数据爬虫服务

use alpha_collector::prelude::*;
use std::sync::Arc;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 初始化日志
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("=== Alpha Collector A股数据爬虫示例 ===\n");

    // 1. 创建数据源
    println!("1. 初始化数据源...");
    let config = CrawlerConfig::default();
    let sources: Vec<Arc<dyn DataSource>> = vec![
        Arc::new(EastmoneySource::new(config.clone())),
        Arc::new(SinaSource::new(config.clone())),
        Arc::new(TencentSource::new(config.clone())),
    ];
    println!("✓ 已初始化 {} 个数据源\n", sources.len());

    // 2. 获取单个股票实时行情
    println!("2. 获取单个股票实时行情...");
    match sources[0].get_realtime_quote("000001").await {
        Ok(quote) => {
            println!("✓ 股票: {} ({})", quote.name, quote.symbol);
            println!("  价格: {:.2}元", quote.price);
            println!("  涨跌幅: {:.2}%", quote.change_percent);
            println!("  成交量: {}手", quote.volume);
        }
        Err(e) => println!("✗ 获取失败: {}", e),
    }
    println!();

    // 3. 批量获取股票行情
    println!("3. 批量获取股票行情...");
    let symbols = vec!["000001".to_string(), "600000".to_string(), "600519".to_string()];
    match sources[0].get_realtime_quotes(&symbols).await {
        Ok(quotes) => {
            println!("✓ 获取到 {} 只股票行情:", quotes.len());
            for quote in &quotes {
                println!("  {} ({}): {:.2}元 ({:.2}%)",
                    quote.name, quote.symbol, quote.price, quote.change_percent);
            }
        }
        Err(e) => println!("✗ 获取失败: {}", e),
    }
    println!();

    // 4. 获取 K线数据
    println!("4. 获取日K线数据...");
    match sources[0].get_kline("000001", KlineType::Day, 5).await {
        Ok(klines) => {
            println!("✓ 获取到 {} 条K线数据:", klines.len());
            for kline in &klines {
                println!("  {}: 开盘={:.2}, 最高={:.2}, 最低={:.2}, 收盘={:.2}",
                    kline.timestamp, kline.open, kline.high, kline.low, kline.close);
            }
        }
        Err(e) => println!("✗ 获取失败: {}", e),
    }
    println!();

    // 5. 使用数据清洗器
    println!("5. 数据清洗示例...");
    let mut cleaner = DataCleaner::with_default_rules();

    let test_quote = RealtimeQuote {
        symbol: "sh600000".to_string(),
        name: "浦发银行".to_string(),
        price: 10.50,
        pre_close: 10.00,
        open: 10.20,
        high: 10.80,
        low: 10.10,
        volume: 1000000,
        amount: 10800000.0,
        change: 0.0,
        change_percent: 0.0,
        bid1: Some(10.50),
        ask1: Some(10.51),
        bid1_volume: Some(1000),
        ask1_volume: Some(1000),
        timestamp: chrono::Utc::now(),
        source: "test".to_string(),
    };

    let clean_result = cleaner.clean_realtime_quote(test_quote);
    println!("✓ 清洗结果: {:?}", clean_result.quality);
    println!("  数据有效性: {}", clean_result.is_valid());
    if !clean_result.warnings.is_empty() {
        println!("  警告: {:?}", clean_result.warnings);
    }
    println!();

    // 6. 使用限流器
    println!("6. 限流器示例...");
    let rate_limiter = TokenBucketRateLimiter::new(10, 5); // 容量10，每秒补充5个
    println!("✓ 令牌桶限流器创建成功");
    println!("  当前可用令牌: {}", rate_limiter.available_tokens().await);
    println!("  获取5个令牌...");
    rate_limiter.acquire(5).await;
    println!("  剩余令牌: {}", rate_limiter.available_tokens().await);
    println!();

    // 7. 使用调度器
    println!("7. 调度器示例...");
    let scheduler_config = SourceSchedulerConfig {
        max_concurrent_tasks: 2,
        enable_cleaning: true,
        ..Default::default()
    };
    let scheduler = Arc::new(SourceScheduler::new(
        scheduler_config.clone(),
        sources.clone(),
        cleaner,
    ));

    // 提交任务
    let task = SourceTaskGenerator::realtime_quotes(
        vec!["000001".to_string(), "600000".to_string()],
        SourceTaskPriority::High,
    );
    scheduler.submit_task(task).await?;
    println!("✓ 任务已提交到调度器");
    println!("  当前队列长度: {}", scheduler.queue_length().await);

    // 获取统计信息
    let stats = scheduler.get_stats().await;
    println!("  总任务数: {}", stats.total_tasks);
    println!();

    // 8. 监控指标示例
    println!("8. 监控指标示例...");
    let metrics = Arc::new(CollectorMetrics::default());

    // 记录一些指标
    let timer = RequestTimer::start(metrics.clone(), "eastmoney", "realtime_quote");
    tokio::time::sleep(Duration::from_millis(100)).await;
    timer.succeed();

    metrics.record_data_points("eastmoney", "quote", 2);

    // 导出 Prometheus 格式
    match metrics.export_prometheus() {
        Ok(prometheus_text) => {
            println!("✓ Prometheus 指标:");
            for line in prometheus_text.lines().take(10) {
                println!("  {}", line);
            }
        }
        Err(e) => println!("✗ 导出失败: {}", e),
    }
    println!();

    // 9. 健康检查
    println!("9. 健康检查示例...");
    let health_checker = HealthChecker::new(metrics.clone());
    health_checker.register_checker("test_source", move || {
        ComponentHealth::healthy("test_source")
    }).await;

    let health_result = health_checker.check().await;
    println!("✓ 健康状态: {:?}", health_result.status);
    println!();

    // 10. 股票代码标准化
    println!("10. 股票代码标准化示例...");
    let code1 = SymbolNormalizer::normalize("600000");
    let code2 = SymbolNormalizer::normalize("sh000001");
    let code3 = SymbolNormalizer::extract_code("sz000001");
    println!("  600000 -> {}", code1);
    println!("  sh000001 -> {}", code2);
    println!("  sz000001 -> {}", code3);
    println!();

    println!("=== 示例完成 ===");

    // 启动调度器（这会一直运行）
    // tokio::spawn(async move {
    //     scheduler.start().await;
    // });

    // 等待一段时间让任务执行
    tokio::time::sleep(Duration::from_secs(2)).await;

    Ok(())
}
