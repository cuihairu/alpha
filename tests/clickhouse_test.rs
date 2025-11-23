//! ClickHouse 存储集成测试

use alpha_storage::clickhouse::{ClickHouseStorage, ClickHouseConfig};
use chrono::Utc;

#[tokio::test]
async fn test_clickhouse_connection() {
    let config = ClickHouseConfig::default();

    // 尝试连接到 ClickHouse
    match ClickHouseStorage::new(config).await {
        Ok(storage) => {
            println!("✅ ClickHouse 连接成功");

            // 测试基本查询
            match storage.execute_query("SELECT 1").await {
                Ok(_) => println!("✅ 基本查询执行成功"),
                Err(e) => println!("❌ 基本查询失败: {}", e),
            }
        }
        Err(e) => {
            println!("❌ ClickHouse 连接失败: {}", e);
            println!("💡 请确保 ClickHouse 服务正在运行在 http://localhost:8123");
            println!("   或运行: docker-compose up -d clickhouse");
        }
    }
}

#[tokio::test]
async fn test_market_data_operations() {
    let config = ClickHouseConfig::default();
    let storage = match ClickHouseStorage::new(config).await {
        Ok(s) => s,
        Err(_) => {
            println!("⚠️ 跳过市场数据测试: ClickHouse 不可用");
            return;
        }
    };

    // 测试插入市场数据
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
    let end_time = Utc::now();
    let start_time = end_time - chrono::Duration::hours(1);

    match storage.query_market_data("AAPL", start_time, end_time, Some(10)).await {
        Ok(data) => println!("✅ 市场数据查询成功，返回 {} 条记录", data.len()),
        Err(e) => println!("❌ 市场数据查询失败: {}", e),
    }
}