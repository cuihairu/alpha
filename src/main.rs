//! Alpha Finance 集成测试程序

use alpha_storage::clickhouse::{ClickHouseStorage, ClickHouseConfig};
use chrono::Utc;
use tokio;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Alpha Finance ClickHouse 集成测试");
    println!("=====================================");

    let config = ClickHouseConfig::default();

    // 尝试连接到 ClickHouse
    println!("📡 正在连接到 ClickHouse...");
    match ClickHouseStorage::new(config).await {
        Ok(storage) => {
            println!("✅ ClickHouse 连接成功");

            // 测试基本查询
            println!("🔍 测试基本查询...");
            match storage.execute_query("SELECT 1").await {
                Ok(_) => println!("✅ 基本查询执行成功"),
                Err(e) => println!("❌ 基本查询失败: {}", e),
            }

            // 测试插入市场数据
            println!("📈 测试市场数据插入...");
            let test_data = vec![
                alpha_storage::clickhouse::MarketDataInsert {
                    timestamp: Utc::now(),
                    symbol_id: 1,
                    symbol: "AAPL".to_string(),
                    open_price: 150.0,
                    high_price: 155.0,
                    low_price: 149.0,
                    close_price: 154.0,
                    adj_close_price: 154.0,
                    volume: 1000000,
                    source: "test".to_string(),
                }
            ];

            match storage.insert_market_data(test_data).await {
                Ok(_) => println!("✅ 市场数据插入成功"),
                Err(e) => println!("❌ 市场数据插入失败: {}", e),
            }

            // 测试查询市场数据
            println!("📊 测试市场数据查询...");
            let end_time = Utc::now();
            let start_time = end_time - chrono::Duration::hours(1);

            match storage.query_market_data("AAPL", start_time, end_time, Some(10)).await {
                Ok(data) => println!("✅ 市场数据查询成功，返回 {} 条记录", data.len()),
                Err(e) => println!("❌ 市场数据查询失败: {}", e),
            }

            // 测试实时报价
            println!("💰 测试实时报价查询...");
            match storage.get_realtime_quotes().await {
                Ok(quotes) => println!("✅ 实时报价查询成功，返回 {} 条记录", quotes.len()),
                Err(e) => println!("❌ 实时报价查询失败: {}", e),
            }
        }
        Err(e) => {
            println!("❌ ClickHouse 连接失败: {}", e);
            println!("\n💡 请确保 ClickHouse 服务正在运行:");
            println!("   docker-compose up -d clickhouse");
            println!("\n或者使用 brew 安装:");
            println!("   brew install clickhouse");
            println!("   clickhouse server");
        }
    }

    println!("\n🎉 集成测试完成！");
    Ok(())
}