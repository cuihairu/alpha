#!/usr/bin/env bash
# 性能分析封装（L456）：perf 采样 + 报告（Linux），服务二进制通用
#
# 用法：
#   scripts/profile.sh record <bin> [args...]   # 采样到 target/perf.data
#   scripts/profile.sh report                   # 交互式火焰报告
#   scripts/profile.sh top                      # 实时热点
#
# 前置：apt install linux-tools-common linux-tools-$(uname -r)（或对应发行版包）
# 内核参数：perf_event_paranoid ≤ 1、kptr_restrict = 0（否则提示降级命令）
#
# 服务侧提示：tokio 任务级画像用 tokio-console（cargo install tokio-console，
# 服务启动带 --cfg tokio_unstable 的构建才支持）；分配画像用
# packages/core::alloc_tracking::TrackingAllocator（见 docs/memory-profiling.md）
set -euo pipefail

cd "$(dirname "$0")/.."
MODE="${1:?用法: profile.sh <record|report|top> [bin args...]}"
shift || true

require_perf() {
  command -v perf >/dev/null 2>&1 || {
    echo "perf 未安装：sudo apt install linux-tools-common linux-tools-$(uname -r)" >&2
    exit 1
  }
}

case "$MODE" in
  record)
    BIN="${1:?record 需要 <bin> [args...]}"
    shift
    require_perf
    mkdir -p target
    # -g 带调用栈；--call-graph dwarf 用户态栈回溯比 fp 更全
    perf record -g --call-graph dwarf -o target/perf.data -- "$BIN" "$@"
    echo "采样完成：target/perf.data（scripts/profile.sh report 查看）"
    ;;
  report)
    require_perf
    [[ -f target/perf.data ]] || { echo "target/perf.data 不存在：先 record" >&2; exit 1; }
    exec perf report -i target/perf.data
    ;;
  top)
    require_perf
    BIN="${1:?top 需要 <bin> [args...]}"
    shift
    exec perf top -g -- "$BIN" "$@"
    ;;
  *)
    echo "未知模式: $MODE（record|report|top）" >&2
    exit 1
    ;;
esac
