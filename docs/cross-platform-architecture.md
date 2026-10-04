# 跨平台 Rust 架构设计

> 状态：v1 草案（2026-09-28），对应 TODO「跨平台 Rust 架构设计」首项「设计统一跨平台架构」。
> 文中编译结论均为本机实测（wasm32-unknown-unknown target），非纸面推断。

## 1. 目标与范围

一套 Rust 工作区同时支撑四类交付面：

| 交付面 | 目录 | 形态 |
|---|---|---|
| 服务端 | `services/*` | Axum/gRPC 微服务（Linux 容器） |
| Web | `web/` + `wasm-analyzer/` | 静态前端 + Rust→WASM 分析引擎 |
| 桌面 | `desktop/` | Tauri 1.5（Windows/macOS/Linux） |
| 移动 | `mobile/`（骨架已落地：alpha-mobile workspace 成员 + android/ios 壳 + 契约测试） | Kotlin/Swift 壳 + Rust 核心库（UniFFI；裸 JNI 仅作 SDK 回调逃生舱） |

原则：**业务逻辑下沉共享库，平台能力走适配层**。任何一层不反向依赖交付面。

## 2. 现状盘点（实测）

| 模块 | 技术栈 | 平台约束 | 现状 |
|---|---|---|---|
| `packages/core` | serde/uuid/chrono + 纯计算（models/indicators/analytics/platform） | 无 tokio/reqwest/sqlx/redis/tonic | **wasm32 编译通过**（需 `--features wasm`：`chrono/wasmbind` + `uuid/js`，见 `packages/core/Cargo.toml:41`）；`errors.rs` 已有 `cfg(target_arch = "wasm32")` 分支 |
| `packages/protocols` | tonic/prost + serde | tonic 默认特性拉 tokio/net | **wasm32 编译失败**（mio 不支持 wasm；见 §5 差距） |
| `packages/storage` | sqlx/redis/clickhouse + DataFusion | 服务端专属 | 按 L1 设计即不追求 wasm |
| `services/*` | Axum + Tokio + DataFusion | 服务端专属 | 已落地（P0–P3 行动清单全绿） |
| `wasm-analyzer` | wasm-bindgen + Arrow | 浏览器 | 已有 arrow_adapter/streaming/websocket/worker 模块 |
| `web/` | 原生 JS + duckdb-wasm vendor + WebSocket | 浏览器 | 已接 real-time-feed WS 与 data-engine REST（见 TODO P3-2） |
| `desktop/` | Tauri 1.5（fs/dialog/tray/notification/global-shortcut 特性） | 桌面三 OS | 已有壳；gate 中以 `--exclude alpha-desktop` 排除 |
| `mobile/` | alpha-mobile（UniFFI proc-macro 桥 + 确定性行情） | Android/iOS 壳；绑定生成需 SDK（无 Xcode 时 iOS 未跑） | 骨架已落地（Rust 核心随 lint/test 门禁；android 契约测试 + 54 个 JVM 单测在跑） |

## 3. 统一分层架构

```
┌────────────────────────── L2 平台表现层 ──────────────────────────┐
│  web/ (JS + wasm-analyzer)   desktop/ (Tauri)   mobile/ (规划)     │
│  只做 UI/交互/平台 API 调用，业务计算一律调 L0                        │
├─────────────────────────── L1 平台服务层 ─────────────────────────┤
│  packages/storage (时序/缓存/DLQ)   packages/protocols (传输协议)   │
│  services/* (data-engine / real-time-feed / api-gateway / collector)│
│  服务端专属；mobile 不直连 storage，经 L2 → REST/WS → services       │
├─────────────────────────── L0 共享核心层 ─────────────────────────┤
│  packages/core：模型(models)、指标(indicators)、分析(analytics)、    │
│  平台适配(platform)、错误(errors) —— 零平台依赖，wasm32 必须编译通过 │
└───────────────────────────────────────────────────────────────────┘
```

依赖方向强制：L2 → L0/L1（按平台取用）、L1 → L0、**禁止 L0 → L1/L2**。
桌面 Tauri 与未来 mobile 的 Rust 侧只允许依赖 L0（+ 经 feature 门控的 L1 子集）。

## 4. 平台适配层接口草案

平台差异（存储键值、HTTP、文件系统、通知）收敛为 `packages/core` 内的 trait 面，
各交付面给出实现；Rust 业务代码面向 trait，不直接摸平台 API：

```rust
// packages/core/platform.rs（已落地：trait + InMemory 参考实现，f6b44cd/9e46658；
// 桌面/移动侧的真实实现仍缺——桌面走 desktop/src 内自有实现，移动端留待接线）
pub trait KeyValueStore {            // 桌面: 文件/SQLite；Web: IndexedDB(wasm 侧)；
    async fn get(&self, key: &str) -> Option<Vec<u8>>;   // 服务端: Redis
    async fn set(&self, key: &str, value: &[u8]);
}

pub trait LocalPersistence {         // 桌面: 原生 fs；mobile: 应用沙箱目录
    async fn export_file(&self, name: &str, data: &[u8]) -> Result<(), AlphaError>;
}

pub trait UserNotification {         // 桌面: Tauri notification；mobile: 系统推送
    fn notify(&self, title: &str, body: &str);
}
```

