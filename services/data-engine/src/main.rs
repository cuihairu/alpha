//! Alpha Finance Data Engine
//!
//! 基于 DataFusion 的高性能数据处理引擎

use datafusion::arrow::record_batch::RecordBatch;
use datafusion::prelude::*;
use std::sync::Arc;

#[tokio::main]
async fn main() -> datafusion::error::Result<()> {
    // 初始化日志
    tracing_subscriber::fmt::init();

    tracing::info!("Starting Alpha Finance Data Engine");

    // 创建 DataFusion 上下文
    let ctx = SessionContext::new();

    // 注册数据源
    register_data_sources(&ctx).await?;

    // 启动 HTTP 服务
    start_http_server(ctx.clone()).await?;

    // 启动 gRPC 服务
    start_grpc_server(ctx.clone()).await?;

    Ok(())
}

/// 注册数据源
async fn register_data_sources(ctx: &SessionContext) -> datafusion::error::Result<()> {
    // 注册 TimescaleDB 作为外部表
    ctx.register_table(
        "stock_quotes",
        Arc::new(datafusion::datasource::MemTable::try_new(
            Arc::new(create_stock_quotes_schema()),
            vec![vec![]], // 初始为空
        )?),
    )?;

    // 注册 Parquet 数据文件
    ctx.register_parquet(
        "historical_data",
        "/data/historical/stock_quotes.parquet",
        ParquetReadOptions::default(),
    ).await?;

    tracing::info!("Data sources registered successfully");
    Ok(())
}

/// 创建股票行情数据的 Arrow Schema
fn create_stock_quotes_schema() -> arrow::datatypes::Schema {
    arrow::datatypes::Schema::new(vec![
        arrow::datatypes::Field::new(
            "symbol",
            arrow::datatypes::DataType::Utf8,
            false,
        ),
        arrow::datatypes::Field::new(
            "timestamp",
            arrow::datatypes::DataType::Timestamp(
                arrow::datatypes::TimeUnit::Millisecond,
                Some(Box::new(arrow::datatypes::DataType::Utf8)),
            ),
            false,
        ),
        arrow::datatypes::Field::new(
            "price",
            arrow::datatypes::DataType::Float64,
            false,
        ),
        arrow::datatypes::Field::new(
            "volume",
            arrow::datatypes::DataType::UInt64,
            false,
        ),
        arrow::datatypes::Field::new(
            "open",
            arrow::datatypes::DataType::Float64,
            true,
        ),
        arrow::datatypes::Field::new(
            "high",
            arrow::datatypes::DataType::Float64,
            true,
        ),
        arrow::datatypes::Field::new(
            "low",
            arrow::datatypes::DataType::Float64,
            true,
        ),
    ])
}

/// 启动 HTTP 服务
async fn start_http_server(ctx: SessionContext) -> datafusion::error::Result<()> {
    let app = axum::Router::new()
        .route("/health", axum::routing::get(health_check))
        .route("/query", axum::routing::post(execute_query))
        .with_state(ctx);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8081").await?;
    tracing::info!("Data Engine HTTP server listening on 0.0.0.0:8081");

    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            tracing::error!("HTTP server error: {}", e);
        }
    });

    Ok(())
}

/// 启动 gRPC 服务
async fn start_grpc_server(_ctx: SessionContext) -> datafusion::error::Result<()> {
    // gRPC 服务实现将在后续添加
    tracing::info!("gRPC service not yet implemented");
    Ok(())
}

/// 健康检查
async fn health_check() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
        "status": "healthy",
        "service": "data-engine",
        "timestamp": chrono::Utc::now(),
    }))
}

/// 执行 SQL 查询
async fn execute_query(
    axum::extract::State(ctx): axum::extract::State<SessionContext>,
    axum::Json(request): axum::Json<QueryRequest>,
) -> Result<axum::Json<QueryResponse>, axum::response::ErrorResponse> {
    let df = match ctx.sql(&request.query).await {
        Ok(df) => df,
        Err(e) => {
            tracing::error!("SQL execution error: {}", e);
            return Err(axum::response::ErrorResponse::from((
                axum::http::StatusCode::BAD_REQUEST,
                format!("SQL error: {}", e)
            )));
        }
    };

    let results = match df.collect().await {
        Ok(results) => results,
        Err(e) => {
            tracing::error!("Result collection error: {}", e);
            return Err(axum::response::ErrorResponse::from((
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("Execution error: {}", e)
            )));
        }
    };

    let response = QueryResponse {
        success: true,
        row_count: results.iter().map(|batch| batch.num_rows()).sum(),
        data: results_to_json(&results)?,
        execution_time_ms: 0, // TODO: 添加执行时间测量
    };

    Ok(axum::Json(response))
}

/// 查询请求
#[derive(serde::Deserialize)]
struct QueryRequest {
    query: String,
}

/// 查询响应
#[derive(serde::Serialize)]
struct QueryResponse {
    success: bool,
    row_count: usize,
    data: serde_json::Value,
    execution_time_ms: u64,
}

/// 将 Arrow RecordBatch 转换为 JSON
fn results_to_json(results: &[RecordBatch]) -> Result<serde_json::Value, anyhow::Error> {
    let mut rows = Vec::new();

    for batch in results {
        let num_rows = batch.num_rows();
        for row_idx in 0..num_rows {
            let mut row = serde_json::Map::new();

            for (col_idx, field) in batch.schema().fields().iter().enumerate() {
                let column = batch.column(col_idx);
                let value = arrow_to_json_value(column, row_idx)?;
                row.insert(field.name().clone(), value);
            }

            rows.push(serde_json::Value::Object(row));
        }
    }

    Ok(serde_json::Value::Array(rows))
}

/// 将 Arrow Array 转换为 JSON Value
fn arrow_to_json_value(
    array: &arrow::array::ArrayRef,
    row_idx: usize,
) -> Result<serde_json::Value, anyhow::Error> {
    use arrow::array::*;

    if array.is_null(row_idx) {
        return Ok(serde_json::Value::Null);
    }

    match array.data_type() {
        arrow::datatypes::DataType::Utf8 => {
            let string_array = array.as_any().downcast_ref::<StringArray>().unwrap();
            Ok(serde_json::Value::String(string_array.value(row_idx).to_string()))
        }
        arrow::datatypes::DataType::Float64 => {
            let float_array = array.as_any().downcast_ref::<Float64Array>().unwrap();
            Ok(serde_json::Value::Number(
                serde_json::Number::from_f64(float_array.value(row_idx)).unwrap()
            ))
        }
        arrow::datatypes::DataType::UInt64 => {
            let uint_array = array.as_any().downcast_ref::<UInt64Array>().unwrap();
            Ok(serde_json::Value::Number(
                serde_json::Number::from(uint_array.value(row_idx))
            ))
        }
        arrow::datatypes::DataType::Timestamp(arrow::datatypes::TimeUnit::Millisecond, _) => {
            let timestamp_array = array.as_any().downcast_ref::<TimestampMillisecondArray>().unwrap();
            let timestamp = timestamp_array.value(row_idx);
            let datetime = chrono::DateTime::from_timestamp_millis(timestamp)
                .ok_or_else(|| anyhow::anyhow!("Invalid timestamp"))?;
            Ok(serde_json::Value::String(datetime.to_rfc3339()))
        }
        _ => Ok(serde_json::Value::String(format!("{:?}", array.data_type()))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_session_context() {
        let ctx = SessionContext::new();

        // 测试简单查询
        let df = ctx.sql("SELECT 1 as test_column").await.unwrap();
        let results = df.collect().await.unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].num_rows(), 1);
    }
}