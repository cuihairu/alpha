#!/usr/bin/env bash
# 跨平台兼容性检查（TODO：跨平台 Rust 架构设计 — 统一 Rust 代码规范和跨平台兼容性检查）
#
# 检查项：
#   1. alpha-core 在 wasm32-unknown-unknown 下编译通过（需 --features wasm，
#      该 feature 启用 chrono/wasmbind + uuid/js）；
#   2. alpha-wasm-analyzer 在 wasm32 下构建通过（cdylib，浏览器分析引擎主目标）；
#   3. packages/core 的默认依赖不得引入平台独占/重运行时库
#      （tokio/reqwest/sqlx/redis/tonic 等）——core 是全平台共享层，
#      异步 IO 与原生文件系统访问属于 storage/protocols/services 层职责。
#
#   4. alpha-protocols 在 wasm32 下以 --no-default-features（纯 serde 契约层：
#      rest/websocket/grpc 结构体）编译通过；proto/gRPC 传输为 grpc feature 门控的
#      服务端专属能力，不在 wasm 契约层内。
#
# 用法：scripts/check-cross-platform.sh   （非交互；缺 wasm32 target 自动 rustup 安装）

set -euo pipefail
cd "$(dirname "$0")/.."

fail() { echo "❌ $*"; exit 1; }
info() { echo "ℹ️  $*"; }
ok()   { echo "✅ $*"; }

TARGET=wasm32-unknown-unknown

echo "=== 跨平台兼容性检查 ==="

# 0) wasm32 标准库（非交互安装）
if ! rustup target list --installed 2>/dev/null | grep -q "^$TARGET$"; then
    info "未安装 $TARGET，尝试 rustup target add（需要网络）..."
    rustup target add "$TARGET" || fail "无法安装 $TARGET，请手动安装后重跑"
fi

# 1) alpha-core wasm32 编译
echo "--- [1/4] alpha-core @ $TARGET (--features wasm)"
cargo check --target "$TARGET" -p alpha-core --features wasm \
    || fail "alpha-core 在 $TARGET 下编译失败（检查是否引入了非 wasm 兼容依赖）"
ok "alpha-core wasm32 编译通过"

# 2) alpha-wasm-analyzer wasm32 构建
echo "--- [2/4] alpha-wasm-analyzer @ $TARGET"
cargo build --target "$TARGET" -p alpha-wasm-analyzer \
    || fail "alpha-wasm-analyzer 在 $TARGET 下构建失败（等价于 cargo wasm-build）"
ok "alpha-wasm-analyzer wasm32 构建通过"

# 3) packages/core 默认依赖黑名单扫描
echo "--- [3/4] packages/core 依赖黑名单扫描"
FORBIDDEN='^(tokio|reqwest|sqlx|redis|tonic|native-tls|rustls|notify|directories|dirs)$'
VIOLATIONS=$(sed -n '/^\[dependencies\]/,/^\[/p' packages/core/Cargo.toml \
    | grep -oE '^[a-zA-Z0-9_-]+' | grep -E "$FORBIDDEN" || true)
if [ -n "$VIOLATIONS" ]; then
    fail "packages/core 默认依赖引入平台独占库: $VIOLATIONS（改放 storage/protocols 或加 feature 门控）"
fi
ok "packages/core 默认依赖不含平台独占库"

# 4) alpha-protocols wasm32 纯契约层（--no-default-features，grpc/proto 已 feature 门控）
echo "--- [4/4] alpha-protocols @ $TARGET (--no-default-features 纯 serde 契约)"
cargo check --target "$TARGET" -p alpha-protocols --no-default-features --features alpha-core/wasm \
    || fail "alpha-protocols 契约层在 $TARGET 下编译失败"
ok "alpha-protocols wasm32 契约层编译通过"

echo "=== 跨平台兼容性检查全部通过 ==="
