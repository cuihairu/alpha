-- Alpha Finance 数据库初始化脚本

-- 启用 TimescaleDB 扩展
CREATE EXTENSION IF NOT EXISTS timescaledb;

-- 创建用户表
CREATE TABLE IF NOT EXISTS users (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    username VARCHAR(50) UNIQUE NOT NULL,
    email VARCHAR(100) UNIQUE NOT NULL,
    password_hash VARCHAR(255) NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

-- 创建股票基础信息表
CREATE TABLE IF NOT EXISTS symbols (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    symbol VARCHAR(10) UNIQUE NOT NULL,
    name VARCHAR(100) NOT NULL,
    exchange VARCHAR(10) NOT NULL,
    sector VARCHAR(50),
    industry VARCHAR(100),
    market_cap BIGINT,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

-- 创建市场数据表（时序数据）
CREATE TABLE IF NOT EXISTS market_data (
    time TIMESTAMP WITH TIME ZONE NOT NULL,
    symbol_id UUID NOT NULL REFERENCES symbols(id),
    open_price DECIMAL(10,2) NOT NULL,
    high_price DECIMAL(10,2) NOT NULL,
    low_price DECIMAL(10,2) NOT NULL,
    close_price DECIMAL(10,2) NOT NULL,
    volume BIGINT NOT NULL,
    adj_close DECIMAL(10,2),
    PRIMARY KEY (time, symbol_id)
);

-- 将市场数据表转换为 TimescaleDB 超表
SELECT create_hypertable('market_data', 'time', chunk_time_interval => INTERVAL '1 day');

-- 创建技术指标表
CREATE TABLE IF NOT EXISTS technical_indicators (
    time TIMESTAMP WITH TIME ZONE NOT NULL,
    symbol_id UUID NOT NULL REFERENCES symbols(id),
    indicator_name VARCHAR(50) NOT NULL,
    period INTEGER NOT NULL,
    value DECIMAL(10,4) NOT NULL,
    PRIMARY KEY (time, symbol_id, indicator_name, period)
);

-- 将技术指标表转换为 TimescaleDB 超表
SELECT create_hypertable('technical_indicators', 'time', chunk_time_interval => INTERVAL '1 day');

-- 创建分析结果表
CREATE TABLE IF NOT EXISTS analysis_results (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    symbol_id UUID NOT NULL REFERENCES symbols(id),
    analysis_type VARCHAR(50) NOT NULL,
    result_data JSONB NOT NULL,
    confidence_score DECIMAL(3,2),
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

-- 创建用户关注列表
CREATE TABLE IF NOT EXISTS watchlists (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id),
    symbol_id UUID NOT NULL REFERENCES symbols(id),
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    UNIQUE(user_id, symbol_id)
);

-- 创建告警表
CREATE TABLE IF NOT EXISTS alerts (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id),
    symbol_id UUID NOT NULL REFERENCES symbols(id),
    alert_type VARCHAR(50) NOT NULL,
    condition_data JSONB NOT NULL,
    is_active BOOLEAN DEFAULT TRUE,
    triggered_at TIMESTAMP WITH TIME ZONE,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

-- 插入示例股票数据
INSERT INTO symbols (symbol, name, exchange, sector, industry) VALUES
('AAPL', 'Apple Inc.', 'NASDAQ', 'Technology', 'Consumer Electronics'),
('GOOGL', 'Alphabet Inc.', 'NASDAQ', 'Technology', 'Search Engine'),
('MSFT', 'Microsoft Corporation', 'NASDAQ', 'Technology', 'Software'),
('AMZN', 'Amazon.com Inc.', 'NASDAQ', 'Consumer Cyclical', 'E-Commerce'),
('TSLA', 'Tesla Inc.', 'NASDAQ', 'Consumer Cyclical', 'Auto Manufacturers'),
('META', 'Meta Platforms Inc.', 'NASDAQ', 'Technology', 'Social Media'),
('NVDA', 'NVIDIA Corporation', 'NASDAQ', 'Technology', 'Semiconductors'),
('NFLX', 'Netflix Inc.', 'NASDAQ', 'Communication Services', 'Streaming')
ON CONFLICT (symbol) DO NOTHING;

-- 创建索引
CREATE INDEX IF NOT EXISTS idx_market_data_symbol_time ON market_data (symbol_id, time DESC);
CREATE INDEX IF NOT EXISTS idx_technical_indicators_symbol_time ON technical_indicators (symbol_id, time DESC);
CREATE INDEX IF NOT EXISTS idx_analysis_results_symbol_type ON analysis_results (symbol_id, analysis_type);
CREATE INDEX IF NOT EXISTS idx_watchlists_user_id ON watchlists (user_id);
CREATE INDEX IF NOT EXISTS idx_alerts_user_active ON alerts (user_id, is_active);

-- 创建更新时间戳的函数
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ language 'plpgsql';

-- 创建触发器
CREATE TRIGGER update_users_updated_at BEFORE UPDATE ON users
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_symbols_updated_at BEFORE UPDATE ON symbols
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();