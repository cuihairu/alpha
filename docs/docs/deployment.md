# Alpha Finance Ubuntu 24.04 部署指南

## 📋 目录

- [系统要求](#系统要求)
- [环境准备](#环境准备)
- [快速部署](#快速部署)
- [详细配置](#详细配置)
- [服务管理](#服务管理)
- [监控和维护](#监控和维护)
- [故障排除](#故障排除)

## 🖥️ 系统要求

### 硬件要求
- **CPU**: 4核心以上（推荐8核心）
- **内存**: 8GB以上（推荐16GB）
- **存储**: 100GB以上可用空间（推荐SSD）
- **网络**: 稳定的互联网连接

### 软件要求
- **操作系统**: Ubuntu 24.04 LTS
- **用户权限**: sudo 权限
- **防火墙**: 开放必要端口

## 🔧 环境准备

### 1. 系统更新

```bash
# 更新系统包
sudo apt update && sudo apt upgrade -y

# 安装基础工具
sudo apt install -y curl wget git vim htop unzip \
    build-essential pkg-config libssl-dev \
    ca-certificates gnupg lsb-release
```

### 2. 安装 Docker 和 Docker Compose

```bash
# 安装 Docker
curl -fsSL https://download.docker.com/linux/ubuntu/gpg | sudo gpg --dearmor -o /usr/share/keyrings/docker-archive-keyring.gpg

echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/docker-archive-keyring.gpg] https://download.docker.com/linux/ubuntu $(lsb_release -cs) stable" | sudo tee /etc/apt/sources.list.d/docker.list > /dev/null

sudo apt update
sudo apt install -y docker-ce docker-ce-cli containerd.io

# 安装 Docker Compose
sudo curl -L "https://github.com/docker/compose/releases/download/v2.21.0/docker-compose-$(uname -s)-$(uname -m)" -o /usr/local/bin/docker-compose
sudo chmod +x /usr/local/bin/docker-compose

# 将用户添加到 docker 组
sudo usermod -aG docker $USER
newgrp docker

# 启动 Docker 服务
sudo systemctl start docker
sudo systemctl enable docker
```

### 3. 安装 Rust 工具链

```bash
# 安装 Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y

# 重新加载环境变量
source ~/.cargo/env

# 验证安装
rustc --version
cargo --version

# 安装常用组件
rustup component add clippy rustfmt
```

### 4. 安装 Node.js

```bash
# 安装 Node.js 18.x
curl -fsSL https://deb.nodesource.com/setup_18.x | sudo -E bash -
sudo apt install -y nodejs

# 验证安装
node --version
npm --version

# 安装 pm2（进程管理）
sudo npm install -g pm2
```

## 🚀 快速部署

### 1. 克隆项目

```bash
# 创建项目目录
sudo mkdir -p /opt/alpha
sudo chown $USER:$USER /opt/alpha
cd /opt/alpha

# 克隆项目
git clone https://github.com/cuihairu/alpha.git .

# 或者如果您有本地项目，可以复制过来
# scp -r /path/to/local/alpha/* user@server:/opt/alpha/
```

### 2. 环境配置

```bash
# 创建环境变量文件
cp .env.example .env

# 编辑环境变量
vim .env
```

**`.env` 文件示例：**
```bash
# 数据库配置
CLICKHOUSE_HOST=localhost
CLICKHOUSE_PORT=8123
CLICKHOUSE_USER=admin
CLICKHOUSE_PASSWORD=admin123
CLICKHOUSE_DATABASE=alpha_finance

# 服务端口
API_GATEWAY_PORT=9080
REAL_TIME_FEED_PORT=9081
DATA_ENGINE_PORT=9082
COLLECTOR_PORT=9083

# Web 前端
WEB_PORT=8080

# 安全配置
JWT_SECRET=your-super-secret-jwt-key-here
API_SECRET_KEY=your-api-secret-key

# 日志级别
RUST_LOG=info
```

### 3. 一键部署脚本

```bash
# 创建部署脚本
vim scripts/deploy-ubuntu.sh
```

**`scripts/deploy-ubuntu.sh` 内容：**
```bash
#!/bin/bash

set -e

echo "🚀 Alpha Finance Ubuntu 24.04 部署开始..."
echo "======================================"

# 检查权限
if [ "$EUID" -ne 0 ]; then
    echo "❌ 请使用 sudo 运行此脚本"
    exit 1
fi

# 创建必要目录
mkdir -p /opt/alpha/{data,logs,config,backups}
mkdir -p /var/log/alpha
chown -R $USER:$USER /opt/alpha

# 构建项目
echo "🔨 构建项目..."
cd /opt/alpha

# 构建 Rust 项目
cargo build --release

# 构建 WebAssembly
chmod +x build-wasm.sh
./build-wasm.sh

# 构建前端
cd web
npm install
npm run build
cd ..

# 启动 ClickHouse
echo "🗄️ 启动 ClickHouse..."
docker-compose up -d clickhouse

# 等待 ClickHouse 启动
echo "⏳ 等待 ClickHouse 启动..."
sleep 30

# 初始化数据库
echo "🔧 初始化数据库..."
./scripts/clickhouse-init.sh

# 创建系统服务
echo "🔧 创建系统服务..."
cat > /etc/systemd/system/alpha-api-gateway.service << EOF
[Unit]
Description=Alpha Finance API Gateway
After=network.target docker.service
Requires=docker.service

[Service]
Type=simple
User=alpha
WorkingDirectory=/opt/alpha
ExecStart=/opt/alpha/target/release/alpha-api-gateway --bind 0.0.0.0:9080
Restart=always
RestartSec=10
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
EOF

cat > /etc/systemd/system/alpha-data-engine.service << EOF
[Unit]
Description=Alpha Finance Data Engine
After=network.target docker.service
Requires=docker.service

[Service]
Type=simple
User=alpha
WorkingDirectory=/opt/alpha
ExecStart=/opt/alpha/target/release/alpha-data-engine --bind 0.0.0.0:9082
Restart=always
RestartSec=10
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
EOF

cat > /etc/systemd/system/alpha-real-time-feed.service << EOF
[Unit]
Description=Alpha Finance Real Time Feed
After=network.target docker.service
Requires=docker.service

[Service]
Type=simple
User=alpha
WorkingDirectory=/opt/alpha
ExecStart=/opt/alpha/target/release/alpha-real-time-feed --bind 0.0.0.0:9081
Restart=always
RestartSec=10
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
EOF

# 创建 alpha 用户
if ! id "alpha" &>/dev/null; then
    useradd -r -s /bin/false alpha
fi
chown -R alpha:alpha /opt/alpha

# 重新加载 systemd
systemctl daemon-reload

# 启动服务
echo "🚀 启动服务..."
systemctl enable alpha-api-gateway
systemctl enable alpha-data-engine
systemctl enable alpha-real-time-feed

systemctl start alpha-api-gateway
systemctl start alpha-data-engine
systemctl start alpha-real-time-feed

# 配置 Nginx（可选）
echo "🌐 配置 Nginx..."
if command -v nginx &> /dev/null; then
    cat > /etc/nginx/sites-available/alpha << EOF
server {
    listen 80;
    server_name your-domain.com;

    # 前端静态文件
    location / {
        root /opt/alpha/web/dist;
        index index.html;
        try_files \$uri \$uri/ /index.html;
    }

    # API 网关代理
    location /api/ {
        proxy_pass http://localhost:9080/;
        proxy_set_header Host \$host;
        proxy_set_header X-Real-IP \$remote_addr;
        proxy_set_header X-Forwarded-For \$proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto \$scheme;
    }

    # WebSocket 代理
    location /ws/ {
        proxy_pass http://localhost:9081/;
        proxy_http_version 1.1;
        proxy_set_header Upgrade \$http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host \$host;
        proxy_set_header X-Real-IP \$remote_addr;
    }
}
EOF

    # 启用站点
    ln -sf /etc/nginx/sites-available/alpha /etc/nginx/sites-enabled/
    nginx -t && systemctl reload nginx
fi

# 配置防火墙
echo "🔥 配置防火墙..."
if command -v ufw &> /dev/null; then
    ufw allow 22/tcp
    ufw allow 80/tcp
    ufw allow 443/tcp
    ufw allow 9080/tcp
    ufw allow 9081/tcp
    ufw allow 9082/tcp
    ufw --force enable
fi

# 健康检查
echo "🔍 健康检查..."
sleep 10

if curl -s http://localhost:9080/health > /dev/null; then
    echo "✅ API Gateway 运行正常"
else
    echo "❌ API Gateway 运行异常"
fi

if curl -s http://localhost:8123/ping > /dev/null; then
    echo "✅ ClickHouse 运行正常"
else
    echo "❌ ClickHouse 运行异常"
fi

echo ""
echo "🎉 部署完成！"
echo "================="
echo "📊 监控面板: http://localhost:8080"
echo "🔗 API 文档: http://localhost:9080/docs"
echo "🗄️ 数据库: http://localhost:8123"
echo ""
echo "📋 服务状态:"
echo "  API Gateway: systemctl status alpha-api-gateway"
echo "  Data Engine: systemctl status alpha-data-engine"
echo "  Real Time Feed: systemctl status alpha-real-time-feed"
echo "  ClickHouse: docker ps | grep clickhouse"
echo ""
echo "📝 日志位置:"
echo "  服务日志: journalctl -u alpha-*"
echo "  应用日志: /var/log/alpha/"
```

### 4. 执行部署

```bash
# 使脚本可执行
chmod +x scripts/deploy-ubuntu.sh

# 执行部署
sudo ./scripts/deploy-ubuntu.sh
```

## ⚙️ 详细配置

### 1. ClickHouse 配置

```bash
# 编辑 ClickHouse 配置
vim /etc/clickhouse-server/config.xml
```

**关键配置项：**
```xml
<!-- 设置监听地址 -->
<listen_host>::</listen_host>

<!-- 设置最大内存限制 -->
<max_memory_usage>10000000000</max_memory_usage>

<!-- 设置日志级别 -->
<level>information</level>

<!-- 启用压缩 -->
<http_server_default_compression>zstd</http_server_default_compression>
```

### 2. Nginx 高级配置

```bash
# 编辑 Nginx 配置
vim /etc/nginx/sites-available/alpha
```

**生产环境配置：**
```nginx
server {
    listen 80;
    server_name your-domain.com;
    return 301 https://$server_name$request_uri;
}

server {
    listen 443 ssl http2;
    server_name your-domain.com;

    ssl_certificate /path/to/certificate.crt;
    ssl_certificate_key /path/to/private.key;
    ssl_protocols TLSv1.2 TLSv1.3;
    ssl_ciphers ECDHE-RSA-AES256-GCM-SHA512:DHE-RSA-AES256-GCM-SHA512;
    ssl_prefer_server_ciphers off;

    # 前端静态文件
    location / {
        root /opt/alpha/web/dist;
        index index.html;
        try_files $uri $uri/ /index.html;

        # 缓存设置
        location ~* \.(js|css|png|jpg|jpeg|gif|ico|svg)$ {
            expires 1y;
            add_header Cache-Control "public, immutable";
        }
    }

    # API 代理
    location /api/ {
        proxy_pass http://localhost:9080/;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;

        # 超时设置
        proxy_connect_timeout 60s;
        proxy_send_timeout 60s;
        proxy_read_timeout 60s;
    }

    # WebSocket 代理
    location /ws/ {
        proxy_pass http://localhost:9081/;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_read_timeout 86400;
    }

    # 限制请求大小
    client_max_body_size 10M;

    # 安全头
    add_header X-Frame-Options "SAMEORIGIN" always;
    add_header X-XSS-Protection "1; mode=block" always;
    add_header X-Content-Type-Options "nosniff" always;
    add_header Referrer-Policy "no-referrer-when-downgrade" always;
    add_header Content-Security-Policy "default-src 'self' http: https: data: blob: 'unsafe-inline'" always;
}
```

### 3. SSL 证书配置

```bash
# 使用 Let's Encrypt
sudo apt install certbot python3-certbot-nginx

# 获取证书
sudo certbot --nginx -d your-domain.com

# 自动续期
sudo crontab -e
# 添加以下行
0 12 * * * /usr/bin/certbot renew --quiet
```

## 📊 服务管理

### 查看服务状态

```bash
# 查看所有 Alpha 服务
systemctl status alpha-api-gateway alpha-data-engine alpha-real-time-feed

# 查看 Docker 容器
docker ps

# 查看端口占用
netstat -tlnp | grep -E "9080|9081|9082|8123"
```

### 日志管理

```bash
# 实时查看服务日志
journalctl -u alpha-api-gateway -f
journalctl -u alpha-data-engine -f
journalctl -u alpha-real-time-feed -f

# 查看 ClickHouse 日志
docker logs clickhouse

# 查看应用日志
tail -f /var/log/alpha/api-gateway.log
tail -f /var/log/alpha/data-engine.log
```

### 服务重启

```bash
# 重启单个服务
sudo systemctl restart alpha-api-gateway

# 重启所有服务
sudo systemctl restart alpha-api-gateway alpha-data-engine alpha-real-time-feed

# 重启 ClickHouse
docker-compose restart clickhouse
```

## 📈 监控和维护

### 1. 系统监控

```bash
# 安装监控工具
sudo apt install -y htop iotop nethogs

# 监控系统资源
htop
iotop
nethogs

# 监控磁盘使用
df -h
du -sh /opt/alpha
```

### 2. 备份策略

```bash
# 创建备份脚本
vim scripts/backup.sh
```

**`scripts/backup.sh` 内容：**
```bash
#!/bin/bash

BACKUP_DIR="/opt/alpha/backups"
DATE=$(date +%Y%m%d_%H%M%S)

# 创建备份目录
mkdir -p $BACKUP_DIR

# 备份 ClickHouse 数据
docker exec clickhouse clickhouse-backup create backup_$DATE

# 备份配置文件
tar -czf $BACKUP_DIR/config_$DATE.tar.gz /opt/alpha/.env /etc/nginx/sites-available/alpha

# 备份应用数据
tar -czf $BACKUP_DIR/data_$DATE.tar.gz /opt/alpha/data

# 清理旧备份（保留7天）
find $BACKUP_DIR -name "*.tar.gz" -mtime +7 -delete

echo "备份完成: $DATE"
```

```bash
# 设置定时备份
chmod +x scripts/backup.sh
crontab -e
# 添加以下行（每天凌晨2点备份）
0 2 * * * /opt/alpha/scripts/backup.sh
```

### 3. 性能优化

```bash
# 系统优化
echo 'vm.swappiness=10' | sudo tee -a /etc/sysctl.conf
echo 'net.core.somaxconn=65535' | sudo tee -a /etc/sysctl.conf
sudo sysctl -p

# ClickHouse 优化
vim /etc/clickhouse-server/config.xml
# 添加以下配置：
<max_memory_usage>20000000000</max_memory_usage>
<max_threads>8</max_threads>
<background_pool_size>8</background_pool_size>
```

## 🔧 故障排除

### 常见问题

1. **服务启动失败**
```bash
# 检查服务状态
systemctl status alpha-api-gateway

# 查看错误日志
journalctl -u alpha-api-gateway -n 50

# 检查端口占用
netstat -tlnp | grep 9080
```

2. **ClickHouse 连接失败**
```bash
# 检查 ClickHouse 状态
docker ps | grep clickhouse

# 测试连接
curl http://localhost:8123/ping

# 查看日志
docker logs clickhouse
```

3. **前端无法访问**
```bash
# 检查 Nginx 配置
nginx -t

# 重启 Nginx
sudo systemctl restart nginx

# 检查文件权限
ls -la /opt/alpha/web/dist/
```

4. **内存不足**
```bash
# 检查内存使用
free -h

# 调整服务配置
vim /opt/alpha/.env
# 添加：RUST_MIN_STACK_SIZE=33554432
```

### 性能问题诊断

```bash
# 检查 CPU 使用
top -p $(pgrep -f "alpha-")

# 检查内存使用
ps aux | grep -E "alpha-|clickhouse" | sort -k4 -nr

# 检查网络连接
ss -tuln | grep -E "9080|9081|9082|8123"

# 检查磁盘 I/O
iotop -o
```

## 📞 技术支持

如果遇到部署问题，请：

1. 检查日志文件获取详细错误信息
2. 确认系统资源是否充足
3. 验证网络连接和防火墙配置
4. 查看项目文档和 GitHub Issues

### 联系方式
- 项目地址: https://github.com/cuihairu/alpha
- 问题反馈: GitHub Issues
- 技术文档: 项目 Wiki

---

**🎉 部署成功后，您就可以开始使用 Alpha Finance 金融数据分析平台了！**
