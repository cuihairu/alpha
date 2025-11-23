#!/bin/bash

# Alpha Finance 开发环境启动脚本

set -e

echo "🚀 启动 Alpha Finance 开发环境..."

# 检查是否安装了必要的工具
command -v docker >/dev/null 2>&1 || { echo "❌ Docker 未安装，请先安装 Docker"; exit 1; }
command -v docker-compose >/dev/null 2>&1 || { echo "❌ Docker Compose 未安装，请先安装 Docker Compose"; exit 1; }

# 启动基础服务（数据库和缓存）
echo "📦 启动基础服务..."
docker-compose up -d postgres redis

# 等待数据库启动
echo "⏳ 等待数据库启动..."
sleep 10

# 启动监控服务
echo "📊 启动监控服务..."
docker-compose up -d prometheus grafana

# 启动微服务
echo "🔧 启动微服务..."
docker-compose up -d api-gateway data-engine real-time-feed collector

# 等待服务启动
echo "⏳ 等待服务启动..."
sleep 15

# 检查服务状态
echo "🔍 检查服务状态..."
docker-compose ps

echo ""
echo "✅ 开发环境启动完成！"
echo ""
echo "📊 服务访问地址："
echo "  - API 网关:     http://localhost:8080"
echo "  - 数据引擎:     http://localhost:8081"
echo "  - 实时数据:     http://localhost:8082"
echo "  - 数据采集:     http://localhost:8083"
echo "  - Prometheus:   http://localhost:9090"
echo "  - Grafana:      http://localhost:3000 (admin/admin)"
echo "  - PostgreSQL:   localhost:5432"
echo "  - Redis:        localhost:6379"
echo ""
echo "📝 查看日志:"
echo "  docker-compose logs -f [service-name]"
echo ""
echo "🛑 停止服务:"
echo "  docker-compose down"