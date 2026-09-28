#!/usr/bin/env bash
# 跨平台兼容性检查（TODO：跨平台 Rust 架构设计 — 统一 Rust 代码规范和跨平台兼容性检查）
#
# 检查项：
#   1. alpha-core 在 wasm32-unknown-unknown 下编译通过（需 --features wasm，
#      该 feature 启用 chrono/wasmbind + uuid/js）；
#   2. packages/core 的默认依赖不得引入平台独占/重运行时库
#      （tokio/reqwest/sqlx/redis/tonic 等）——core 是全平台共享层，
#      异步 IO 与原生文件系统访问属于 storage/protocols/services 层职责。
#
# 已知差距（设计文档 docs/cross-platform-architecture.md §5）：alpha-protocols
# 因 tonic 默认特性拉入 tokio/net 尚不 wasm-clean， remediation 是给 grpc 模块
# 加 feature 门控；在此以 informational 输出，不作为门禁失败条件。
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
echo "--- [1/3] alpha-core @ $TARGET (--features wasm)"
cargo check --target "$TARGET" -p alpha-core --features wasm \
    || fail "alpha-core 在 $TARGET 下编译失败（检查是否引入了非 wasm 兼容依赖）"
ok "alpha-core wasm32 编译通过"

# 2) packages/core 默认依赖黑名单扫描
echo "--- [2/3] packages/core 依赖黑名单扫描"
FORBIDDEN='^(tokio|reqwest|sqlx|redis|tonic|native-tls|rustls|notify|directories|dirs)$'
VIOLATIONS=$(sed -n '/^\[dependencies\]/,/^\[/p' packages/core/Cargo.toml \
    | grep -oE '^[a-zA-Z0-9_-]+' | grep -E "$FORBIDDEN" || true)
if [ -n "$VIOLATIONS" ]; then
    fail "packages/core 默认依赖引入平台独占库: $VIOLATIONS（改放 storage/protocols 或加 feature 门控）"
fi
ok "packages/core 默认依赖不含平台独占库"

# 3) alpha-protocols wasm32 现状（informational，已知差距）
echo "--- [3/3] alpha-protocols @ $TARGET（informational）"
if cargo check --target "$TARGET" -p alpha-protocols --features alpha-core/wasm >/dev/null 2>&1; then
    ok "alpha-protocols wasm32 编译通过（已知差距已消除，请更新设计文档 §5）"
else
    info "alpha-protocols 尚不 wasm-clean（tonic→tokio/net→mio；remediation 见 docs/cross-platform-architecture.md §5），不作为本次门禁失败条件"
fi

echo "=== 跨平台兼容性检查全部通过 ==="
