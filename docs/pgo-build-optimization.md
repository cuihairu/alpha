# 基于 LLVM Profile 的编译优化（TODO L455）

工具面：`scripts/build-pgo.sh`——PGO（Profile-Guided Optimization）三阶段
构建循环的一键封装。本文档定用法、负载画像要求与边界。

## 1. 原理与收益面

PGO 让编译器基于**真实运行的执行画像**做代码布局与内联决策：

| 优化 | 机制 | 对行情系统的意义 |
|---|---|---|
| 热路径布局 | 高频基本块落同一 `.text` 页，icache/ITLB 命中率↑ | 反代/解帧/指标计算等热循环受益直接 |
| 激进内联 | 热调用点放宽内联阈值、冷路径收缩 | 抽象层（错误处理、迭代器适配）不再是抽象税 |
| 冷路径优化 | 不可达分支可安全下沉/裁剪 | 配置分支不污染热路径寄存器分配 |

与 LTO 的关系：`profile-use` 与 `-Clto=fat` 正交可叠加（发布流水线可
组合 `-Cprofile-use=... -Clto=fat`；本脚本默认不开 LTO，避免单变量归因困难）。

## 2. 用法

```bash
# 最小形态（负载退化为 --help 启动路径，仅验证流水线）
scripts/build-pgo.sh alpha-api-gateway target/release/alpha-api-gateway --help

# 真实负载：传入会实际驱动该二进制的命令
scripts/build-pgo.sh alpha-data-engine scripts/dev-start.sh   # 示例
```

环境变量：`PGO_DIR`（插桩输出目录，默认 `target/pgo-data`）、
`PGO_OUT`（合并 profdata，默认 `target/pgo.profdata`）、
`LLVM_PROFDATA`（显式指定工具路径）。缺省探测顺序：PATH →
**`rustc --print sysroot` 对应 toolchain 的 llvm-tools**（raw profile
格式版本随 rustc 演进——实测 1.97 的 profdata 读不了 1.99 产的
version-11 profile，**跨版本兜底目录不可靠**，故只信当前 sysroot；
缺失时脚本自动 `rustup component add llvm-tools-preview`）。

四阶段（脚本内同序执行）：

1. `RUSTFLAGS="-Cprofile-generate=<dir>" cargo build --release --bin <name>`
2. 运行负载 → `<dir>/*.profraw`（脚本校验非空，负载没跑真二进制会明确报错）
3. `llvm-profdata merge -output=<out> <dir>/*.profraw`
4. `RUSTFLAGS="-Cprofile-use=<out>" cargo build --release --bin <name>`，
   结束对比两阶段二进制大小（代码布局优化大小可能增减，**不是收益指标**——
   收益以运行时热点微基准为准）

## 3. 负载画像要求（决定优化质量）

> 垃圾进垃圾出：负载不代表性，PGO 会把冷路径当热路径优化。

- **api-gateway**：反代回放——以真实比例重放 `/api/v1/*` 请求集
  （如 90% 行情查询 + 10% K 线历史）+ 少量 `/health` 探测；
- **data-engine**：`/query` SQL 回放集 + `/clickhouse/export.parquet` 导出；
- **real-time-feed**：建立 WS 连接注入一段真实 tick 回放；
- 采集时长原则：覆盖稳态（跳过冷启动段），不刻意制造极端输入。

## 4. 边界与假设

1. **本脚本产物不进 CI/发布**：CI 构建（确定性、可复现）保持无 PGO；
   PGO 二进制随发布流水线（TODO「多平台发布流水线」）产出并归档 profdata 溯源。
2. wasm32 / 移动端目标不在本脚本范围：`-Cprofile-generate` 对
   wasm32-unknown-unknown 的运行时支持依赖宿主 embedder，浏览器侧画像
   采集归桌面/移动发布项另行评估。
3. 交叉验证原则：优化前后以 `packages/core` 微基准（simd/parallel 组）
   对比热路径耗时，避免只看二进制大小做结论。
4. 工具链假设：rustup 管理的 toolchain 带 `llvm-tools-preview` 组件
   （本机 1.97.0 已含 `llvm-profdata`；缺组件时脚本给出明确安装指引）。

## 5. 已验证（本机实测）

- 脚本语法（`bash -n`）；
- 完整四阶段循环对本仓 `alpha-api-gateway` 实测跑通：插桩构建 →
  `--help` 采集（1 个 profraw）→ merge（11.9MB profdata）→ 优化构建 →
  优化后二进制 `--help` 冒烟通过；
- 实测踩坑两条（脚本已固化修正）：
  1. **profdata 工具版本必须匹配 rustc**：1.97 toolchain 的
     `llvm-profdata` 读不了 1.99 产的 raw profile version 11（报
     `raw profile format version mismatch`）→ 探测只信当前 sysroot；
  2. **`-Cprofile-use` 相对路径不可靠**：profdata 实际存在时 rustc 仍报
     `does not exist` → 脚本统一转绝对路径。
