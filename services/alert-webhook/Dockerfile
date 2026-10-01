# Alert Webhook 服务 Dockerfile（L464；构建体系并入 L468 统一口径，见 docs/docker-deployment.md）
# 多阶段构建：builder 阶段编译，runtime 阶段仅运行二进制（体积最小化）

# 构建工具链与 workspace 实测版本对齐（rustc 1.98.1，Docker Hub 现行最新；
# 更低版本解析不了新 edition 的传递依赖；本服务无 alpha-protocols/grpc 与 C++ 依赖，无需 protoc/g++）
FROM rust:1.98.1-slim-bookworm AS builder

# 安装构建依赖
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# 复制完整 workspace：根 Cargo.toml 的成员清单引用全部 member，
# 局部 COPY 成员 manifest 的预缓存做法会让 cargo 在解析阶段直接失败（实测教训）
COPY . .

RUN cargo build --release -p alpha-alert-webhook

# Runtime 阶段（bookworm 与构建环境同基底：glibc 匹配）
FROM debian:bookworm-slim

# ca-certificates/libssl3：出站 HTTPS 通知转发；curl：HEALTHCHECK 探 /health
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 \
    curl \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# 从 builder 拷贝二进制
COPY --from=builder /app/target/release/alpha-alert-webhook /app/alpha-alert-webhook

# 非 root 用户
RUN useradd -r -s /bin/false appuser && chown appuser:appuser /app/alpha-alert-webhook
USER appuser

EXPOSE 8080

ENV RUST_LOG=info

# 健康检查
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:8080/health || exit 1

ENTRYPOINT ["/app/alpha-alert-webhook"]
