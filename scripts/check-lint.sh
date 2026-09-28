#!/usr/bin/env bash
# Rust 代码规范门禁（docs/rust-code-standards.md §11；本地与 CI 完全一致）
#   1. cargo fmt --check：格式化差异即失败
#   2. cargo clippy -D warnings：任何警告即失败
# 与测试门禁同口径排除 alpha-desktop（Tauri 桌面 crate 需系统级 GUI 依赖）。

set -euo pipefail
cd "$(dirname "$0")/.."

echo "=== Rust 规范门禁 ==="

echo "--- [1/2] cargo fmt --check"
cargo fmt --all -- --check
echo "✅ 格式化检查通过"

echo "--- [2/2] cargo clippy -- -D warnings"
cargo clippy --workspace --all-targets --exclude alpha-desktop -- -D warnings
echo "✅ clippy 零警告"

echo "=== Rust 规范门禁全部通过 ==="
