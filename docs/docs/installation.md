---
title: 安装指南
---

# 安装指南

本指南将帮助您在不同环境中安装和配置 Alpha Finance。

## 📦 安装方式

Alpha Finance 提供多种安装方式，您可以根据需求选择最适合的一种：

| 安装方式 | 适用场景 | 优点 | 缺点 |
|----------|----------|------|------|
| **源码构建** | 开发、定制 | 完全可控、最新功能 | 编译时间长 |
| **Docker** | 生产、测试 | 环境一致性、易部署 | 镜像体积较大 |
| **一键脚本** | 快速部署 | 简单快速 | 灵活性较低 |

## 🔧 源码构建

### 1. 克隆项目

```bash
# 克隆主仓库
git clone https://github.com/cuihairu/alpha.git
cd alpha

# 或者克隆您的 Fork
git clone https://github.com/YOUR_USERNAME/alpha.git
cd alpha
```

### 2. 环境检查

确保您已安装所需的环境（[查看环境要求](./getting-started.md)）：

```bash
# 检查 Rust
rustc --version && cargo --version

# 检查 Node.js
node --version && npm --version

# 检查 Docker
docker --version
```

### 3. 构建项目

```bash
# 构建 Rust 项目
cargo build --release

# 构建 WebAssembly 模块
./build-wasm.sh

# 构建前端
cd web
npm install
npm run build
cd ..

# 构建桌面应用（可选）
cargo build --release --package alpha-desktop
```

### 4. 启动服务

```bash
# 启动 ClickHouse（使用 Docker）
docker-compose up -d clickhouse

# 等待 ClickHouse 启动
sleep 30

# 初始化数据库
./scripts/clickhouse-init.sh

# 启动微服务
cargo run --release --bin alpha-api-gateway &
cargo run --release --bin alpha-data-engine &
cargo run --release --bin alpha-real-time-feed &

# 启动 Web 前端
cd web
npm start
```

默认仅监听本机（`127.0.0.1`）。如需局域网访问：
```bash
cd web
HOST=0.0.0.0 PORT=8080 npm start
```

## 🐳 Docker 安装

### 1. 使用 Docker Compose

```bash
# 克隆项目
git clone https://github.com/cuihairu/alpha.git
cd alpha

# 启动所有服务
docker-compose up -d

# 查看服务状态
docker-compose ps

# 查看日志
docker-compose logs -f
```

### 2. Dockerfile 构建

```bash
# 构建镜像
docker build -t alpha-finance:latest .

# 运行容器
docker run -d \
  --name alpha-finance \
  -p 8080:8080 \
  -p 9080:9080 \
  -v /opt/alpha/data:/data \
  alpha-finance:latest
```

### 3. 自定义配置

创建 `docker-compose.override.yml`：

```yaml
version: '3.8'

services:
  clickhouse:
    environment:
      CLICKHOUSE_DB: custom_database
      CLICKHOUSE_USER: custom_user
      CLICKHOUSE_PASSWORD: custom_password
    ports:
      - "8123:8123"
      - "9000:9000"
    volumes:
      - ./clickhouse-data:/var/lib/clickhouse

  api-gateway:
    environment:
      RUST_LOG: debug
      CLICKHOUSE_HOST: clickhouse
    ports:
      - "9080:9080"
    depends_on:
      - clickhouse
```

## 🚀 一键脚本安装

### Ubuntu/Debian

```bash
# 下载安装脚本
curl -fsSL https://raw.githubusercontent.com/cuihairu/alpha/main/scripts/install-ubuntu.sh -o install.sh

# 执行安装
chmod +x install.sh
sudo ./install.sh
```

### CentOS/RHEL

```bash
# 下载安装脚本
curl -fsSL https://raw.githubusercontent.com/cuihairu/alpha/main/scripts/install-centos.sh -o install.sh

# 执行安装
chmod +x install.sh
sudo ./install.sh
```

### macOS

```bash
# 下载安装脚本
curl -fsSL https://raw.githubusercontent.com/cuihairu/alpha/main/scripts/install-macos.sh -o install.sh

# 执行安装
chmod +x install.sh
./install.sh
```

## ⚙️ 配置设置

### 1. 环境变量

复制环境变量模板：

```bash
cp .env.example .env
```

编辑 `.env` 文件：

