# 跨平台 Docker 镜像与容器化部署方案（TODO L468）

> 范围：Rust 服务镜像的多架构构建、运行时安全与编排接线。CI 发布流水线归 L470，
> 应用商店/安装包归 L471，Web CDN 归 L515。

## 1. 镜像矩阵

服务清单**自动发现** = `services/*/Dockerfile`（新增服务写好 Dockerfile 即纳入构建脚本与本文档口径）。

| 服务 | 端口 | HEALTHCHECK | 运行时依赖 |
|---|---|---|---|
| api-gateway | 8080 | `curl -f :8080/health` | ca-certificates, curl |
| data-engine | 8081 | `curl -f :8081/health` | ca-certificates, curl, libpq5 |
| real-time-feed | 8082 | `curl -f :8082/health` | ca-certificates, curl |
| collector | 8083 | `curl -f :8083/health` | ca-certificates, curl, libpq5 |
| alert-webhook | 8080（compose 映射 8084） | `curl -f :8080/health` | ca-certificates, libssl3, curl |

（基础设施镜像 timescaledb/clickhouse/redis/prometheus/grafana 等走上游官方多架构镜像，见 `docker-compose.yml`。）

## 2. 构建体系：多阶段 + 同基底

每个服务 Dockerfile 两阶段：

1. **builder `rust:1.98.1-slim-bookworm`**：与 workspace 实测工具链同 minor（本地 `rustc 1.99.0`、
   Docker Hub 现行最新 1.98.1，已用 `cargo +1.98.1 check --workspace` 实证向后兼容；1.99 镜像发布后随
   例行升级跟进）——过旧的钉定（原 1.75）解析不了新 edition 的传递依赖；`cargo build --release -p <crate>` 只编目标服务。
   - `protobuf-compiler`：`alpha-protocols` 默认 `grpc` 特性跑 tonic-build proto 代码生成
     （collector 不依赖 alpha-protocols，无需装）。
2. **runtime `debian:bookworm-slim`**：与 builder **同基底**——builder 产物链接 bookworm 的
   glibc 2.36，运行时若降级到 bullseye（2.31）会直接符号查找失败；反向升级运行时则浪费。
   非 root 用户 `alpha` + `HEALTHCHECK` 探 `/health`（curl 显式入运行时依赖，slim 基底默认没有）。

官方 `rust`/`debian` 均为多架构镜像（linux/amd64 + linux/arm64），阶段内无架构假设，
因此**同一 Dockerfile 天然支持多平台构建**，无需按架构分叉。

## 3. 构建入口：`scripts/build-images.sh`

```bash
scripts/build-images.sh                                  # 全部服务，当前平台构建
scripts/build-images.sh --only api-gateway               # 单服务快速验证
scripts/build-images.sh --platforms linux/amd64,linux/arm64 \
    --push --registry ghcr.io/<owner>                    # 多平台构建并推送（需 buildx）
```

约定：

- 镜像名 `[<registry>/]alpha-<服务短名>`，tag 默认 git 短 SHA（`TAG=` 可覆写，无 git 元数据退化为 `local`），
  与 `docker-compose.yml` 内 `build:` 出的镜像命名一致；
- **多平台与 `--push` 强制 buildx**，缺插件直接失败提示（不静默降级成单架构冒充多架构），
  `--push` 必须带 registry 前缀（拒绝推到无名仓库）；
- buildx 单平台默认 `--load` 进本地镜像库，compose 可直接引用；
- 任一服务失败 → 汇总失败清单非零退出（不因首个失败短路，便于一次看全）。

多平台构建依赖 buildx + binfmt/QEMU（`docker run --privileged tonistiigi/binfmt` 或
`docker buildx create --use` 配置后即可用）；本机无 buildx 时本地单平台构建不受影响。

## 4. 编排接线

- **开发/测试栈** = `docker-compose.yml`：`build: context: .` 直接消费上述 Dockerfile，
  服务间走 `alpha-network`，`restart: unless-stopped`，健康检查随镜像 `HEALTHCHECK` 生效；
- **生产建议**（本文档登记，不在 compose 硬编码）：镜像按 git SHA 出库而非现场 `build:`；
  密钥走 secrets/env 注入（`.env` 已在 `.dockerignore` 排除）；为各服务补 `deploy.resources`
  或编排层限额；数据库/可观测性组件用托管或独立卷持久化（`config/` 挂载点已具备）。

## 5. 已知边界（登记为后续项）

- **Cargo.lock 未入库**：Dockerfile 内 `cargo build` 每次解析最新兼容依赖，构建不可完全复现
  （当前 `.dockerignore` 不排除本地 lockfile，本地构建尚可沾光）。锁文件入库策略归质量保证节统筹；
- **依赖层缓存**：`COPY . .` 后源码任意变更即整编（cargo-chef 分层可解，构建耗时优化归后续）；
- **CI 镜像构建接线**（buildx push 进 L470 发布流水线，PR 阶段只验单平台 build 不入门禁）；
- **镜像扫描/SBOM/签名** 随 L470 供应链环节统筹；本机验证入口 = `scripts/build-images.sh --only <服务>`。
