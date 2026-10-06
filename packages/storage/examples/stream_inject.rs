//! e2e 数据面注入助手：往指定 stream 发一条与生产 publish 完全同构的
//! 行情 envelope（字段组装走 [`RedisStreamQueue::publish`]，与手拼 JSON
//! 的差异是字段/哈希与生产路径同一出处）。
//!
//! 用法：`cargo run -p alpha-storage --example stream_inject -- \
//!         <redis_url> <stream> <symbol> <price>`
//!
//! scripts/check-e2e.sh 断言 3c（XADD 行情 → real-time-feed 广播 →
//! 网关 /ws 客户端收敛）的注入端。

use std::process::ExitCode;

use alpha_storage::{RedisStreamQueue, StreamEnvelope};

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let [redis_url, stream, symbol, price] = &args[1..] else {
        eprintln!("用法: stream_inject <redis_url> <stream> <symbol> <price>");
        return ExitCode::from(2);
    };

    let price: f64 = match price.parse() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("价格解析失败 ({price}): {e}");
            return ExitCode::from(2);
        }
    };

    let queue = match RedisStreamQueue::connect(redis_url) {
        Ok(q) => q,
        Err(e) => {
            eprintln!("redis 连接失败 ({redis_url}): {e}");
            return ExitCode::FAILURE;
        }
    };

    // payload 字段口径对齐 real-time-feed envelope_to_realtime：
    // symbol/price/volume 必填，change/change_percent 缺省容忍
    let envelope = StreamEnvelope::new(
        stream.clone(),
        "realtime_quote",
        "e2e",
        Some(symbol.clone()),
        serde_json::json!({
            "symbol": symbol,
            "price": price,
            "volume": 100u64,
            "change": 0.0,
            "change_percent": 0.0,
        }),
    );

    match queue.publish(stream, &envelope).await {
        Ok(id) => {
            println!("{id}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("publish 失败: {e}");
            ExitCode::FAILURE
        }
    }
}