约束：trait 方法均为 async 且不带平台类型；实现放各交付面 crate
（如 `desktop/src/platform.rs`），通过依赖注入进入业务代码。

## 5. 构建目标矩阵与兼容性差距

| Crate | x86_64 linux | wasm32-unknown-unknown | 桌面三 OS | android/ios |
|---|---|---|---|---|
| alpha-core | ✅ | ✅（`--features wasm`，实测） | ✅（同 core 约束） | 规划 |
| alpha-protocols | ✅（default=grpc） | ✅（`--no-default-features` 纯 serde 契约，实测） | ✅ | 规划 |
| alpha-storage / services | ✅ | 不适用（L1） | 不适用 | 不适用 |
| alpha-wasm-analyzer | — | ✅（实测 `cargo wasm-build`） | — | — |

**alpha-protocols 分层（原「已知差距」，2026-09-28 已消除）**：tonic 默认特性
（`transport`）拉入 tokio/net → mio，wasm32 编译失败（实测报错 `This wasm target is
unsupported by mio`）。已落地：tonic/prost/tonic-build 转 optional，`grpc` feature
（default = ["grpc"]）门控 proto 代码生成（build.rs 按 cfg 跳过）与 `proto` 模块；
`rest.rs`/`websocket.rs`/`grpc.rs`（纯 serde 结构体）保持无门控共享。服务端默认开启
grpc 零改动；wasm 侧取 `--no-default-features` 契约层，wasm32 编译实测通过，
并已并入 `scripts/check-cross-platform.sh` 门禁（第 4 步）。

## 6. 代码规范与兼容性检查

> 完整 Rust 代码规范（格式化/clippy 零警告/错误处理/异步/日志/测试纪律）见
> **docs/rust-code-standards.md**；本节只保留跨平台相关约定。

强制检查 = `scripts/check-cross-platform.sh`（非交互可入 CI）；多目标构建入口 =
`.cargo/config.toml` 的 cargo alias（`cargo wasm-check` / `cargo wasm-build`）：

1. `cargo wasm-check`（alpha-core @ wasm32，`--features wasm`）必须通过；
2. `cargo wasm-build`（alpha-wasm-analyzer @ wasm32 cdylib 构建）必须通过；
3. `packages/core` 默认依赖黑名单扫描（tokio/reqwest/sqlx/redis/tonic/native-tls/
   rustls/notify/directories/dirs）——命中即失败，新增平台能力先放 L1 或加 feature 门控；
4. alpha-protocols wasm32 `--no-default-features` 契约层编译门禁（脚本第 4 步，硬失败）。

编码规范（评审口径，配合脚本执行）：

* L0 crate 禁平台独占依赖与 `std::fs`/`std::net` 直接使用（wasm32 无文件系统）；
* 平台分支统一 `cfg(target_arch = "wasm32")` / `cfg(target_os = ...)`，禁止散落 link；
* 时间处理用 chrono（开启 `wasmbind`）；ID 用 uuid（wasm 侧开启 `js`）；
* 新增共享计算逻辑先进 `packages/core`，再由各交付面消费；反向搬运会破坏 §3 依赖方向。

## 7. 落地路线图（映射本节 TODO 余项）

| TODO 项 | 依赖/顺序 | 说明 |
|---|---|---|
| Cargo workspace 多目标构建配置 | 本设计 | ✅ 已落地：`.cargo/config.toml` alias（wasm-check/wasm-build）+ 检查脚本纳入 wasm-analyzer wasm32 构建；CI 矩阵随「CI/CD」节推进 |
| 跨平台共享核心库（core/protocols/storage） | §5 | ✅ 已落地：core wasm-clean（wasm feature）；protocols grpc feature 门控后 wasm32 契约层编译通过；storage 按设计属 L1 服务端专属（移动端经 REST/WS 访问 services，不直连 storage） |
| 平台适配层抽象接口 | §4 草案 | ✅ trait + InMemory 参考实现已落地（`platform.rs`，f6b44cd/9e46658）；桌面/移动侧真实实现待接 |
| 统一 Rust 代码规范与兼容性检查 | 本设计 §6 | 脚本已落地，CI 集成随「CI/CD」节推进 |
| Web UI 框架选型与集成（L427） | 本设计 §3 L2 | ✅ 已落地：docs/web-framework-selection.md 定论 **React 18 + TS + Vite**（Yew/Leptos 不选作主框架），`web/app/` 骨架 + `scripts/check-web.sh` 接入 CI `wasm` 作业；旧演示页零回退；Desktop/Mobile 接入边界登记于该文档 §6 |
