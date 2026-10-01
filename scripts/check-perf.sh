#!/usr/bin/env bash
# 自动化性能回归检测（TODO L495；非交互）
#
# 机制：criterion 基线比对。首次（或显式 PERF_SAVE=1）以 `--save-baseline`
# 固化当前性能为基线（target/criterion/，不入库）；此后每次以 `--baseline`
# 复跑同套件，criterion 对显著变慢的基准给出统计判定并以非零码退出——
# CI 红灯即回归。基线过期（性能有意变更/机器漂移）重跑 PERF_SAVE=1 刷新。
#
# 基线与机器强相关（同机比对才有效）：CI 用 actions/cache 按周滚动固化
# （.github/workflows/perf.yml），本地用本机基线，跨机器数字不可比。
#
# **前置条件：机器须空闲**。criterion 记墙钟，同机并行 cargo 构建（哪怕
# 另一终端跑测试）会系统性拉慢全部基准——实测并行构建期间 +100% 级漂移、
# p=0.00 全线判退。本地跑本门禁前停掉其他构建；CI 单租户 runner 天然满足。
#
# 用法：
#   scripts/check-perf.sh              # 与基线比对（无基线则先固化）
#   PERF_SAVE=1 scripts/check-perf.sh  # 强制刷新基线
set -euo pipefail

cd "$(dirname "$0")/.."
export PATH="${HOME}/.cargo/bin:${PATH}"

BASELINE="${PERF_BASELINE:-main}"

echo "=== 性能回归门禁（criterion baseline: ${BASELINE}） ==="

# 基线存在性：criterion 基线按基准分桶存（target/criterion/<group>/<bench>/<名>）
have_baseline() {
  find target/criterion -type d -name "$BASELINE" 2>/dev/null | grep -q .
}

if [ "${PERF_SAVE:-0}" = "1" ] || ! have_baseline; then
  if [ "${PERF_SAVE:-0}" != "1" ]; then
    echo "--- 无基线，本次运行为固化基线（不判回归）"
  else
    echo "--- 刷新基线"
  fi
  cargo bench -p alpha-core --bench indicators_bench -- --save-baseline "$BASELINE"
  echo "✅ 基线已固化：cargo bench --save-baseline ${BASELINE}"
  exit 0
fi

# criterion 对回归只打印不退出：跑比对后按输出判——任一基准出现
# "Performance has regressed." 即门禁失败（变快/持平不判）。
# 抖动耐受由 criterion 的统计显著性（p 值）自带，不在此另设百分比阈值。
LOG="target/perf-compare.log"
mkdir -p target
echo "--- 以基线 ${BASELINE} 比对（显著变慢即失败）"
set +e
cargo bench -p alpha-core --bench indicators_bench -- --baseline "$BASELINE" 2>&1 | tee "$LOG"
set -e

if grep -q "Performance has regressed" "$LOG"; then
  echo "❌ 性能回归（见上，基准名行上方）；有意变更则 PERF_SAVE=1 刷新基线"
  exit 1
fi
echo "✅ 性能回归门禁通过（无显著回退）"
