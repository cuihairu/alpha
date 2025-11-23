#!/bin/bash

# Alpha Finance ClickHouse 初始化脚本
# 用于设置和初始化 ClickHouse 数据库

set -e

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# 配置变量
CLICKHOUSE_HOST=${CLICKHOUSE_HOST:-"localhost"}
CLICKHOUSE_PORT=${CLICKHOUSE_PORT:-"8123"}
CLICKHOUSE_USER=${CLICKHOUSE_USER:-"admin"}
CLICKHOUSE_PASSWORD=${CLICKHOUSE_PASSWORD:-"admin123"}
CLICKHOUSE_DATABASE=${CLICKHOUSE_DATABASE:-"alpha_finance"}

echo -e "${BLUE}🚀 Alpha Finance ClickHouse 初始化开始${NC}"

# 等待 ClickHouse 启动
echo -e "${YELLOW}⏳ 等待 ClickHouse 服务启动...${NC}"
until curl -s "http://${CLICKHOUSE_HOST}:${CLICKHOUSE_PORT}/ping" > /dev/null; do
    echo -e "${YELLOW}   ClickHouse 尚未就绪，等待中...${NC}"
    sleep 2
done

echo -e "${GREEN}✅ ClickHouse 服务已就绪${NC}"

# 检查数据库是否存在
echo -e "${YELLOW}📊 检查数据库状态...${NC}"
DB_EXISTS=$(curl -s -G "http://${CLICKHOUSE_HOST}:${CLICKHOUSE_PORT}" \
    --data-urlencode "query=SELECT name FROM system.databases WHERE name='${CLICKHOUSE_DATABASE}'" \
    --user "${CLICKHOUSE_USER}:${CLICKHOUSE_PASSWORD}" | wc -l)

if [ "$DB_EXISTS" -eq "0" ]; then
    echo -e "${YELLOW}🏗️  创建数据库 ${CLICKHOUSE_DATABASE}...${NC}"
    curl -s -X POST "http://${CLICKHOUSE_HOST}:${CLICKHOUSE_PORT}" \
        --data-urlencode "query=CREATE DATABASE IF NOT EXISTS ${CLICKHOUSE_DATABASE}" \
        --user "${CLICKHOUSE_USER}:${CLICKHOUSE_PASSWORD}"
    echo -e "${GREEN}✅ 数据库创建成功${NC}"
else
    echo -e "${GREEN}✅ 数据库 ${CLICKHOUSE_DATABASE} 已存在${NC}"
fi

# 执行架构初始化
echo -e "${YELLOW}🔧 执行数据库架构初始化...${NC}"
if [ -f "$(dirname "$0")/../config/clickhouse-schema.sql" ]; then
    curl -X POST "http://${CLICKHOUSE_HOST}:${CLICKHOUSE_PORT}" \
        --data-binary @$(dirname "$0")/../config/clickhouse-schema.sql \
        --user "${CLICKHOUSE_USER}:${CLICKHOUSE_PASSWORD}"
    echo -e "${GREEN}✅ 架构初始化完成${NC}"
else
    echo -e "${RED}❌ 架构文件未找到: clickhouse-schema.sql${NC}"
    exit 1
fi

# 验证表创建
echo -e "${YELLOW}🔍 验证表创建状态...${NC}"
TABLES=$(curl -s -G "http://${CLICKHOUSE_HOST}:${CLICKHOUSE_PORT}" \
    --data-urlencode "query=SELECT name FROM system.tables WHERE database='${CLICKHOUSE_DATABASE}'" \
    --user "${CLICKHOUSE_USER}:${CLICKHOUSE_PASSWORD}")

echo -e "${BLUE}📋 已创建的表:${NC}"
echo "$TABLES" | grep -v "name" || echo "  (没有表)"

# 插入示例数据
echo -e "${YELLOW}📝 插入示例数据...${NC}"
SAMPLE_DATA_SQL="
INSERT INTO symbols (symbol, name, exchange, sector, industry, market_cap, created_at, updated_at) VALUES
('AAPL', 'Apple Inc.', 'NASDAQ', 'Technology', 'Consumer Electronics', 3000000000000, now(), now()),
('GOOGL', 'Alphabet Inc.', 'NASDAQ', 'Technology', 'Search Engine', 2000000000000, now(), now()),
('MSFT', 'Microsoft Corporation', 'NASDAQ', 'Technology', 'Software', 2800000000000, now(), now()),
('AMZN', 'Amazon.com Inc.', 'NASDAQ', 'Consumer Cyclical', 'E-Commerce', 1800000000000, now(), now()),
('TSLA', 'Tesla Inc.', 'NASDAQ', 'Consumer Cyclical', 'Auto Manufacturers', 900000000000, now(), now()),
('META', 'Meta Platforms Inc.', 'NASDAQ', 'Technology', 'Social Media', 1200000000000, now(), now()),
('NVDA', 'NVIDIA Corporation', 'NASDAQ', 'Technology', 'Semiconductors', 1500000000000, now(), now()),
('NFLX', 'Netflix Inc.', 'NASDAQ', 'Communication Services', 'Streaming', 250000000000, now(), now());
"

