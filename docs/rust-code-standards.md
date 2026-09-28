# Rust 代码规范

> 状态 v1（2026-09-28），对应 TODO「定义统一的 Rust 代码规范和跨平台兼容性检查」。
> 所有规则均可在本地/CI 由脚本强制（见 §11），不以评审口头约定为准。
> 适用范围：整个 Cargo workspace（CI 门禁与测试门禁同口径排除 `alpha-desktop`，
> 桌面 crate 随 Tauri 工具链另行约束）。

## 1. 格式化：rustfmt 是唯一格式化权威

* 提交前 `cargo fmt --all`，门禁 `cargo fmt --all -- --check`（差异即失败）。
* 仓库根 `rustfmt.toml` 声明了部分 nightly-only 选项（wrap_comments 等）：
  stable 工具链忽略并告警，属预期；换用 nightly 工具链时自动生效。
* 已知风险：不同 rustfmt 版本输出可能有漂移；如出现，引入 `rust-toolchain.toml`
  统一工具链版本（本规范不预设，出现即决策）。

## 2. Clippy：零警告，allow 必须留痕

* 门禁：`cargo clippy --workspace --all-targets --exclude alpha-desktop -- -D warnings`。
* 任何 `#[allow(...)]` 必须附同行注释说明理由（示例见
  `wasm-analyzer/src/worker.rs` 的路线图预留 API、
  `services/collector/src/scheduler.rs` 的共享状态句柄传参）。
* 禁止 crate 级一把梭 `#![allow(...)]`、禁止 `#![deny(warnings)]`（避免工具链
  升级时把新 lint 变成编译失败；统一由 CI `-D warnings` 把关）。
* lint 名称以 clippy 当前版本为准（曾把 `result_large_err` 误写为
  `large_enum_variant`，编译期不报错、压制不生效——allow 后必须实际复跑验证）。
* 代码生成产物（如 tonic 生成 proto）不在人工改造范围，在 include 处统一 allow。

## 3. 错误处理

* packages 层一律 `AlphaResult<T>` / `AlphaError`（`packages/core/src/errors.rs`），
  带上下文的错误用 `format!` 补充语义（如 `StorageError(format!("redis XADD failed: {e}"))`）。
* services 二进制层可用 `anyhow::Result`；对外 HTTP 边界必须转结构化 JSON 错误。
* 库代码禁 `unwrap()` / `expect()` / `panic!`（测试代码豁免）。
  锁中毒用 `map_err(|_| ...poisoned...)` 模式（见 `packages/core/src/platform.rs`）。
* 「外部系统不可用」不是 panic 理由：按三态降级口径处理（未启用 / 降级 /
  正常，参考 data-engine 的 persistence/ClickHouse 装配）。

## 4. 命名与模块挂载

* 模块声明按字母序排列于 `lib.rs`；**新增文件必须挂载**——历史上出现过
  `core/src/trading.rs`、collector 六模块等从未挂载、从未编译的孤儿文件
  （内含潜伏语法错误数年未暴露，已于 2026-09-28 清理）。
* 类型/函数命名遵循 Rust 官方惯例；测试函数用行为式命名
  （`claim_stale_caps_redelivery_and_routes_poison_to_dlq` 风格）。

## 5. 异步

* 全仓单运行时 tokio；service 入口 `#[tokio::main]`，测试 `#[tokio::test]`
  （对应 crate 需 dev-dep `tokio = { features = ["macros", "rt"] }`，不进 lib 构建）。
* 需要对象安全的异步 trait 用 `#[async_trait::async_trait]`
  （见 `packages/core/src/platform.rs`、`trading` 惯例）。
* 消费循环、后台任务的退出条件与 ack 语义必须显式（参考 Redis Streams
  消费者的「处理成功才 ack、DLQ 失败不 ack」契约）。

## 6. 日志

* 统一 `tracing`；级别语义：
  * `error`：需要人介入的故障；
  * `warn`：降级、毒消息、DLQ、重试（可自动恢复但需留痕）；
  * `info`：服务生命周期、装配结果（含降级原因）；
  * `debug`：热路径与逐条消息。