```bash
# 数据库配置
CLICKHOUSE_HOST=localhost
CLICKHOUSE_PORT=8123
CLICKHOUSE_USER=admin
CLICKHOUSE_PASSWORD=your_secure_password
CLICKHOUSE_DATABASE=alpha_finance

# 服务端口
API_GATEWAY_PORT=9080
WEB_PORT=8080

# 日志级别
RUST_LOG=info

# 安全配置
JWT_SECRET=your_super_secret_jwt_key
API_SECRET_KEY=your_api_secret_key
```

### 2. ClickHouse 配置

```bash
# 检查 ClickHouse 配置
cat /etc/clickhouse-server/config.xml

# 自定义配置
sudo tee /etc/clickhouse-server/config.d/custom.xml << EOF
<clickhouse>
    <max_memory_usage>10000000000</max_memory_usage>
    <max_threads>8</max_threads>
    <background_pool_size>8</background_pool_size>
</clickhouse>
EOF

# 重启 ClickHouse
sudo systemctl restart clickhouse-server
```

### 3. Nginx 配置

```bash
# 安装 Nginx
sudo apt install -y nginx  # Ubuntu/Debian
sudo yum install -y nginx  # CentOS/RHEL
brew install nginx          # macOS

# 配置文件
sudo tee /etc/nginx/sites-available/alpha << EOF
server {
    listen 80;
    server_name your-domain.com;

    location / {
        root /opt/alpha/web/dist;
        index index.html;
        try_files \$uri \$uri/ /index.html;
    }

    location /api/ {
        proxy_pass http://localhost:9080/;
        proxy_set_header Host \$host;
        proxy_set_header X-Real-IP \$remote_addr;
        proxy_set_header X-Forwarded-For \$proxy_add_x_forwarded_for;
    }
}
EOF

# 启用站点
sudo ln -s /etc/nginx/sites-available/alpha /etc/nginx/sites-enabled/
sudo nginx -t && sudo systemctl reload nginx
```

## 🔍 验证安装

### 1. 检查服务状态

```bash
# 检查 ClickHouse
curl http://localhost:8123/ping

# 检查 API Gateway
curl http://localhost:9080/health

# 检查前端
curl -I http://localhost:8080
```

### 2. 运行测试

```bash
# 运行单元测试
cargo test

# 运行集成测试
cargo test --test integration_tests

# 测试 ClickHouse 连接
cargo run --package alpha-storage --bin clickhouse_test
```

### 3. 验证功能

- 访问 Web 应用：`http://localhost:8080`
- 查看 API 文档：`http://localhost:9080/docs`
- 数据库管理：`http://localhost:8123`

## 🛠️ 故障排除

### 常见安装问题

#### 1. Rust 编译失败

```bash
# 清理缓存
cargo clean

# 更新工具链
rustup update

# 检查目标平台
rustup target list --installed
rustup target add wasm32-unknown-unknown
```

#### 2. Node.js 依赖问题

```bash
# 清理缓存
npm cache clean --force

# 删除 node_modules
rm -rf node_modules package-lock.json

# 重新安装
npm install
```

#### 3. Docker 启动失败

```bash
# 检查 Docker 服务
sudo systemctl status docker

# 检查端口占用
netstat -tlnp | grep -E "8080|9080|8123"

# 查看容器日志
docker logs alpha-finance
```

#### 4. ClickHouse 连接失败

```bash
# 检查 ClickHouse 服务
sudo systemctl status clickhouse-server

# 检查配置文件
cat /etc/clickhouse-server/config.xml

# 手动连接测试
clickhouse-client --host localhost --user admin --password admin123
```

### 获取帮助

如果遇到安装问题：

1. 查看 [故障排除指南](./troubleshooting.md)
2. 搜索 [GitHub Issues](https://github.com/cuihairu/alpha/issues)
3. 创建新的 Issue 并提供详细信息

## 📚 下一步

安装完成后，您可以：

- [📖 阅读配置指南](./configuration.md)
- [🔧 了解 API 文档](./api/overview.md)
- [🏗️ 查看架构设计](./architecture/overview.md)
- [💻 开始开发](./development/setup.md)

---

**🎉 恭喜！您已经成功安装了 Alpha Finance！现在可以开始探索强大的金融数据分析功能了。**