curl -X POST "http://${CLICKHOUSE_HOST}:${CLICKHOUSE_PORT}" \
    --data-urlencode "query=${SAMPLE_DATA_SQL}" \
    --user "${CLICKHOUSE_USER}:${CLICKHOUSE_PASSWORD}"

# 插入一些示例市场数据
echo -e "${YELLOW}📈 插入示例市场数据...${NC}"
MARKET_DATA_SQL="
INSERT INTO market_data (timestamp, symbol_id, symbol, open_price, high_price, low_price, close_price, adj_close_price, volume, source) VALUES
('2024-01-01 09:30:00', 1, 'AAPL', 180.50, 181.25, 180.10, 181.00, 181.00, 1000000, 'simulation'),
('2024-01-01 09:31:00', 1, 'AAPL', 181.00, 181.50, 180.75, 181.25, 181.25, 800000, 'simulation'),
('2024-01-01 09:30:00', 2, 'GOOGL', 150.25, 150.75, 149.90, 150.50, 150.50, 500000, 'simulation'),
('2024-01-01 09:31:00', 2, 'GOOGL', 150.50, 151.00, 150.25, 150.90, 150.90, 600000, 'simulation');
"

curl -X POST "http://${CLICKHOUSE_HOST}:${CLICKHOUSE_PORT}" \
    --data-urlencode "query=${MARKET_DATA_SQL}" \
    --user "${CLICKHOUSE_USER}:${CLICKHOUSE_PASSWORD}"

# 测试查询
echo -e "${YELLOW}🧪 测试数据库连接和查询...${NC}"
TEST_QUERY="SELECT COUNT(*) as symbol_count FROM symbols"

RESULT=$(curl -s -G "http://${CLICKHOUSE_HOST}:${CLICKHOUSE_PORT}" \
    --data-urlencode "query=${TEST_QUERY}" \
    --user "${CLICKHOUSE_USER}:${CLICKHOUSE_PASSWORD}")

echo -e "${GREEN}✅ 测试查询结果:${NC}"
echo "$RESULT"

# 显示数据库统计信息
echo -e "${YELLOW}📊 数据库统计信息:${NC}"
STATS_QUERIES=(
    "SELECT 'Symbols' as table_name, COUNT(*) as count FROM symbols"
    "SELECT 'Market Data' as table_name, COUNT(*) as count FROM market_data"
    "SELECT 'Technical Indicators' as table_name, COUNT(*) as count FROM technical_indicators"
)

for query in "${STATS_QUERIES[@]}"; do
    curl -s -G "http://${CLICKHOUSE_HOST}:${CLICKHOUSE_PORT}" \
        --data-urlencode "query=${query}" \
        --user "${CLICKHOUSE_USER}:${CLICKHOUSE_PASSWORD}"
done

# 创建健康检查端点
echo -e "${YELLOW}💓 创建健康检查...${NC}"
HEALTH_CHECK="SELECT 'healthy' as status, now() as timestamp, version() as clickhouse_version"

HEALTH_RESULT=$(curl -s -G "http://${CLICKHOUSE_HOST}:${CLICKHOUSE_PORT}" \
    --data-urlencode "query=${HEALTH_CHECK}" \
    --user "${CLICKHOUSE_USER}:${CLICKHOUSE_PASSWORD}")

echo -e "${GREEN}✅ 健康检查结果:${NC}"
echo "$HEALTH_RESULT"

echo ""
echo -e "${GREEN}🎉 ClickHouse 初始化完成！${NC}"
echo ""
echo -e "${BLUE}📋 连接信息:${NC}"
echo -e "   主机: ${CLICKHOUSE_HOST}"
echo -e "   端口: ${CLICKHOUSE_PORT}"
echo -e "   数据库: ${CLICKHOUSE_DATABASE}"
echo -e "   用户: ${CLICKHOUSE_USER}"
echo ""
echo -e "${BLUE}🔗 访问 URL:${NC}"
echo -e "   HTTP 接口: http://${CLICKHOUSE_HOST}:${CLICKHOUSE_PORT}"
echo -e "   Web 界面: http://${CLICKHOUSE_HOST}:${CLICKHOUSE_PORT}/play"
echo ""
echo -e "${YELLOW}💡 提示: 使用以下命令连接到 ClickHouse:${NC}"
echo -e "   clickhouse-client --host ${CLICKHOUSE_HOST} --port ${CLICKHOUSE_PORT} --user ${CLICKHOUSE_USER} --password ${CLICKHOUSE_PASSWORD} --database ${CLICKHOUSE_DATABASE}"