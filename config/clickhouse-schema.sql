-- Alpha Finance ClickHouse 数据库架构
-- 专为高性能金融数据分析优化的列式存储设计

-- 创建数据库
CREATE DATABASE IF NOT EXISTS alpha_finance;
USE alpha_finance;

-- ===============================
-- 1. 股票基础信息表 (维度表)
-- ===============================
CREATE TABLE IF NOT EXISTS symbols (
    symbol String CODEC(ZSTD(1)),
    name String CODEC(ZSTD(1)),
    exchange String CODEC(ZSTD(1)),
    sector String CODEC(ZSTD(1)),
    industry String CODEC(ZSTD(1)),
    market_cap UInt64 CODEC(ZSTD(3)),
    created_at DateTime CODEC(ZSTD(3)),
    updated_at DateTime CODEC(ZSTD(3))
) ENGINE = MergeTree()
ORDER BY symbol
SETTINGS index_granularity = 8192;

-- ===============================
-- 2. 市场数据表 (核心事实表)
-- ===============================
CREATE TABLE IF NOT EXISTS market_data (
    -- 时间维度 (分区键)
    timestamp DateTime CODEC(DoubleDelta, ZSTD(3)),
    date Date MATERIALIZED toDate(timestamp),

    -- 股票标识
    symbol_id UInt32 CODEC(ZSTD(3)),
    symbol String CODEC(ZSTD(1)),

    -- OHLCV 数据
    open_price Decimal64(2) CODEC(Gorilla, ZSTD(3)),
    high_price Decimal64(2) CODEC(Gorilla, ZSTD(3)),
    low_price Decimal64(2) CODEC(Gorilla, ZSTD(3)),
    close_price Decimal64(2) CODEC(Gorilla, ZSTD(3)),
    adj_close_price Decimal64(2) CODEC(Gorilla, ZSTD(3)),
    volume UInt64 CODEC(ZSTD(3)),

    -- 计算字段
    price_change Decimal64(2) MATERIALIZED close_price - open_price,
    price_change_percent Float64 MATERIALIZED round((price_change / open_price) * 100, 2),

    -- 元数据
    source String CODEC(ZSTD(1)),
    created_at DateTime MATERIALIZED now()
) ENGINE = MergeTree()
PARTITION BY toYYYYMM(timestamp)
ORDER BY (symbol_id, timestamp)
TTL timestamp + toIntervalYear(5)
SETTINGS index_granularity = 8192;

-- ===============================
-- 3. 技术指标表
-- ===============================
CREATE TABLE IF NOT EXISTS technical_indicators (
    timestamp DateTime CODEC(DoubleDelta, ZSTD(3)),
    date Date MATERIALIZED toDate(timestamp),
    symbol_id UInt32 CODEC(ZSTD(3)),
    symbol String CODEC(ZSTD(1)),

    -- 指标基础信息
    indicator_name String CODEC(ZSTD(1)),
    period UInt16 CODEC(ZSTD(3)),

    -- 指标值 (使用 Decimal64 保证精度)
    value Decimal64(6) CODEC(Gorilla, ZSTD(3)),

    -- 可选的额外指标值 (如布林带上下轨)
    value_upper Nullable(Decimal64(6)) CODEC(Gorilla, ZSTD(3)),
    value_lower Nullable(Decimal64(6)) CODEC(Gorilla, ZSTD(3)),

    -- 元数据
    calculation_time DateTime MATERIALIZED now(),
    source String CODEC(ZSTD(1))
) ENGINE = MergeTree()
PARTITION BY (toYYYYMM(timestamp), indicator_name)
ORDER BY (symbol_id, indicator_name, period, timestamp)
TTL timestamp + toIntervalYear(2)
SETTINGS index_granularity = 8192;

-- ===============================
-- 4. 实时行情表 (最新数据缓存)
-- ===============================
CREATE TABLE IF NOT EXISTS realtime_quotes (
    symbol String CODEC(ZSTD(1)),
    last_price Decimal64(2) CODEC(Gorilla, ZSTD(3)),
    bid_price Decimal64(2) CODEC(Gorilla, ZSTD(3)),
    ask_price Decimal64(2) CODEC(Gorilla, ZSTD(3)),
    volume UInt64 CODEC(ZSTD(3)),
    timestamp DateTime CODEC(DoubleDelta, ZSTD(3)),
    change_amount Decimal64(2) CODEC(Gorilla, ZSTD(3)),
    change_percent Float64 CODEC(Gorilla, ZSTD(3)),

    -- 使用 ReplacingMergeTree 自动更新同一股票的最新数据
    updated_at DateTime MATERIALIZED now()
) ENGINE = ReplacingMergeTree()
ORDER BY symbol
TTL timestamp + toIntervalDay(1)
SETTINGS index_granularity = 1024;

