#!/usr/bin/env bash
# 基于 LLVM Profile 的 PGO（Profile-Guided Optimization）构建（L455）
#
# 三阶段循环：
#   [1/4] 插桩构建    RUSTFLAGS="-Cprofile-generate=<dir>" cargo build --release
#   [2/4] 采集负载    运行代表性负载，产出 .profraw
#   [3/4] 合并 profile llvm-profdata merge → pgo.profdata
#   [4/4] 优化构建    RUSTFLAGS="-Cprofile-use=<profdata>" cargo build --release
#
# 用法：
#   scripts/build-pgo.sh <bin-name> [load-command...]
# 例（真实负载应是该服务的典型运行画像，见 docs/pgo-build-optimization.md）：
#   scripts/build-pgo.sh alpha-api-gateway target/release/alpha-api-gateway --help
#   scripts/build-pgo.sh alpha-data-engine <回放脚本>
#
# 环境变量：
#   PGO_DIR      插桩 profile 输出目录（默认 target/pgo-data）
#   PGO_OUT      合并后 profdata 路径（默认 target/pgo.profdata）
#   负载缺省时跑 `target/release/<bin> --help`（仅触发启动路径——
#   覆盖有限，正式优化应提供真实负载命令）
set -euo pipefail

cd "$(dirname "$0")/.."

BIN="${1:?用法: build-pgo.sh <bin-name> [load-command...]}"
shift || true
# rustc -Cprofile-use/-Cprofile-generate 对相对路径解析不稳（实测 profile-use
# 在文件实际存在时仍报 does not exist），统一转绝对路径
abspath() { case "$1" in /*) echo "$1" ;; *) echo "$(pwd)/$1" ;; esac; }
PGO_DIR="$(abspath "${PGO_DIR:-target/pgo-data}")"
PGO_OUT="$(abspath "${PGO_OUT:-target/pgo.profdata}")"

echo "=== [1/4] 定位 llvm-profdata ==="
# 版本必须与当前 rustc 匹配（raw profile 格式随 rustc 演进，旧 toolchain 的
# llvm-profdata 读不了新 profile——故 sysroot 优先，跨版本兜底目录不采用）
SYSROOT="$(rustc --print sysroot)"
HOST_TRIPLE="$(rustc -vV | awk '/^host:/ {print $2}')"
SYSROOT_PROFDATA="$SYSROOT/lib/rustlib/$HOST_TRIPLE/bin/llvm-profdata"
LLVM_PROFDATA="${LLVM_PROFDATA:-}"
if [[ -z "$LLVM_PROFDATA" ]]; then
  if command -v llvm-profdata >/dev/null 2>&1; then
    LLVM_PROFDATA="$(command -v llvm-profdata)"
  elif [[ -x "$SYSROOT_PROFDATA" ]]; then
    LLVM_PROFDATA="$SYSROOT_PROFDATA"
  else
    echo "当前 toolchain 缺 llvm-tools-preview，尝试安装..." >&2
    rustup component add llvm-tools-preview
    [[ -x "$SYSROOT_PROFDATA" ]] || {
      echo "安装后仍未找到 $SYSROOT_PROFDATA" >&2
      exit 1
    }
    LLVM_PROFDATA="$SYSROOT_PROFDATA"
  fi
fi
echo "llvm-profdata: $LLVM_PROFDATA"

echo "=== [2/4] 插桩构建（profile-generate）==="
rm -rf "$PGO_DIR" "$PGO_OUT"
mkdir -p "$PGO_DIR"
RUSTFLAGS="-Cprofile-generate=$PGO_DIR" cargo build --release --bin "$BIN"

echo "=== [3/4] 采集负载 ==="
if [[ $# -gt 0 ]]; then
  "$@"
else
  echo "（未提供负载命令，退化为 --help 启动路径；正式优化请传入真实负载）"
  "target/release/$BIN" --help >/dev/null
fi
COUNT="$(find "$PGO_DIR" -name '*.profraw' | wc -l)"
[[ "$COUNT" -gt 0 ]] || { echo "未产出 .profraw：负载是否真的执行了二进制？" >&2; exit 1; }
echo "profile 文件数: $COUNT"

echo "=== [4/4] 合并 + 优化构建（profile-use）==="
"$LLVM_PROFDATA" merge -output="$PGO_OUT" "$PGO_DIR"/*.profraw
BASE_SIZE="$(stat -c%s "target/release/$BIN")"
RUSTFLAGS="-Cprofile-use=$PGO_OUT" cargo build --release --bin "$BIN"
OPT_SIZE="$(stat -c%s "target/release/$BIN")"

echo "=== PGO 构建完成 ==="
echo "profile:      $PGO_OUT"
printf '二进制大小:   %d → %d 字节（%+.1f%%；代码布局优化，大小可能增减）\n' \
  "$BASE_SIZE" "$OPT_SIZE" \
  "$(awk "BEGIN { printf ($OPT_SIZE - $BASE_SIZE) * 100 / $BASE_SIZE }")"
