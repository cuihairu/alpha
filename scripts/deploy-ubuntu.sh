#!/bin/bash

# Alpha Finance Ubuntu 24.04 快速部署脚本
# 使用方法: sudo ./scripts/deploy-ubuntu.sh

set -e

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# 日志函数
log_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

log_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# 检查权限
if [ "$EUID" -ne 0 ]; then
    log_error "请使用 sudo 运行此脚本"
    exit 1
fi

log_info "🚀 Alpha Finance Ubuntu 24.04 部署开始..."
echo "======================================"

# 设置工作目录
WORK_DIR="/opt/alpha"
PROJECT_USER="$SUDO_USER"
if [ -z "$PROJECT_USER" ]; then
    PROJECT_USER="alpha"
fi

log_info "工作目录: $WORK_DIR"
log_info "项目用户: $PROJECT_USER"

# 创建项目目录
log_info "创建项目目录..."
mkdir -p $WORK_DIR
mkdir -p $WORK_DIR/{data,logs,config,backups}
mkdir -p /var/log/alpha

# 创建项目用户（如果不存在）
if ! id "$PROJECT_USER" &>/dev/null; then
    log_info "创建项目用户: $PROJECT_USER"
    useradd -r -s /bin/false $PROJECT_USER
fi

# 设置权限
chown -R $PROJECT_USER:$PROJECT_USER $WORK_DIR

# 更新系统
log_info "更新系统包..."
apt update && apt upgrade -y

# 安装基础工具
log_info "安装基础工具..."
apt install -y curl wget git vim htop unzip \
    build-essential pkg-config libssl-dev \
    ca-certificates gnupg lsb-release

# 安装 Docker
log_info "安装 Docker..."
curl -fsSL https://download.docker.com/linux/ubuntu/gpg | gpg --dearmor -o /usr/share/keyrings/docker-archive-keyring.gpg

echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/docker-archive-keyring.gpg] https://download.docker.com/linux/ubuntu $(lsb_release -cs) stable" | tee /etc/apt/sources.list.d/docker.list > /dev/null

apt update
apt install -y docker-ce docker-ce-cli containerd.io

# 安装 Docker Compose
log_info "安装 Docker Compose..."
curl -L "https://github.com/docker/compose/releases/download/v2.21.0/docker-compose-$(uname -s)-$(uname -m)" -o /usr/local/bin/docker-compose
chmod +x /usr/local/bin/docker-compose

# 将用户添加到 docker 组
usermod -aG docker $PROJECT_USER

# 启动 Docker 服务
log_info "启动 Docker 服务..."
systemctl start docker
systemctl enable docker

# 切换到项目目录
cd $WORK_DIR

# 克隆或更新项目
if [ ! -d ".git" ]; then
    log_info "克隆项目..."
    sudo -u $PROJECT_USER git clone https://github.com/cuihairu/alpha.git .
else
    log_info "更新项目..."
    sudo -u $PROJECT_USER git pull origin main
fi

# 安装 Rust
log_info "安装 Rust..."
if [ ! -f "/home/$PROJECT_USER/.cargo/env" ]; then
    sudo -u $PROJECT_USER curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
fi

# 设置 Rust 环境变量
export PATH="/home/$PROJECT_USER/.cargo/bin:$PATH"

# 安装 Node.js
log_info "安装 Node.js..."
curl -fsSL https://deb.nodesource.com/setup_18.x | bash -
apt install -y nodejs

# 安装 pm2
log_info "安装 pm2..."
npm install -g pm2

# 构建项目
log_info "构建 Rust 项目..."
sudo -u $PROJECT_USER /home/$PROJECT_USER/.cargo/bin/cargo build --release

# 构建 WebAssembly
log_info "构建 WebAssembly..."
chmod +x build-wasm.sh
sudo -u $PROJECT_USER ./build-wasm.sh

# 构建前端
log_info "构建前端..."
cd web
sudo -u $PROJECT_USER npm install
sudo -u $PROJECT_USER npm run build
cd ..

# 启动 ClickHouse
log_info "启动 ClickHouse..."
docker-compose up -d clickhouse

# 等待 ClickHouse 启动
log_info "等待 ClickHouse 启动..."
sleep 30

# 初始化数据库
log_info "初始化数据库..."
if [ -f "scripts/clickhouse-init.sh" ]; then
    chmod +x scripts/clickhouse-init.sh
    sudo -u $PROJECT_USER ./scripts/clickhouse-init.sh
else
    log_warning "ClickHouse 初始化脚本未找到，跳过初始化"
fi

# 创建环境变量文件
log_info "创建环境变量文件..."
if [ ! -f ".env" ]; then
    sudo -u $PROJECT_USER cat > .env << EOF
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
JWT_SECRET=your-super-secret-jwt-key-here-$(date +%s)
API_SECRET_KEY=your-api-secret-key-$(date +%s)

# 日志级别
RUST_LOG=info
EOF
fi

# 创建系统服务
log_info "创建系统服务..."
cat > /etc/systemd/system/alpha-api-gateway.service << EOF
[Unit]
Description=Alpha Finance API Gateway
After=network.target docker.service
Requires=docker.service

