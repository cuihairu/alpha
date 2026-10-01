# 构建工具链与 workspace 实测版本对齐（rustc 1.98.1，Docker Hub 现行最新；更低版本解析不了新 edition 的传递依赖）
FROM rust:1.98.1-slim-bookworm AS builder

# 安装必要的系统依赖（protobuf-compiler：alpha-protocols 默认 grpc 跑 tonic-build 代码生成；g++：clickhouse cityhash 等 C/C++ 依赖，rust:slim 默认无）
RUN apt-get update && apt-get install -y \
    pkg-config \
    g++ \
    libssl-dev \
    libpq-dev \
    protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

# 设置工作目录
WORKDIR /app

# 复制完整 workspace（workspace 成员清单在根 Cargo.toml，其他成员的 manifest 也必须存在才能解析）
COPY . .

# 构建数据处理引擎
RUN cargo build --release -p alpha-data-engine

# 运行时镜像（bookworm 与 rust:1.98.1-slim-bookworm 构建环境同基底：glibc 向下不兼容，bullseye 会缺符号）
FROM debian:bookworm-slim

# 安装运行时依赖
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libpq5 \
    curl \
    && rm -rf /var/lib/apt/lists/*

# 创建非 root 用户
RUN useradd -r -s /bin/false alpha

# 复制编译好的二进制文件
COPY --from=builder /app/target/release/alpha-data-engine /usr/local/bin/

# 设置用户
USER alpha

# 暴露端口
EXPOSE 8081

# 健康检查
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:8081/health || exit 1

# 启动命令
CMD ["alpha-data-engine"]