-- ===============================
-- 5. 用户分析结果表
-- ===============================
CREATE TABLE IF NOT EXISTS user_analysis (
    user_id String CODEC(ZSTD(1)),
    session_id String CODEC(ZSTD(1)),
    symbol String CODEC(ZSTD(1)),
    analysis_type String CODEC(ZSTD(1)),

    -- 分析结果 (JSON 格式存储复杂结果)
    result_json String CODEC(ZSTD(1)),

    -- 置信度评分 (0-100)
    confidence_score UInt8 CODEC(ZSTD(3)),

    -- 分析时间
    analysis_time DateTime CODEC(DoubleDelta, ZSTD(3)),
    created_at DateTime MATERIALIZED now()
) ENGINE = MergeTree()
PARTITION BY toYYYYMM(analysis_time)
ORDER BY (user_id, analysis_type, analysis_time)
TTL analysis_time + toIntervalMonth(3)
SETTINGS index_granularity = 8192;

-- ===============================
-- 6. 系统日志表 (用于监控和调试)
-- ===============================
CREATE TABLE IF NOT EXISTS system_logs (
    timestamp DateTime CODEC(DoubleDelta, ZSTD(3)),
    level String CODEC(ZSTD(1)),
    service String CODEC(ZSTD(1)),
    message String CODEC(ZSTD(1)),
    context String CODEC(ZSTD(1)),

    -- 性能指标
    execution_time_ms Nullable(UInt32) CODEC(ZSTD(3)),
    memory_usage_mb Nullable(UInt32) CODEC(ZSTD(3)),

    created_at DateTime MATERIALIZED now()
) ENGINE = MergeTree()
PARTITION BY toYYYYMMDD(timestamp)
ORDER BY (timestamp, level, service)
TTL timestamp + toIntervalMonth(1)
SETTINGS index_granularity = 8192;

-- ===============================
-- 创建物化视图 (实时聚合)
-- ===============================

-- 1分钟 K线数据聚合
CREATE MATERIALIZED VIEW IF NOT EXISTS market_data_1m TO market_data_1m AS
SELECT
    toStartOfMinute(timestamp) as timestamp,
    symbol_id,
    symbol,
    first(open_price) as open_price,
    max(high_price) as high_price,
    min(low_price) as low_price,
    last(close_price) as close_price,
    sum(volume) as volume,
    any(source) as source
FROM market_data
GROUP BY symbol_id, symbol, toStartOfMinute(timestamp);

-- 创建1分钟聚合结果表
CREATE TABLE IF NOT EXISTS market_data_1m (
    timestamp DateTime CODEC(DoubleDelta, ZSTD(3)),
    date Date MATERIALIZED toDate(timestamp),
    symbol_id UInt32 CODEC(ZSTD(3)),
    symbol String CODEC(ZSTD(1)),
    open_price Decimal64(2) CODEC(Gorilla, ZSTD(3)),
    high_price Decimal64(2) CODEC(Gorilla, ZSTD(3)),
    low_price Decimal64(2) CODEC(Gorilla, ZSTD(3)),
    close_price Decimal64(2) CODEC(Gorilla, ZSTD(3)),
    volume UInt64 CODEC(ZSTD(3)),
    source String CODEC(ZSTD(1)),
    created_at DateTime MATERIALIZED now()
) ENGINE = AggregatingMergeTree()
PARTITION BY toYYYYMM(timestamp)
ORDER BY (symbol_id, timestamp)
TTL timestamp + toIntervalYear(2)
SETTINGS index_granularity = 8192;

-- 5分钟 K线数据聚合
CREATE MATERIALIZED VIEW IF NOT EXISTS market_data_5m TO market_data_5m AS
SELECT
    toStartOfFiveMinutes(timestamp) as timestamp,
    symbol_id,
    symbol,
    first(open_price) as open_price,
    max(high_price) as high_price,
    min(low_price) as low_price,
    last(close_price) as close_price,
    sum(volume) as volume,
    any(source) as source
