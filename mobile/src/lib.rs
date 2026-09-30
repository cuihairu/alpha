//! Alpha Finance 移动端 Rust 核心库（TODO「设计移动端 Rust 核心库架构
//! （JNI + UniFFI）」，台账 L118）
//!
//! 分层（docs/mobile-core-architecture.md）：
//!
//! * **本 crate（FFI 桥 + 核心状态，零平台 SDK 依赖）**：[`state::MobileCore`]
//!   持观察列表/配置槽/分析引擎，经 UniFFI proc-macro（`setup_scaffolding!`，
//!   无 UDL 副本）导出 Kotlin/Swift 绑定面；载荷为 JSON 字符串（字段契约由
//!   单测锁定），错误经 [`MobileError`] 跨桥抛类型化异常。业务与计算全部在
//!   `alpha-core`——与 web/desktop 同一份引擎，不重复实现。
//! * **平台壳（后续 TODO）**：Kotlin/Compose、Swift/SwiftUI 侧调用生成的绑定，
//!   主线程禁止直调（重活上后台）；裸 JNI 只作 SDK 回调等逃生舱（workspace
//!   已 pin `jni`，本骨架不引用）。
//!
//! 骨架进 workspace 成员名单：`cargo clippy/test --workspace` 即覆盖编译与
//! 单测（Linux 可全绿）；绑定**生成**与真机运行需 Android SDK / Xcode，
//! 属登记的验收边界（文档 §12）。
//!
//! 非交互假设（§11 编号）：proc-macro-only（不写 UDL）、JSON 字符串桥而非
//! `uniffi::Record`、JNI 逃生舱本轮不引用、per-call current-thread 运行时、
//! 观察列表外一律 `InvalidSymbol`。

#![warn(missing_docs)]
// uniffi 0.25 `setup_scaffolding!` 展开码自带函数指针版本校验，rustc 的
// `unpredictable_function_pointer_comparisons`（warn-by-default）会命中——
// 门禁 `-D warnings` 下误伤；该 lint 属性在宏调用处不传导（span 归宏自身），
// 故 crate 级放行。本 crate 自身无函数指针比较，无实际覆盖损失。
#![allow(unpredictable_function_pointer_comparisons)]

mod market;
mod state;

pub use market::{
    synthetic_quote, synthetic_quote_at, synthetic_series, synthetic_series_at, DEFAULT_BARS,
};
pub use state::{MobileCore, MobileError};

// uniffi 顶层装配：proc-macro-only 模式的唯一入口（与 include_scaffolding!
// 互斥——本 crate 不写 UDL，双份定义漂移是 uniffi 的头号事故源）。宏展开项
// 上的 /// 文档注释会触发 unused_doc_comments，故用行注释。
uniffi::setup_scaffolding!();
