# Alpha Finance 部署文档

## 📋 目录

- [快速开始](#快速开始)
- [系统架构](#系统架构)
- [服务部署](#服务部署)
- [环境配置](#环境配置)
- [监控与维护](#监控与维护)
- [故障排除](#故障排除)
- [性能优化](#性能优化)

---

## 快速开始

### 🚀 一键部署

```bash
# 克隆项目
git clone https://github.com/your-org/alpha-finance.git
cd alpha-finance

# 安装依赖
make setup

# 启动服务
make dev

# 停止服务
make stop

# 重启服务
make restart
```

### 📋 前置要求

- **系统要求**
  - **操作系统**: Ubuntu 20.04+ / CentOS 8+ / macOS 12+ / Windows 10+
  - **内存**: 最低 4GB，推荐 8GB+
  - **磁盘**: 最低 20GB，推荐 SSD
  - **CPU**: 最低 4 核，推荐 8+ 核心
  - **网络**: 稳定的互联网连接

- **软件依赖**
  ```bash
  # Docker & Docker Compose (推荐)
  docker --version
  docker-compose --version

  # Node.js 16+ (如果使用前端开发)
  node --version

  # Rust 1.70+
  rustc --version

  # 其他构建工具
  make --version
  ```

- **可选数据库**
  - PostgreSQL 13+ (推荐用于生产环境)
  - TimescaleDB (用于时序数据)
  - ClickHouse (用于分析型查询)
  - Redis (用于缓存和会话)

### 🔧 构建步骤

```bash
# 1. 克隆仓库
git clone https://github.com/your-org/alpha-finance.git
cd alpha-finance

# 2. 配置环境
cp .env.example .env
# 编辑 .env 文件，配置数据库连接等

# 3. 构建项目
cargo build --release

# 4. 构建并启动 WebAssembly 模块
./build-wasm-optimized.sh

# 5. 构建并启动服务
make build-all

# 6. 启动开发环境
make dev
```

---

## 系统架构

### 🏗️ 整体架构

```
┌─────────────────────────────────────────────────────────┐
│                   ┌─────────┐                ┌─────────┐        │
│                   │  Web Frontend   │                │  WebAssembly │
│                   │  (React/Vue)   │                │   (Rust/Wasm) │
│                   └─────────────┘                └─────────────┘        │
│                                                            │
┌─────────────────────────────────────────────────────────┐
│              ┌───────────┐           ┌─────────┐             │
│              │ Load Balancer │           │   Gateway  │             │
│              │ (Nginx/HAProxy)│           │  (Nginx)   │             │
│              └─────────────┘           └─────────────┘             │
└─────────────────────────────────────────────────────────┘                            │
                                                            │
┌─────────────────────────────────────────────────────────┐
│  ┌─────────────┐       ┌─────────────┐      ┌─────────────┐       │
│  │ Collector    │       │ Data Engine  │       │ Real-time Feed │       │
│  │ (Multiple)   │       │ (Go Service) │       │ (Go Service)    │
│  │              │       └─────────────┘       └─────────────┘       │
│  └─────────────┘       ┌─────────────┐      ┌─────────────┐       │
│                    │ Scheduler  │      │ Task Queue  │       │ WebSocket    │
│                    │ (Go Service) │       │ (Redis/NATS)  │       │ (Go Service)    │
│                    └─────────────┘       └─────────────┘       └─────────────┘
└─────────────────────────────────────────────────────────┘                             │
                                                            │
┌─────────────────────────────────────────────────────────┐
│              ┌───────────────────┐    ┌─────────────┐    ┌─────────────┐   │
│              │    Storage Layer      │    │ Storage Layer  │    │ Storage Layer  │
│              │ (Multiple)           │    │ (PostgreSQL)   │    │ (ClickHouse)  │
│              │                      │    │ + TimescaleDB │    │ + Redis Cache   │
│              └───────────────────┘    └─────────────┘    └─────────────┘
└─────────────────────────────────────────────────────────┘                             │
                                                            │
┌─────────────────────────────────────────────────────────┐
│              ┌───────────────────┐       ┌─────────────┐    ┌─────────────┐   │
│              │   Message Queue     │       │ Message Queue │       │ Message Queue │
│              │ (Kafka/Redis)     │       │ (Kafka)     │       │ (Redis/NATS) │
│              └───────────────────┘       └─────────────┘       └─────────────┘
└─────────────────────────────────────────────────────────┘                             │
                                                            │
┌─────────────────────────────────────────────────────────┐
│                   ┌─────────┐         ┌─────────────┐        │
│                   │ Monitoring │         │  Metrics     │         │   Metrics     │
│                   │ (Prometheus)    │         │ (Prometheus) │         │ (Prometheus) │
│                   └─────────────┘         └─────────────┘         └─────────────┘
└─────────────────────────────────────────────────────────┘                             │
                                                            │
```

---

## 服务部署

### 🚢 生产环境部署

#### 1. **Collector Service** (数据收集服务)

```bash
# 环境变量
export RUST_LOG=info
export DATABASE_URL=postgresql://user:password@localhost:5432/alpha_finance
export REDIS_URL=redis://localhost:6379/0
export KAFKA_BROKERS=localhost:9092
export COLLECTOR_CONFIG=/path/to/collector/config.toml

# 启动服务
cd services/collector
cargo run --release
```

**配置文件示例** (`config/collector.toml`):
```toml
[server]
host = "0.0.0.0"
port = 8080

[database]
url = "postgresql://alpha:password@localhost:5432/alpha_finance"
max_connections = 20
connection_timeout = 30

[redis]
url = "redis://localhost:6379/1"
pool_size = 10
connection_timeout = 5

[kafka]
brokers = ["localhost:9092"]
topics = ["market-data", "news-data", "task-results"]
producer_config = { batch_size = 1000 }

[crawlers]
max_concurrent_tasks = 50
default_timeout = 300
retry_policy = { max_retries = 3, backoff_strategy = "exponential" }

[monitoring]
metrics_port = 9090
health_check_interval = 30
log_level = "info"
```

#### 2. **Data Engine Service** (数据处理服务)

```bash
# 启动数据处理服务
cd services/data-engine
cargo run --release --config /path/to/data-engine/config.toml
```

**配置文件示例**:
```toml
[server]
host = "0.0.0.0"
port = 8081

[database]
url = "postgresql://alpha:password@localhost:5432/alpha_finance"
query_timeout = 30
connection_pool_size = 10

[processing]
batch_size = 1000
memory_limit = "2GB"
worker_threads = 8
```

#### 3. **Real-time Feed Service** (实时数据流服务)

```bash
# 启动实时数据流服务
cd services/real-time-feed
cargo run --release --config /path/to/real-time-feed/config.toml
```

**配置文件示例**:
```toml
[websocket]
host = "0.0.0.0"
port = 8082
path = "/ws"
compression = true

[redis]
url = "redis://localhost:6379/2"
pub_channel = "real-time-updates"
sub_channel = "market-data"

[processors]
tick_interval = 1000
max_connections = 10000
buffer_size = 8192
```

#### 4. **API Gateway Service** (API网关服务)

```bash
# 启动API网关
cd services/api-gateway
cargo run --release --config /path/to/api-gateway/config.toml
```

**配置文件示例**:
```toml
[server]
host = "0.0.0.0"
port = 8080
workers = 4

[database]
url = "postgresql://alpha:password@localhost:5432/alpha_finance"
connection_pool_size = 20

[rate_limiting]
requests_per_minute = 1000
burst_size = 100
window_size = 60000

[auth]
api_keys = ["your-api-key-here"]
jwt_secret = "your-jwt-secret-here"

[monitoring]
metrics_port = 9091
health_check_interval = 60
```

### 🌐 负载均衡配置

#### **Nginx 配置**
```nginx
upstream alpha_backend {
    least_conn;
    server 127.0.0.1:8080;
    server 127.0.0.1:8081;
    server 127.0.0.1:8082;
    server 127.0.0.1:8083;
}

server {
    listen 80;
    server_name api.alpha.finance;

    location / {
        proxy_pass http://alpha_backend;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;

        # 健康检查
        location /health {
            access_log off;
            return 200 "healthy";
            add_header Content-Type text/plain;
            add_header Cache-Control no-cache;
        }

        # API 路由
        location /api/ {
            proxy_pass http://alpha_backend;
            proxy_set_header Host $host;
            proxy_set_header X-Real-IP $remote_addr;
            proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;

            # CORS 头
            add_header Access-Control-Allow-Origin *;
            add_header Access-Control-Allow-Methods GET,POST,PUT,DELETE,OPTIONS;
            add_header Access-Control-Allow-Headers Content-Type,Authorization;
            add_header Access-Control-Allow-Credentials true;
        }

        # WebSocket 代理
        location /ws {
            proxy_pass http://alpha_backend;
            proxy_http_version 1.1;
            proxy_set_header Upgrade $http_upgrade;
            proxy_set_header Connection $connection_upgrade;
            proxy_set_header Host $host;
        }
    }
}
```

#### **Docker Compose 配置**
```yaml
version: '3.8'

services:
  # 数据库服务
  postgres:
    image: postgres:15
    environment:
      POSTGRES_DB: alpha_finance
      POSTGRES_USER: alpha
      POSTGRES_PASSWORD: ${POSTGRES_PASSWORD}
    volumes:
      - postgres_data:/var/lib/postgresql/data
      - ./config/postgres/init.sql:/docker-entrypoint-initdb.d
    networks:
      - alpha-network

  # Redis 缓存
  redis:
    image: redis:7-alpine
    command: redis-server --appendonly yes --requirepass ${REDIS_PASSWORD}
    volumes:
      - redis_data:/data
    networks:
      - alpha-network

  # Kafka 消息队列
  zookeeper:
    image: confluentinc/cp-zookeeper:7.4.0
    environment:
      ZOOKEEPER_CLIENT_PORT: 2181
      ZOOKEEPER_TICK_TIME: 2000
    networks:
      - alpha-network

  kafka:
    image: confluentinc/cp-kafka:7.4.0
    depends_on:
      - zookeeper
    environment:
      KAFKA_BROKER_ID: 1
      KAFKA_ZOOKEEPER_CONNECT: zookeeper:2181
      KAFKA_ADVERTISED_LISTENERS: PLAINTEXT://9092
      KAFKA_OFFSETS_TOPIC_REPLICATION_FACTOR: 1
      KAFKA_AUTO_CREATE_TOPICS_ENABLE: true
    networks:
      - alpha-network

  # 应用服务
  collector:
    build: ./services/collector
    environment:
      - DATABASE_URL=postgresql://alpha:password@postgres:5432/alpha_finance
      - REDIS_URL=redis://redis:6379/0
      - KAFKA_BROKERS=localhost:9092
    depends_on:
      - postgres
      - redis
      - kafka
    networks:
      - alpha-network
    ports:
      - "8080:8080"

  data-engine:
    build: ./services/data-engine
    environment:
      - DATABASE_URL=postgresql://alpha:password@postgres:5432/alpha_finance
    depends_on:
      - postgres
    networks:
      - alpha-network

  real-time-feed:
    build: ./services/real-time-feed
    environment:
      - REDIS_URL=redis://redis:6379/1
      - KAFKA_BROKERS=localhost:9092
    depends_on:
      - redis
      - kafka
    networks:
      - alpha-network
    ports:
      - "8081:8081"

  api-gateway:
    build: ./services/api-gateway
    environment:
      - DATABASE_URL=postgresql://alpha:password@postgres:5432/alpha_finance
      - REDIS_URL=redis://redis:6379/2
    depends_on:
      - postgres
      - redis
    networks:
      - alpha-network
    ports:
      - "8080:8080"

  # 负载均衡器
  nginx:
    image: nginx:alpine
    ports:
      - "80:80"
    volumes:
      - ./config/nginx/nginx.conf:/etc/nginx/nginx.conf
    depends_on:
      - collector
      - data-engine
      - real-time-feed
      - api-gateway
    networks:
      - alpha-network

networks:
  alpha-network:
    driver: bridge
```

---

## 环境配置

### 🔧 环境变量配置

```bash
# .env 文件示例
# 数据库配置
DATABASE_URL=postgresql://alpha:secure_password@localhost:5432/alpha_finance
DATABASE_POOL_SIZE=20
DATABASE_QUERY_TIMEOUT=30

# Redis 配置
REDIS_URL=redis://localhost:6379/0
REDIS_POOL_SIZE=10
REDIS_CONNECTION_TIMEOUT=5

# Kafka 配置
KAFKA_BROKERS=localhost:9092
KAFKA_TOPIC_PREFIX=alpha-finance
KAFKA_BATCH_SIZE=1000
KAFKA_PRODUCER_CONFIG={}

# 服务配置
COLLECTOR_HOST=0.0.0.0
COLLECTOR_PORT=8080
DATA_ENGINE_HOST=0.0.0.0
DATA_ENGINE_PORT=8081
REAL_TIME_FEED_HOST=0.0.0.0
REAL_TIME_FEED_PORT=8081
API_GATEWAY_HOST=0.0.0.0
API_GATEWAY_PORT=8080

# 调度和日志
RUST_LOG=info
RUST_LOG_STYLE=json
RUST_BACKTRACE=1

# 性能配置
TOKIO_WORKER_THREADS=8
COLLECTOR_MAX_CONCURRENT=50
DATA_ENGINE_MEMORY_LIMIT=2GB
REAL_TIME_FEED_MAX_CONNECTIONS=10000
```

### 🔐 安全配置

#### 1. **数据库安全**
```sql
-- 创建专用用户
CREATE USER alpha_user WITH PASSWORD 'secure_password_123';
CREATE DATABASE alpha_finance OWNER alpha_user;

-- 授权用户访问
GRANT ALL PRIVILEGES ON DATABASE alpha_finance TO alpha_user;
GRANT ALL PRIVILEGES ON ALL TABLES IN SCHEMA public TO alpha_user;

-- 启用行级安全
ALTER DATABASE alpha_finance SET row_level_security = on;
```

#### 2. **API 安全**
```toml
[security]
# 启用 API 密钥认证
api_key_required = true

# 速率限制
rate_limit_per_minute = 1000
rate_limit_burst = 100

# CORS 配置
cors_origins = ["https://app.alpha.finance", "https://admin.alpha.finance"]
cors_methods = ["GET", "POST", "PUT", "DELETE"]
cors_headers = ["Content-Type", "Authorization", "X-API-Key"]

# JWT 配置
jwt_secret = "your-256-bit-secret-here"
jwt_expiration = 86400  # 24小时
jwt_refresh_window = 3600  # 1小时
```

#### 3. **网络安全**
```bash
# 防火墙配置 (ufw)
sudo ufw enable 80/tcp    # HTTP
sudo ufw enable 443/tcp   # HTTPS
sudo ufw enable 9092/tcp  # Kafka
sudo ufw enable 5432/tcp   # PostgreSQL
sudo ufw enable 6379/tcp   # Redis

# 反向代理配置
# 在 nginx.conf 中配置真实 IP 检查
set_real_ip_from 192.168.1.0/24;
set_real_ip_header X-Real-IP;
```

---

## 监控与维护

### 📊 监控指标

#### 1. **应用指标**
```yaml
# Prometheus 配置
global:
  scrape_interval: 15s
  evaluation_interval: 15s

scrape_configs:
  - job_name: 'alpha-collector'
    static_configs:
      - targets: ['collector:8080/metrics']
    metrics_path: '/metrics'
    scrape_interval: 5s

  - job_name: 'alpha-data-engine'
    static_configs:
      - targets: ['data-engine:8080/metrics']
    metrics_path: '/metrics'
    scrape_interval: 5s

  - job_name: 'alpha-real-time-feed'
    static_configs:
      - targets: ['real-time-feed:8081/metrics']
    metrics_path: '/metrics'
    scrape_interval: 5s

  - job_name: 'alpha-api-gateway'
    static_configs:
      - targets: ['api-gateway:8080/metrics']
    metrics_path: '/metrics'
    scrape_interval: 5s
```

#### 2. **Grafana 仪表板**
```json
{
  "datasources": [
    {
      "name": "Alpha Finance",
      "type": "prometheus",
      "url": "http://prometheus:9090",
      "access": "proxy",
      "isDefault": true
    }
  ],
  "dashboard": {
    "title": "Alpha Finance 监控",
    "panels": [
      {
        "title": "任务执行统计",
        "type": "stat",
        "targets": [
          {
            "expr": "sum(alpha_tasks_total)",
            "legendFormat": "总任务数"
          }
        ]
      },
      {
        "title": "API 请求量",
        "type": "graph",
        "targets": [
          {
            "expr": "rate(alpha_api_requests_total[5m])",
            "legendFormat": "每5分钟请求数"
          }
        ]
      }
    ]
  }
}
```

#### 3. **日志聚合**
```yaml
# Loki 配置
auth_enabled: false
server:
  http_listen_port: 3100
  grpc_listen_port: 9096

positions:
  filename: /tmp/positions.yaml
  target_config: config/loki-local-config.yaml

clients:
  - url: http://loki:3100/loki/api/v1/push
    batchsize: 400
    external_labels:
      job: "alpha-finance"
      instance: "prod-1"
```

### 🛠️ 运维脚本

#### 1. **健康检查脚本**
```bash
#!/bin/bash

# Alpha Finance 健康检查脚本
# 检查所有服务的健康状态

SERVICES=("collector:8080" "data-engine:8081" "real-time-feed:8081" "api-gateway:8080")

ALL_HEALTHY=true
FAILED_SERVICES=()

for service in "${SERVICES[@]}"; do
    name="${service%:*}"
    port="${service#*:}"

    echo "检查 $name (端口 $port)..."

    if curl -f -s "http://localhost:$port/health" --max-time 10 >/dev/null 2>&1; then
        echo "✅ $name 健康"
    else
        echo "❌ $name 不健康"
        ALL_HEALTHY=false
        FAILED_SERVICES+=("$name")
    fi
done

echo ""
if [ "$ALL_HEALTHY" = true ]; then
    echo "🎉 所有服务运行正常"
    exit 0
else
    echo "💥 以下服务不健康: $FAILED_SERVICES"
    exit 1
fi
```

#### 2. **日志轮转脚本**
```bash
#!/bin/bash

# 日志轮转脚本
# 定期压缩和归档日志文件

LOG_DIR="/var/log/alpha-finance"
ARCHIVE_DIR="/var/log/alpha-finance/archive"
RETENTION_DAYS=30

# 创建归档目录
mkdir -p "$ARCHIVE_DIR"

# 压缩旧日志
find "$LOG_DIR" -name "*.log" -mtime +$RETENTION_DAYS -exec gzip -v {} \;

# 移动压缩后的日志
find "$LOG_DIR" -name "*.gz" -mtime +$((RETENTION_DAYS + 90)) -exec mv {} "$ARCHIVE_DIR/" \;

echo "日志轮转完成"
```

---

## 故障排除

### 🔧 常见问题解决

#### 1. **服务无法启动**
```bash
# 检查端口占用
netstat -tlnp | grep :8080

# 检查 Docker 容器状态
docker ps -a

# 查看服务日志
docker logs alpha-collector
docker logs alpha-data-engine
```

#### 2. **数据库连接问题**
```bash
# 测试数据库连接
psql "postgresql://alpha:password@localhost:5432/alpha_finance" -c "SELECT 1;"

# 检查 PostgreSQL 状态
systemctl status postgresql

# 重启数据库
sudo systemctl restart postgresql
sudo systemctl restart redis
```

#### 3. **性能问题**
```bash
# 检查系统资源
htop
iostat -x 1

# 检查 Rust 内存使用
pmap $(pgrep -f collector | head -1)

# 分析日志
tail -f /var/log/alpha-finance/collector.log | grep ERROR
```

#### 4. **内存泄漏检测**
```bash
# 使用 valgrind 检测内存泄漏
valgrind --tool=memcheck --leak-check=full ./target/release/alpha-collector

# 使用 AddressSanitizer
RUSTFLAGS="-Zaddress-sanitizer -fsanitize=address" cargo build --release
```

---

## 性能优化

### ⚡ 性能调优建议

#### 1. **数据库优化**
```sql
-- 创建索引
CREATE INDEX CONCURRENTLY ON market_data(timestamp, symbol);
CREATE INDEX CONCURRENTLY ON task_results(created_at);

-- 分区表
CREATE TABLE market_data_2024 PARTITION OF market_data
FOR VALUES FROM ('2024-01-01') TO ('2024-12-31');

-- 查询优化
EXPLAIN ANALYZE SELECT * FROM market_data WHERE symbol='AAPL' AND timestamp >= '2024-01-01';
```

#### 2. **Rust 代码优化**
```toml
# Cargo.toml 优化配置
[profile.release]
opt-level = 3
lto = true
codegen-units = 1
panic = "abort"

[dependencies]
# 使用优化的依赖版本
tokio = { version = "1.39", features = ["full", "parking_lot"] }
axum = { version = "0.7", features = ["macros", "http2"] }
serde = { version = "1.0.210", features = ["derive"] }
```

#### 3. **系统级优化**
```bash
# 系统参数优化
echo 'net.core.somaxconn = 65536' >> /etc/sysctl.conf
echo 'vm.swappiness = 10' >> /etc/sysctl.conf
echo 'fs.file-max = 2097152' >> /etc/sysctl.conf

# 应用参数
sysctl -p
```

#### 4. **缓存策略**
```toml
# Redis 缓存配置
[cache]
default_ttl = 3600
max_memory = "1GB"
compression = "gzip"
serialization = "json"
```

### 📈 扩展指南

#### 1. **添加新数据源**
```rust
// 1. 在 types.rs 中添加新的数据源类型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskSource {
    // 现有类型...
    AShare { ... },
    HKShare { ... },
    // 新增数据源
    CryptoExchange {
        exchange: String,
        symbols: Vec<String>,
        api_endpoint: String,
        auth_type: AuthType,
    },
    CommodityMarket {
        exchange: String,
        commodities: Vec<String>,
        data_types: Vec<DataType>,
    },
}

// 2. 实现对应的解析器和配置
impl CryptoExchangeDataSource {
    pub async fn fetch_data(&self, config: &DataSourceConfig) -> Result<Vec<MarketData>> {
        // 实现加密交易所 API 调用
    }
}
```

#### 2. **水平扩展**
```bash
# Kubernetes 部署配置
apiVersion: apps/v1
kind: Deployment
metadata:
  name: alpha-collector
  labels:
    app: alpha-collector
spec:
  replicas: 3
  selector:
    matchLabels:
      app: alpha-collector
  template:
    metadata:
      labels:
        app: alpha-collector
    spec:
      containers:
      - name: alpha-collector
        image: alpha-finance/collector:latest
        ports:
        - containerPort: 8080
        env:
          - name: DATABASE_URL
            valueFrom:
              secretKeyRef:
                name: database-url
          - name: REDIS_URL
            valueFrom:
              secretKeyRef:
                name: redis-url
          - name: KAFKA_BROKERS
            valueFrom:
              secretKeyRef:
                name: kafka-brokers
        resources:
          limits:
            memory: "1Gi"
            cpu: "500m"
```

---

## 🔒 安全最佳实践

### 1. **数据安全**
- **加密存储**: 所有敏感数据使用 AES-256 加密
- **访问控制**: 实施基于角色的访问控制 (RBAC)
- **审计日志**: 记录所有数据访问和修改操作
- **定期备份**: 每日自动备份数据库
- **网络隔离**: 使用 VPC 或防火墙隔离服务网络

### 2. **应用安全**
- **HTTPS 强制**: 生产环境强制使用 HTTPS
- **API 密钥轮换**: 定期轮换 API 密钥
- **输入验证**: 严格验证所有 API 输入
- **依赖安全**: 使用最新版本的依赖，定期安全扫描

### 3. **运行时安全**
- **容器化部署**: 使用非 root 用户运行容器
- **资源限制**: 设置合理的 CPU 和内存限制
- **健康检查**: 定期检查服务健康状态
- **监控告警**: 配置异常情况的告警机制

---

## 📝 API 文档

### 核心 API 端点

| 方法 | 端点 | 描述 | 参数 |
|------|------|--------|--------|------|--------|
| GET | `/health` | 健康检查 | 无 |
| GET | `/metrics` | 获取监控指标 | 无 |
| GET | `/stats` | 获取系统统计 | 无 |
| GET | `/tasks` | 获取任务列表 | `?status=pending`, `?limit=10` |
| GET | `/tasks/{id}` | 获取任务详情 | 任务ID |
| GET | `/tasks/{id}/status` | 获取任务状态 | 任务ID |
| POST | `/tasks` | 创建新任务 | 任务配置JSON |
| POST | `/tasks/{id}/execute` | 执行任务 | 任务ID |
| POST | `/tasks/{id}/cancel` | 取消任务 | 任务ID |
| POST | `/tasks/{id}/retry` | 重试任务 | 任务ID |
| DELETE | `/tasks/{id}` | 删除任务 | 任务ID |

### WebSocket 事件

| 事件类型 | 描述 | 数据格式 |
|----------|--------|--------|----------|----------|
| `task.created` | 任务创建 | `{ "taskId": "uuid", "task": {...} }` |
| `task.updated` | 任务更新 | `{ "taskId": "uuid", "status": "running", "timestamp": "2024-01-01T00:00:00Z" }` |
| `task.completed` | 任务完成 | `{ "taskId": "uuid", "result": {...}, "executionTime": 120.5 }` |
| `task.failed` | 任务失败 | `{ "taskId": "uuid", "error": "Timeout", "timestamp": "2024-01-01T00:00:00Z" }` |
| `system.status` | 系统状态 | `{ "timestamp": "2024-01-01T00:00:00Z", "cpu": 45.2, "memory": 67.8, "connections": 1200 }` |

---

这个部署文档提供了 Alpha Finance 项目的完整部署指南，包括：

- 🏗️ **完整的系统架构图**
- 🚀 **一键部署脚本**
- ⚙️ **详细的配置示例**
- 🛠️ **全面的故障排除指南**
- 📈 **性能优化建议**
- 🔒 **安全最佳实践**
- 📝 **完整的 API 文档**

通过这个文档，开发和运维团队可以快速部署和管理 Alpha Finance 金融数据平台。