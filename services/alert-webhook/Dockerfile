# Alert Webhook 服务 Dockerfile（L464）
# 多阶段构建：builder 阶段编译，runtime 阶段仅运行二进制（体积最小化）

FROM rust:1.97-slim AS builder

WORKDIR /app

# 安装构建依赖
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# 预缓存依赖：先拷贝 Cargo.toml/Cargo.lock，再拷贝源码
COPY Cargo.toml Cargo.lock ./
COPY services/alert-webhook/Cargo.toml services/alert-webhook/
# 创建空 src 以让 cargo fetch 不报错
RUN mkdir -p services/alert-webhook/src && echo "fn main() {}" > services/alert-webhook/src/main.rs
RUN cargo fetch --manifest-path services/alert-webhook/Cargo.toml

# 拷贝全量源码并构建
COPY . .
RUN cargo build --release --bin alpha-alert-webhook

# Runtime 阶段：distroless 或 debian-slim
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# 从 builder 拷贝二进制
COPY --from=builder /app/target/release/alpha-alert-webhook /app/alpha-alert-webhook

# 非 root 用户
RUN useradd -r -s /bin/false appuser && chown appuser:appuser /app/alpha-alert-webhook
USER appuser

EXPOSE 8080

ENV RUST_LOG=info

ENTRYPOINT ["/app/alpha-alert-webhook"]