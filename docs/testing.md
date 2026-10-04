# 测试体系统筹

四层金字塔的落点、门禁与 CI 作业矩阵一表清；缺口登记在 §4。

## 1. 分层与落点

| 层 | 落点 | 运行面 | 门禁 |
|---|---|---|---|
| 单元测试 | 各 crate `src/**/tests`（`#[cfg(test)]`，纯函数/手算锚） | `cargo test --workspace --all-targets --exclude alpha-desktop` | 常规测试门禁 + CI `test` 作业 |
| 属性/模糊测试 | `packages/core/tests/proptest_invariants.rs`（L493，9 性质） | 随 `cargo test -p alpha-core`（256 案例/性质） | 同上（默认启用） |
| 集成/契约测试 | `packages/core/tests/`、`packages/storage/tests/redis_streams.rs`（REDIS_TEST_URL 门控）、`services/data-engine/tests/`、`services/real-time-feed/tests/`、`desktop/tests/`（配置/接线契约）、`mobile/tests/android_shell_contract.rs`、`wasm-analyzer/tests/` | 各 crate `cargo test`（集成）+ 专项门禁 | 常规门禁 + `scripts/check-desktop.sh` |
| 端到端 | `scripts/check-e2e.sh`（L469 四服务链路 + L491 WS 消息级深化） | 真实进程 + Redis，五段断言 | CI `e2e` 作业（ubuntu/macOS 矩阵） |
| 性能基准 | `packages/core/benches/indicators_bench.rs`（L492，criterion 8 项） | `cargo bench` | `scripts/check-perf.sh` 基线比对（L495） |
| 内存安全 | Miri 解释执行（L496） | `scripts/check-miri.sh`（alpha-core，种子 0..5） | CI `miri` 作业 |
| 覆盖率 | llvm-cov 汇总（L494） | `scripts/check-coverage.sh` | CI `coverage` 作业（不设阈值） |

## 2. 前端测试面（web/desktop 共用前端）

- vitest 单测：`web/app/src/lib/*.test.ts`（纯函数：指标口径/liveFeed 协议
  解析/工作区归约器/PWA 策略），node 环境，`npx vitest run`；
- `npx tsc -b` 类型面 + `vite build` 构建门禁（`scripts/check-web.sh`）；
- Android JVM：`:app:testPlayDebugUnitTest`（54 用例，纯逻辑不触框架类）。

## 3. WS 消息级契约（L491 补深，L469 缺口闭环）

`check-e2e.sh` 断言 3b：连 `/ws` → Subscribe + Resync(from_seq=0) →
**无论通道是否有数据必须回帧**（Sync Full 快照或显式 Error），超时静默 =
协议破坏；Sync 帧校验 channel 回显/seq 数值/op ∈ \{full,delta\}/data 在位
（op 大小写不敏感匹配——线上帧是 PascalCase variant 名）。
线上帧型 = serde variant 原名 **PascalCase**（`WsMessage` tag 无 rename，
探针实测 `{"type":"Resync",...}`）——该事实同时修正了 web liveFeed 的入站
解析（大小写不敏感）与出站帧型（L499 修复面）。

## 4. 边界与后续（诚实登记）

- **数据面 e2e**：XADD 行情 → 广播 Delta → 客户端合成收敛的全链路断言未做
  （依赖 collector envelope 注入链）；当前 Resync 断言走 Error 回帧路径
  （e2e 无行情注入时通道未建立），Full 快照路径由 real-time-feed 单测
  （`test_server_delta_applies_on_client_engine`）锁定；
- **Windows 运行期 e2e**：随 L469 登记不变（缺 Redis 简单获取途径）；
- 测试量口径：只到门禁量，不开覆盖率批次（用户约束）。
