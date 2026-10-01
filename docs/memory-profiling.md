# 内存泄漏检测与性能分析工具（TODO L456）

工具面三件：**TrackingAllocator**（Rust 分配计数/泄漏判读）、
**scripts/profile.sh**（perf 采样封装）、登记项（tokio-console / valgrind）。

## 1. TrackingAllocator（packages/core::alloc_tracking）

包装 `std::alloc::System` 的计数分配器：累计分配/释放次数、存活字节数、
峰值字节数（全 AtomicU64，锁-free）。

```rust
use alpha_core::alloc_tracking::TrackingAllocator;

// 服务二进制全局启用（一个进程只能注册一个 global allocator）
#[global_allocator]
static ALLOCATOR: TrackingAllocator = TrackingAllocator::new();

// 运行期/优雅退出前判读
let stats = ALLOCATOR.stats();
// stats.live_bytes 只增不减 + live_allocs 单调上行 → 泄漏嫌疑；
// 稳态服务应看趋势：峰值稳定在水位线 = 健康
```

判读语义：
- `has_unreleased()`：存活分配数 > 0——**测试场景即泄漏**；长驻服务须结合
  趋势（短生命周期对象在采样窗口内会自然存活）。
- `peak_bytes` 单调不回退（reset 除外）；并发下可能高估（宽松序设计，
  「不低估」的判读取向）。
- `reset()`：跨轮次压测间复位计数，不动真实内存。

单测锁定：配对 alloc/dealloc 计数往返、未释放块在统计面可见、峰值单调。

## 2. scripts/profile.sh（CPU 画像，Linux）

```bash
scripts/profile.sh record target/release/alpha-api-gateway   # 采样 → target/perf.data
scripts/profile.sh report                                    # 交互式热点/调用栈
scripts/profile.sh top target/release/alpha-data-engine      # 实时热点
```

- `-g --call-graph dwarf`：用户态完整调用栈（release 二进制建议保留
  `debug = "line-tables-only"` 或 `debuginfo` 以获得可读栈）。
- 前置：`linux-tools-$(uname -r)`；容器内可能受 `perf_event_paranoid`
  限制（报权限时降级 `sudo` 或调内核参数）。
- 服务定位与 docs/pgo-build-optimization.md §3 负载画像共用同一套回放集。

## 3. 登记项（不阻塞本单，随对应工程项启用）

| 工具 | 用途 | 启用条件 |
|---|---|---|
| tokio-console | 任务级延迟/阻塞画像 | 服务构建带 `--cfg tokio_unstable`（发布节决定是否默认） |
| valgrind --leak-check | 堤外兜底的精确泄漏定位 | 长跑复现后按需（比计数器慢两个量级） |
| heaptrack / bytehound | 分配热点与火焰图 | CI 外按需 |

## 4. 边界

1. wasm32 侧不走 TrackingAllocator（浏览器内存由 JS 堆承载，
   `performance.memory` / DevTools 承担画像）——本工具面为 native 服务/桌面。
2. TrackingAllocator 不做 backtrace 采集（每分配一次栈回溯开销不可接受）；
   定位到嫌疑点后用 valgrind/heaptrack 深挖。
3. 全局注册互斥：服务若同时启用其他 third-party 计数分配器只能二选一。
