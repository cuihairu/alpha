#!/usr/bin/env bash
# 代码覆盖率报告（TODO L494；非交互）
#
# cargo-llvm-cov（基于 LLVM source-based coverage）对 Rust workspace 出
# 行/函数覆盖率汇总，lcov 格式落盘供 CI artifact / genhtml 消费。
# 范围与常规测试门禁一致：--exclude alpha-desktop（GUI 接线层需 macOS，
# 见 ci.yml Desktop 作业的分工）。
#
# 不设硬阈值：覆盖率是度量不是门禁数字——阈值会诱导凑数测试（用户约束：
# 测试只到门禁量，不开覆盖率批次）。报告产出即交付，回归观察靠趋势。
#
# 依赖：cargo-llvm-cov（`cargo install cargo-llvm-cov`）+
# llvm-tools-preview 组件（`rustup component add llvm-tools-preview`），
# 缺失时给出安装提示后退出 1。退出码 0 通过 / 非 0 失败。
set -euo pipefail

cd "$(dirname "$0")/.."
export PATH="${HOME}/.cargo/bin:${PATH}"

echo "=== 代码覆盖率门禁（llvm-cov，行/函数汇总） ==="

if ! command -v cargo-llvm-cov >/dev/null 2>&1; then
  echo "❌ 缺 cargo-llvm-cov：cargo install cargo-llvm-cov"
  echo "   并安装 llvm-tools-preview：rustup component add llvm-tools-preview"
  exit 1
fi

OUT_DIR="target/coverage"
mkdir -p "$OUT_DIR"

cargo llvm-cov \
  --workspace \
  --all-targets \
  --exclude alpha-desktop \
  --summary-only \
  --lcov \
  --output-path "$OUT_DIR/lcov.info"

echo "✅ lcov 报告已写入 $OUT_DIR/lcov.info"
echo "   HTML 视图：cargo llvm-cov --workspace --all-targets --exclude alpha-desktop --html --open"
echo "=== 覆盖率门禁通过 ==="
