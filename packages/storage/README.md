# alpha-storage

统一存储抽象层，提供内存、时间序列、PostgreSQL KV、Redis KV、本地磁盘 KV 和对象存储后端。

## 已实现后端

- `Memory`
  - 适合测试、短生命周期缓存、进程内元数据。
- `LocalDisk`
  - 适合单机开发环境或桌面端离线缓存。
- `Postgres`
  - 适合持久化 KV 元数据，支持 TTL 字段。
- `Redis`
  - 适合热点缓存、锁、令牌桶、短 TTL 数据。
- `S3`
  - 当前实现为兼容简单 HTTP 对象接口的最小可用对象存储层，适合 MinIO 或自控兼容网关。

## `StorageFactory` 用法

```rust
use alpha_storage::{StorageBackendType, StorageConfig, StorageFactory};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = StorageConfig {
        backend: StorageBackendType::LocalDisk,
        connection_string: "file:///tmp/alpha-storage".to_string(),
        ttl_seconds: None,
        max_connections: None,
    };

    let storage = StorageFactory::create(config).await?;
    storage.store("quotes/AAPL", b"150.25".to_vec()).await?;

    Ok(())
}
```

## 连接串约定

### `Memory`

```text
memory://
```

`connection_string` 当前不参与连接，仅用于统一配置结构。

### `LocalDisk`

支持两种格式：

```text
file:///var/lib/alpha/storage
disk:///var/lib/alpha/storage
```

也支持裸路径：

```text
/var/lib/alpha/storage
```

键会映射为目录路径，最后一段以 `.bin` 结尾保存。路径段会做 URL 编码，避免特殊字符破坏目录结构。

### `Postgres`

```text
postgresql://postgres:password@localhost:5432/alpha_finance
```

说明：

- `StorageFactory` 默认使用表名 `alpha_kv`
- 表会自动创建
- `ttl_seconds` 会写入 `expires_at`
- `max_connections` 会传给 `sqlx::PgPoolOptions`

### `Redis`

```text
redis://localhost:6379
redis://localhost:6379/0
```

说明：

- `ttl_seconds` 会在 `store` 时转成 `SETEX`
- `list_keys` 基于 `SCAN MATCH`
- `clear` 会执行 `FLUSHDB`，只适合独立 DB 或测试环境

### `S3` / 对象存储

当前连接串格式：

```text
s3://alpha-bucket?provider=minio&endpoint=http%3A%2F%2F127.0.0.1%3A9000&region=us-east-1&access_key=minioadmin&secret_key=minioadmin
```

支持参数：

- `provider`
  - 可选值：`aws`、`s3`、`alibaba`、`oss`、`tencent`、`cos`、`minio`
- `endpoint`
  - 对象存储 HTTP 入口
- `region`
  - 区域名
- `access_key`
  - 访问凭证
- `secret_key`
  - 密钥

实现约束：

- 当前并未接入官方 AWS SDK
- 默认依赖简单 HTTP 对象接口：`PUT/GET/HEAD/DELETE {endpoint}/{bucket}/{key}`
- `list_keys` 和 `clear` 通过维护 `alpha/_index.json` 索引对象实现

## `DataAccessLayer`

`DataAccessLayer` 提供更高层的组合接口：

- 市场数据写入内存时间序列
- 最新价格走进程内缓存
- 元数据通过可配置 `StorageConfig` 交给 `StorageFactory`
- 支持导入导出和简单查询构建器

默认配置：

```rust
use alpha_storage::DataAccessConfig;

let config = DataAccessConfig::default();
```

默认元数据后端为 `Memory`。如果需要切换到磁盘或数据库，需要显式传入 `metadata_storage: StorageConfig`。

## 测试

```bash
cargo test -p alpha-storage
```

可选集成测试环境变量：

- `REDIS_TEST_URL`
- `POSTGRES_TEST_URL`
- `TIMESCALE_TEST_URL`
