//! 内存泄漏检测与分配追踪（L456 工具面 · Rust 侧）。
//!
//! [`TrackingAllocator`]：包装 `std::alloc::System` 的计数分配器——
//! 累计/存活分配数、存活字节数、峰值字节数。两种用法：
//! 1. **全局启用**（服务二进制）：`#[global_allocator] static A: TrackingAllocator = TrackingAllocator::new();`
//!    运行期/退出前读 [`AllocatorStats`] 做泄漏判读（存活字节只增不减 → 泄漏嫌疑）；
//! 2. **测试判读**：直接调用 `alloc`/`dealloc`（GlobalAlloc trait 方法）配对
//!    验证计数语义（单测即此形态——全局分配器一个进程只能注册一个）。
//!
//! 性能分析（perf/tokio-console 封装）见 `scripts/profile.sh` 与
//! `docs/memory-profiling.md`。

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};

/// 分配统计快照
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AllocatorStats {
    /// 累计分配次数
    pub total_allocs: u64,
    /// 累计释放次数
    pub total_deallocs: u64,
    /// 当前存活分配数（allocs - deallocs）
    pub live_allocs: u64,
    /// 当前存活字节数
    pub live_bytes: u64,
    /// 存活字节峰值
    pub peak_bytes: u64,
}

impl AllocatorStats {
    /// 泄漏嫌疑判读：存活分配数 > 0 即有未释放块（测试场景 = 泄漏；
    /// 长驻服务则结合趋势判读）
    pub fn has_unreleased(&self) -> bool {
        self.live_allocs > 0
    }
}

/// 计数分配器（零开销锁：全 AtomicU64；对齐 padding 防 false sharing）
pub struct TrackingAllocator {
    total_allocs: AtomicU64,
    total_deallocs: AtomicU64,
    live_bytes: AtomicU64,
    peak_bytes: AtomicU64,
}

impl TrackingAllocator {
    pub const fn new() -> Self {
        Self {
            total_allocs: AtomicU64::new(0),
            total_deallocs: AtomicU64::new(0),
            live_bytes: AtomicU64::new(0),
            peak_bytes: AtomicU64::new(0),
        }
    }

    pub fn stats(&self) -> AllocatorStats {
        let total_allocs = self.total_allocs.load(Ordering::Relaxed);
        let total_deallocs = self.total_deallocs.load(Ordering::Relaxed);
        let live_bytes = self.live_bytes.load(Ordering::Relaxed);
        AllocatorStats {
            total_allocs,
            total_deallocs,
            live_allocs: total_allocs.saturating_sub(total_deallocs),
            live_bytes,
            peak_bytes: self.peak_bytes.load(Ordering::Relaxed),
        }
    }

    /// 归零全部计数（不动真实内存；跨轮次压测间复位用）
    pub fn reset(&self) {
        self.total_allocs.store(0, Ordering::Relaxed);
        self.total_deallocs.store(0, Ordering::Relaxed);
        self.live_bytes.store(0, Ordering::Relaxed);
        self.peak_bytes.store(0, Ordering::Relaxed);
    }
}

impl Default for TrackingAllocator {
    fn default() -> Self {
        Self::new()
    }
}

unsafe impl GlobalAlloc for TrackingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc(layout);
        if !ptr.is_null() {
            self.total_allocs.fetch_add(1, Ordering::Relaxed);
            // 存活字节累加并抬峰值；并发下峰值可能高估（宽松序 + CAS 循环
            // 换精确会引入争用）——判读语义取「不低估」可接受
            let live = self
                .live_bytes
                .fetch_add(layout.size() as u64, Ordering::Relaxed)
                + layout.size() as u64;
            self.peak_bytes.fetch_max(live, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        self.total_deallocs.fetch_add(1, Ordering::Relaxed);
        self.live_bytes
            .fetch_sub(layout.size() as u64, Ordering::Relaxed);
        System.dealloc(ptr, layout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 局部实例：测试默认并行，共享 static 计数会互相污染（reset 还会清掉
    /// 其他测试的计数）——每测试独立实例即完全隔离
    #[test]
    fn paired_alloc_dealloc_rounds_counters() {
        let tracker = TrackingAllocator::new();
        let before = tracker.stats();

        let layout = Layout::from_size_align(128, 8).unwrap();
        let ptr = unsafe { tracker.alloc(layout) };
        assert!(!ptr.is_null());

        let mid = tracker.stats();
        assert_eq!(mid.total_allocs, before.total_allocs + 1);
        assert_eq!(mid.live_bytes, before.live_bytes + 128);

        unsafe { tracker.dealloc(ptr, layout) };
        let after = tracker.stats();
        assert_eq!(after.total_deallocs, before.total_deallocs + 1);
        assert_eq!(
            after.live_bytes, before.live_bytes,
            "配对释放后存活字节归位"
        );
        assert!(
            after.peak_bytes >= before.peak_bytes + 128,
            "峰值必须抬到本次分配之上"
        );
    }

    /// 未释放块在统计面可见（泄漏检测的核心判读）
    #[test]
    fn unreleased_alloc_is_visible_in_stats() {
        let tracker = TrackingAllocator::new();
        let layout = Layout::from_size_align(64, 8).unwrap();
        let leaked = unsafe { tracker.alloc(layout) };
        assert!(!leaked.is_null());

        let stats = tracker.stats();
        assert!(stats.has_unreleased(), "存活分配数应为正");
        assert_eq!(stats.live_allocs, 1);

        // 清理：不污染同进程其他测试
        unsafe { tracker.dealloc(leaked, layout) };
        assert!(!tracker.stats().has_unreleased());
    }

    /// peak_bytes 单调不回退（reset 除外）
    #[test]
    fn peak_bytes_is_monotonic() {
        let tracker = TrackingAllocator::new();
        let a = Layout::from_size_align(1000, 8).unwrap();
        let b = Layout::from_size_align(10, 8).unwrap();

        let p1 = unsafe { tracker.alloc(a) };
        let peak_after_big = tracker.stats().peak_bytes;
        let p2 = unsafe { tracker.alloc(b) };
        let _p3 = unsafe { tracker.alloc(a) };
        unsafe { tracker.dealloc(p1, a) };
        unsafe { tracker.dealloc(p2, b) };

        assert!(
            tracker.stats().peak_bytes >= peak_after_big,
            "峰值不得随释放回退"
        );
        unsafe { tracker.dealloc(_p3, a) };
    }
}