* 服务启动必须显式初始化订阅器并设默认级别（`ALPHA_LOG_LEVEL` → `RUST_LOG`
  → `info`），禁止依赖 `fmt::init()` 的隐式 ERROR-only 行为（历史教训：
  未设 RUST_LOG 时 WARN 级兜底日志不可见）。
* 关键路径带结构化字段（`stream = %stream, entry_id = %id, delivery_count`）。

## 7. 平台与依赖（详见 docs/cross-platform-architecture.md）

* L0（packages/core、packages/protocols 契约层）依赖黑名单：
  tokio/reqwest/sqlx/redis/tonic/native-tls/rustls/notify/directories/dirs——
  新增平台能力先放 L1 或加 feature 门控；
* 平台分支统一 `cfg(target_arch = "wasm32")` / `cfg(target_os = ...)`；
* feature 默认值取「服务端全功能」：`default = ["grpc"]` 使服务端零改动，
  wasm 侧显式 `--no-default-features` 取契约层；
* 门禁：`scripts/check-cross-platform.sh`（wasm32 编译 + L0 黑名单扫描）。

## 8. 测试

* 单测贴近被测代码（`#[cfg(test)] mod tests`）；跨进程/跨服务行为放
  `tests/` 集成测试。
* 依赖外部系统的测试必须 env 门控 + 无环境自跳过：
  `REDIS_TEST_URL` / `TIMESCALE_TEST_URL`（门禁命令见 §11）。
* 共性协议/数据结构用契约测试 helper（`assert_key_value_store_contract` 模式：
  一个函数断言所有实现都满足的语义，逐实现复用）。
* 断言必须有信息量：`assert!(true)` 式空断言等于没有测试（review 即删）。
* 二进制行为变更后须用重建的二进制做进程级冒烟（`cargo test` 不重编 bin target，
  先 `cargo build -p <svc>`——历史上多次踩坑）。

## 9. 注释与文档

* 注释与文档用中文，解释「为什么」而非复述代码；公共项必须有 `///`。
* 架构级决策落在 `docs/`（cross-platform-architecture.md、本文件），
  TODO.md 记录进度与依据，二者在 commit message 中互相引用。

## 10. 提交纪律

* 一个任务一个 commit，push 前先 `git fetch origin main && git rebase origin/main`；
* commit message 首行 `<type>(<scope>): <要点>`，正文列变更与依据；
* 禁 tag / release / force push。

## 11. 门禁命令（本地与 CI 完全一致）

```bash
# Rust 规范门禁（fmt + clippy）
scripts/check-lint.sh
# 等价于：
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --exclude alpha-desktop -- -D warnings

# 跨平台门禁（wasm32 编译 + L0 依赖黑名单）
scripts/check-cross-platform.sh

# 全量测试（Redis 门控测试需本机 Redis；无则自跳过）
REDIS_TEST_URL=redis://127.0.0.1:6379 \
  cargo test --workspace --all-targets --exclude alpha-desktop
```

CI（.github/workflows/ci.yml）：`lint` 作业 = §11 前两命令；
`test` 作业 = 作业内 Redis service + `protobuf-compiler` 安装后跑全量测试；
`wasm` 作业 = `scripts/check-cross-platform.sh` + wasm-pack 构建；
`security` 作业 = cargo audit（**报告型**，`continue-on-error: true` 不阻塞——
现存 8 个 cargo 依赖漏洞属依赖升级债，另行立项，避免长期红灯淹没真信号）。

## 12. 已知债务（按立项顺序消化）

* cargo audit 8 个依赖漏洞、docs/ npm 侧 ~102 个 dependabot 告警：依赖升级专项；
* data-engine/real-time-feed/storage/collector 尚有历史遗留的结构性
  clippy 债务已于 2026-09-28 一并清零；后续新增代码直接被 `-D warnings` 拦截；
* fmt 工具链版本漂移风险（§1）：出现首个漂移案例时引入 rust-toolchain.toml。