[Service]
Type=simple
User=$PROJECT_USER
WorkingDirectory=$WORK_DIR
ExecStart=$WORK_DIR/target/release/alpha-api-gateway --bind 0.0.0.0:9080
Restart=always
RestartSec=10
Environment=RUST_LOG=info
Environment=CLICKHOUSE_HOST=localhost
Environment=CLICKHOUSE_PORT=8123
Environment=CLICKHOUSE_USER=admin
Environment=CLICKHOUSE_PASSWORD=admin123
Environment=CLICKHOUSE_DATABASE=alpha_finance

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
User=$PROJECT_USER
WorkingDirectory=$WORK_DIR
ExecStart=$WORK_DIR/target/release/alpha-data-engine --bind 0.0.0.0:9082
Restart=always
RestartSec=10
Environment=RUST_LOG=info
Environment=CLICKHOUSE_HOST=localhost
Environment=CLICKHOUSE_PORT=8123
Environment=CLICKHOUSE_USER=admin
Environment=CLICKHOUSE_PASSWORD=admin123
Environment=CLICKHOUSE_DATABASE=alpha_finance

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
User=$PROJECT_USER
WorkingDirectory=$WORK_DIR
ExecStart=$WORK_DIR/target/release/alpha-real-time-feed --bind 0.0.0.0:9081
Restart=always
RestartSec=10
Environment=RUST_LOG=info
Environment=CLICKHOUSE_HOST=localhost
Environment=CLICKHOUSE_PORT=8123
Environment=CLICKHOUSE_USER=admin
Environment=CLICKHOUSE_PASSWORD=admin123
Environment=CLICKHOUSE_DATABASE=alpha_finance

[Install]
WantedBy=multi-user.target
EOF

# 配置 Nginx
if command -v nginx &> /dev/null; then
    log_info "配置 Nginx..."
    cat > /etc/nginx/sites-available/alpha << EOF
server {
    listen 80;
    server_name _;

    # 前端静态文件
    location / {
        root $WORK_DIR/web/dist;
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
    rm -f /etc/nginx/sites-enabled/default

    # 测试并重新加载 Nginx
    nginx -t && systemctl reload nginx
else
    log_warning "Nginx 未安装，跳过 Web 服务器配置"
fi

# 配置防火墙
log_info "配置防火墙..."
if command -v ufw &> /dev/null; then
    ufw --force reset
    ufw default deny incoming
    ufw default allow outgoing
    ufw allow 22/tcp
    ufw allow 80/tcp
    ufw allow 443/tcp
    ufw allow 9080/tcp
    ufw allow 9081/tcp
    ufw allow 9082/tcp
    ufw --force enable
else
    log_warning "UFW 防火墙未安装，请手动配置防火墙"
fi

# 重新加载 systemd
systemctl daemon-reload

# 启动服务
log_info "启动服务..."
systemctl enable alpha-api-gateway
systemctl enable alpha-data-engine
systemctl enable alpha-real-time-feed

# 等待一下再启动服务
sleep 5

systemctl start alpha-api-gateway
systemctl start alpha-data-engine
systemctl start alpha-real-time-feed

# 等待服务启动
log_info "等待服务启动..."
sleep 10

# 健康检查
log_info "执行健康检查..."
success_count=0

# 检查 ClickHouse
if curl -s http://localhost:8123/ping > /dev/null; then
    log_success "✅ ClickHouse 运行正常"
    ((success_count++))
else
    log_error "❌ ClickHouse 运行异常"
fi

# 检查 API Gateway
if curl -s http://localhost:9080/health > /dev/null 2>&1; then
    log_success "✅ API Gateway 运行正常"
    ((success_count++))
else
    log_warning "⚠️ API Gateway 可能还在启动中"
fi

# 检查服务状态
for service in alpha-api-gateway alpha-data-engine alpha-real-time-feed; do
    if systemctl is-active --quiet $service; then
        log_success "✅ $service 运行正常"
    else
        log_error "❌ $service 运行异常"
    fi
done

# 获取服务器 IP
SERVER_IP=$(ip -4 addr show | grep -oP '(?<=inet\s)\d+(\.\d+){3}' | grep -v '127.0.0.1' | head -n 1)

# 部署完成信息
echo ""
echo "🎉 部署完成！"
echo "================="
echo "🖥️ 服务器信息:"
echo "   IP 地址: $SERVER_IP"
echo "   项目目录: $WORK_DIR"
echo "   项目用户: $PROJECT_USER"
echo ""
echo "🌐 访问地址:"
if [ -n "$SERVER_IP" ]; then
    echo "   Web 应用: http://$SERVER_IP"
    echo "   API 文档: http://$SERVER_IP/api/docs"
else
    echo "   Web 应用: http://localhost"
    echo "   API 文档: http://localhost/api/docs"
fi
echo ""
echo "🗄️ 数据库管理:"
echo "   ClickHouse: http://localhost:8123"
echo "   用户名: admin"
echo "   密码: admin123"
echo ""
echo "📋 服务管理命令:"
echo "   查看服务状态: systemctl status alpha-*"
echo "   重启所有服务: sudo systemctl restart alpha-api-gateway alpha-data-engine alpha-real-time-feed"
echo "   查看服务日志: journalctl -u alpha-* -f"
echo "   查看容器状态: docker ps"
echo ""
echo "📝 日志位置:"
echo "   系统日志: journalctl -u alpha-*"
echo "   应用日志: /var/log/alpha/"
echo "   Docker 日志: docker logs clickhouse"
echo ""
echo "🔧 维护命令:"
echo "   更新项目: cd $WORK_DIR && sudo -u $PROJECT_USER git pull && sudo systemctl restart alpha-*"
echo "   查看端口占用: netstat -tlnp | grep -E '9080|9081|9082|8123'"
echo "   备份数据: ./scripts/backup.sh"
echo ""

if [ $success_count -ge 2 ]; then
    log_success "🚀 Alpha Finance 部署成功！系统已准备就绪。"
else
    log_warning "⚠️ 部署完成但部分服务可能需要手动检查。"
fi

echo "📚 更多信息请查看: $WORK_DIR/docs/DEPLOYMENT.md"