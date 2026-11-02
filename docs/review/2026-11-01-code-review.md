# 全仓代码深审报告（2026-11-01 巡检）

> 范围：`packages/`、`services/`、`wasm-analyzer/`、`desktop/`、`mobile/`、`web/`、`web/app/`、
> `tools/`、`scripts/`、`config/`、`.github/`、`crawlers/`（Rust ~57k LOC + web TS ~4.5k LOC + shell/python/CI）。
> 方法：安全静态扫描 + 隐藏 bug 猎捕（3 个并行子代理，逐条读码取证）+ 本地质量门
> （clippy / cargo test / web typecheck+vitest+build / docs build）。
> 口径：发现按严重度排序，每条带 `file:line` 证据；P0/P1 当场修（一问题一 commit，测试绿），P2/P3 记 todo。

## 1. 安全静态扫描（六类）

按注入/输入校验、认证授权、数据暴露、网络传输、秘密配置、依赖供应链六类扫描。命中一类（注入/输入校验，共 3 条），全部 P0、当场修复：

| 编号 | 类别 | 严重度 | 证据 | 修复 |
|---|---|---|---|---|
| S-1 | 注入/输入校验 | P0 | `services/collector/src/main_simple.rs:675` `generate_script_code` 四语言模板把 `task.config.request.url`/`headers` 裸插进脚本，引号/换行可语句逃逸 | `c0ab9bb` |
| S-2 | 注入/输入校验 | P0 | `services/data-engine/src/main.rs:1607` `/query` 未限制语句类型：`CREATE EXTERNAL TABLE ... LOCATION 'file://…'` 可读任意文件，DDL/DML 破坏数据 | `c401e34` |
| S-3 | 注入/输入校验 | P0 | `packages/storage/src/clickhouse.rs:122` `query_market_data` 把 `symbol` 直接拼进 SQL，单引号未转义可注入 | `2d38125` |

认证授权、数据暴露、网络传输、秘密配置、依赖供应链五类：静态扫描覆盖，未发现 P0/P1（无对应修复；质量门全绿见 §3）。

## 2. 隐藏 bug 猎捕（六类）

按数值/边界、异步/竞态、内存/生命周期、状态管理/幂等、并发/线程安全、资源泄漏/清理六类扫描。命中两类，全部 P1、当场修复：

| 编号 | 类别 | 严重度 | 证据 | 修复 |
|---|---|---|---|---|
| B-1 | 数值/边界 | P1 | `packages/core/src/indicators.rs:31` `calculate_sma` 当 `period=0` 时 `sma[period-1]` 下溢 `usize::MAX` 越界 panic；未认证 `GET /api/v1/indicators?sma_short=0` 可达 | `b636a2d` |
| B-2 | 异步/竞态 | P1 | `web/app/src/hooks/useLiveQuotes.ts:137` `onerror` 与 `onclose` 先后触发令 `onDown` 双跑：`attempt` 每失败双计（`MAX_RECONNECT=2` 一次真实失败即耗尽）、第二 `setTimeout` 覆盖 `retryTimer` 泄漏首个定时器并重复 `connect()` | `89db63c` |

内存/生命周期、状态管理/幂等、并发/线程安全、资源泄漏/清理四类：未发现 P0/P1。

## 3. 质量门（基线对比）

| 门禁 | 命令 | 结果 |
|---|---|---|
| Rust 静态分析 | `cargo clippy --workspace --all-targets --exclude alpha-desktop -- -D warnings` | ✅ 0 lint（仅 1 条未来兼容 informational 注记：sqlx-postgres v0.7.4 将被未来 Rust 拒绝） |
| Rust 全量测试 | `cargo test --workspace --all-targets --exclude alpha-desktop` | ✅ 624 通过 / 0 失败（退出码 0） |
| Web 类型+单测+构建 | `scripts/check-web.sh`（tsc --noEmit + vitest run + vite build） | ✅ 14 文件 / 97 测试通过，构建成功 |
| 文档站构建 | `cd docs && pnpm build` | ✅ Server/Client 编译成功，静态产物生成（Docusaurus 3.10.2） |

基线对比：无回归（CI 期望全绿，实测与期望一致）。lint/typecheck/测试三项与本仓 CI 门禁同源。

## 4. 修复与剩余

**P0/P1 当场修复（5 笔，一问题一 commit，各带回归测试、测试绿）：**

- `c0ab9bb` S-1：`generate_script_code` 新增 `script_json_literal`/`script_shell_literal` 收口 URL/header；`test_script_code_generation_neutralizes_injection` 断言恶意载荷全收进字符串字面量、无语句逃逸。
- `c401e34` S-2：`execute_query` 入口加 `is_read_only_query`（sqlparser 白名单 SELECT/WITH/EXPLAIN/SHOW/DESCRIBE，解析失败一律拒绝）；`execute_query_rejects_ddl_and_dml` 断言 DDL/DML 拒绝、SELECT 放行。
- `2d38125` S-3：`symbol.replace('\'', "''")` 转义单引号。
- `b636a2d` B-1：`calculate_sma` 增 `period == 0` 守卫（与 bollinger 同式，返回全 0）；`test_sma_zero_period_does_not_panic`。
- `89db63c` B-2：`onDown` 加 `if (ws === null) return` 幂等守卫，重连每连接只排一次。

**剩余（P2/P3）：** 本次代码深审未产生新 P2/P3 任务；`todo.md` 无未勾项，不新增任务。

## 5. 汇总

- 审查范围：Rust ~57k LOC + web TS ~4.5k LOC + shell/python/CI，全仓静态扫描 + 3 并行子代理取证。
- 发现：P0×3（注入/输入校验安全面）+ P1×2（隐藏 bug：数值边界、异步竞态），全部当场修复，5 笔 commit 各带回归测试。
- 质量门：clippy 0 lint、Rust 624 测试、web 97 测试+构建、docs 构建全绿；基线无回归。
- 剩余风险：无 P0/P1 未修；P2/P3 无新任务（`todo.md` 无未勾项）。
- 结论：本次巡检 PASS，无阻塞项。