FROM market_data
GROUP BY symbol_id, symbol, toStartOfFiveMinutes(timestamp);

-- 创建5分钟聚合结果表
CREATE TABLE IF NOT EXISTS market_data_5m (
    timestamp DateTime CODEC(DoubleDelta, ZSTD(3)),
    date Date MATERIALIZED toDate(timestamp),
    symbol_id UInt32 CODEC(ZSTD(3)),
    symbol String CODEC(ZSTD(1)),
    open_price Decimal64(2) CODEC(Gorilla, ZSTD(3)),
    high_price Decimal64(2) CODEC(Gorilla, ZSTD(3)),
    low_price Decimal64(2) CODEC(Gorilla, ZSTD(3)),
    close_price Decimal64(2) CODEC(Gorilla, ZSTD(3)),
    volume UInt64 CODEC(ZSTD(3)),
    source String CODEC(ZSTD(1)),
    created_at DateTime MATERIALIZED now()
) ENGINE = AggregatingMergeTree()
PARTITION BY toYYYYMM(timestamp)
ORDER BY (symbol_id, timestamp)
TTL timestamp + toIntervalYear(3)
SETTINGS index_granularity = 8192;

-- ===============================
-- 插入示例数据
-- ===============================

-- 插入股票基础信息
INSERT INTO symbols (symbol, name, exchange, sector, industry, market_cap, created_at, updated_at) VALUES
('AAPL', 'Apple Inc.', 'NASDAQ', 'Technology', 'Consumer Electronics', 3000000000000, now(), now()),
('GOOGL', 'Alphabet Inc.', 'NASDAQ', 'Technology', 'Search Engine', 2000000000000, now(), now()),
('MSFT', 'Microsoft Corporation', 'NASDAQ', 'Technology', 'Software', 2800000000000, now(), now()),
('AMZN', 'Amazon.com Inc.', 'NASDAQ', 'Consumer Cyclical', 'E-Commerce', 1800000000000, now(), now()),
('TSLA', 'Tesla Inc.', 'NASDAQ', 'Consumer Cyclical', 'Auto Manufacturers', 900000000000, now(), now()),
('META', 'Meta Platforms Inc.', 'NASDAQ', 'Technology', 'Social Media', 1200000000000, now(), now()),
('NVDA', 'NVIDIA Corporation', 'NASDAQ', 'Technology', 'Semiconductors', 1500000000000, now(), now()),
('NFLX', 'Netflix Inc.', 'NASDAQ', 'Communication Services', 'Streaming', 250000000000, now(), now());

-- 创建用户管理表 (兼容现有功能)
CREATE TABLE IF NOT EXISTS users (
    user_id String CODEC(ZSTD(1)),
    username String CODEC(ZSTD(1)),
    email String CODEC(ZSTD(1)),
    created_at DateTime CODEC(DoubleDelta, ZSTD(3)),
    last_login DateTime CODEC(DoubleDelta, ZSTD(3))
) ENGINE = MergeTree()
ORDER BY user_id
SETTINGS index_granularity = 8192;

-- ===============================
-- 创建索引优化查询性能
-- ===============================

-- 为高频查询添加二级索引 (ClickHouse 22.8+)
-- ALTER TABLE market_data ADD INDEX idx_symbol_change (symbol, price_change_percent) TYPE minmax GRANULARITY 1;
-- ALTER TABLE technical_indicators ADD INDEX idx_symbol_indicator (symbol, indicator_name) TYPE set(100) GRANULARITY 1;

-- ===============================
-- 配置文件优化设置
-- ===============================

-- 设置全局配置
SET max_memory_usage = 10000000000;  -- 10GB
SET max_threads = 8;
SET max_insert_threads = 4;

-- 创建用于连接的只读用户
-- CREATE USER IF NOT EXISTS alpha_readonly IDENTIFIED WITH plaintext_password BY 'readonly123'
-- SETTINGS readonly = 1;

-- 创建用于写入的用户
-- CREATE USER IF NOT EXISTS alpha_writer IDENTIFIED WITH plaintext_password BY 'writer123'
-- SETTINGS max_memory_usage = 20000000000;