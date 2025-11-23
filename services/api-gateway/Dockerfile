FROM rust:1.75-slim as builder

# 安装必要的系统依赖
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# 设置工作目录
WORKDIR /app

# 复制 Cargo 文件
COPY Cargo.toml Cargo.lock ./
COPY packages/ ./packages/
COPY services/api-gateway ./services/api-gateway/

# 构建 API 网关
RUN cargo build --release -p alpha-api-gateway

# 运行时镜像
FROM debian:bullseye-slim

# 安装运行时依赖
RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# 创建非 root 用户
RUN useradd -r -s /bin/false alpha

# 复制编译好的二进制文件
COPY --from=builder /app/target/release/alpha-api-gateway /usr/local/bin/

# 设置用户
USER alpha

# 暴露端口
EXPOSE 8080

# 健康检查
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:8080/health || exit 1

# 启动命令
CMD ["alpha-api-gateway"]