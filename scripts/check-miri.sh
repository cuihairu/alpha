#!/usr/bin/env bash
# Miri 内存安全静态分析（TODO L496；非交互）
#
# 范围：alpha-core（unsafe 清单集中地——simd.rs / alloc_tracking.rs 两个
# 舱位豁免点，见 L463 safety_audit 预算断言）。解释执行慢，全 workspace
# 不现实也不必要：其余 crate 的 unsafe 面由 check-lint.sh 的
# `#![deny(unsafe_code)]`（workspace 级）静态封死——新增 unsafe 文件会编译
# 失败，Miri 只需要覆盖真实写了 unsafe 的那一块。
#
# 多种子：-Zmiri-many-seeds=0..5（5 个种子）——UB 常常只在特定分配布局/
# 哈希种子下显形，单种子通过不等于无 UB。
#
# 依赖：rustup（有 nightly 工具链即可，无则自动安装）、cargo-miri 组件
# （首次经 `cargo +nightly miri setup` 自动装）。退出码 0 通过 / 非 0 失败。
#
# 已知边界（诚实登记）：simd.rs 的 AVX2 target_feature intrinsics 由
# runtime 检测门进入；Miri 对 x86_64 SIMD intrinsics 的支持是子集——
# 若报 "unsupported operation" 属模拟器能力边界而非 UB，需要在 CI 上
# 以 MIRIFLAGS 补 -Zmiri-skip-execution 或调整检测路径（见 docs 项）。
set -euo pipefail

cd "$(dirname "$0")/.."
export PATH="${HOME}/.cargo/bin:${PATH}"

echo "=== Miri 内存安全门禁（alpha-core，种子 0..5） ==="

# 1) nightly + miri 就位（幂等：已有则零开销）
if ! rustup toolchain list | grep -q nightly; then
  echo "--- 安装 nightly 工具链"
  rustup toolchain install nightly --component miri --profile minimal
fi
if ! cargo +nightly miri --version >/dev/null 2>&1; then
  echo "--- 安装 cargo-miri 组件"
  rustup +nightly component add miri
  cargo +nightly miri setup
fi
cargo +nightly miri --version

# 2) 多种子解释执行 alpha-core lib 目标（doctest/集成测试不在 miri 面）
#    之所以锁定 --lib：① unsafe 面都在 src 单测里（simd/alloc_tracking/memory/
#    parallel 在 --cfg test 下全覆盖）；② tests/proptest_invariants.rs（9 性质×
#    256 案例×5 种子）与 tests/*.rs 在解释执行下耗时爆炸（实测整仓 >2h 未跑完），
#    其语义性质由常规 cargo test 门禁把关，miri 不重复验算；③ lib 单目标让门禁
#    可在合理时间内跑完（全量实测 >2h 仍未覆盖完 safety_audit 之后的模块，
#    诊断为集成测试目标的 proptest 256×5 案例面拖垮——信号低于其语义价值）。
#    -Zmiri-disable-isolation：测试套件读墙钟（models::MarketData::new 的
#    chrono::Utc::now 时间戳），clock_gettime(REALTIME) 无隔离回退路径
#    （-Zmiri-isolation-error=warn 对该操作不生效，实测）——只能关隔离；
#    -Zmiri-tree-borrows：rayon 栈里的上游依赖 crossbeam-epoch 0.9.21 存在
#    已知的 Stacked Borrows 违规（其 epoch GC 的指针惯用法，非本项目代码，
#    实测复现于 parallel 模块测试路径）——Tree Borrows 是 Miri 的另一套
#    别名模型，上游违规不误报，自有 unsafe 面（simd.rs/alloc_tracking.rs）
#    在该模型下同样全量受检（同套件执行）
echo "--- MIRIFLAGS='-Zmiri-many-seeds=0..5 -Zmiri-disable-isolation -Zmiri-tree-borrows' cargo +nightly miri test -p alpha-core --lib"
MIRIFLAGS="-Zmiri-many-seeds=0..5 -Zmiri-disable-isolation -Zmiri-tree-borrows" cargo +nightly miri test -p alpha-core --lib

echo "=== Miri 门禁通过（无未定义行为） ==="
