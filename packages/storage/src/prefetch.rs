//! 智能预取与后台数据同步（L454）。
//!
//! 面向「顺序/等差步进访问」的行情消费模式（按时间槽扫描、按代码表遍历）：
//! 观察到等差步进模式后，在访问到达**之前**把后续键预装入缓存
//! （[`crate::DistributedCache`] cache-aside 回填），后台任务化不阻塞读路径。
//!
//! 分层：
//! * 纯函数层 [`detect_step`] / [`plan_prefetch`] / [`PrefetchWindow`]：
//!   模式检测与预取计划，无 IO 可独立单测；
//! * [`BackgroundPrefetcher`]：tokio 后台执行——在飞去重（同一键不重复
//!   装载）、loader 失败静默降级（预取是优化不是正确性依赖）。

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use alpha_core::errors::{AlphaError, AlphaResult};
use async_trait::async_trait;

use crate::cache::DistributedCache;

/// 等差步进检测：最近 `min(len, WINDOW)` 个键严格等差（公差非零）→ Some(公差)。
/// 严格判定（无噪声容忍）——误预取只浪费 IO，但检测器必须可解释可单测。
pub fn detect_step(series: &[i64]) -> Option<i64> {
    const WINDOW: usize = 4;
    let window = if series.len() > WINDOW {
        &series[series.len() - WINDOW..]
    } else {
        series
    };
    let last = *window.last()?;
    if window.len() < 2 {
        return None;
    }
    let step = last - window[window.len() - 2];
    if step == 0 {
        return None;
    }
    window
        .windows(2)
        .all(|pair| pair[1] - pair[0] == step)
        .then_some(step)
}

/// 预取计划：从 `last_key` 出发按公差 `step` 生成至多 `count` 个未来键
/// （`step` 为 0 返回空——静止序列无事可做）。
pub fn plan_prefetch(last_key: i64, step: i64, count: usize) -> Vec<i64> {
    if step == 0 || count == 0 {
        return Vec::new();
    }
    (1..=count as i64).map(|i| last_key + step * i).collect()
}

/// 访问滑窗（观察序列的线程安全容器）
#[derive(Default)]
pub struct PrefetchWindow {
    keys: Mutex<Vec<i64>>,
}

impl PrefetchWindow {
    pub fn new() -> Self {
        Self::default()
    }

    /// 追加访问键并裁剪到窗口上限（保留最近 `keep` 个）
    pub fn observe(&self, key: i64, keep: usize) -> Vec<i64> {
        let mut keys = self.keys.lock().unwrap_or_else(|e| e.into_inner());
        keys.push(key);
        if keys.len() > keep {
            let drop = keys.len() - keep;
            keys.drain(..drop);
        }
        keys.clone()
    }
}

/// 后台装载契约（调用方实现：查库/查上游/重算皆可）
#[async_trait]
pub trait PrefetchLoader: Send + Sync {
    async fn load(&self, key: i64) -> AlphaResult<i64>;
}

/// 预取统计（观测收益用）
#[derive(Default)]
pub struct PrefetchStats {
    pub observed: AtomicU64,
    pub planned: AtomicU64,
    pub loaded: AtomicU64,
    pub failed: AtomicU64,
}

/// 后台预取器：observe 访问 → 检测步进 → 计划未来键 → 后台回填缓存。
///
/// 在飞去重：同键任务进行中不重复派发；完成（成败皆算）后移除标记，
/// 之后再次访问仍可重试。loader 失败只计数不上抛（预取优化不构成
/// 正确性依赖——读路径 miss 时 cache-aside 自会同步装载）。
pub struct BackgroundPrefetcher<L: PrefetchLoader> {
    cache: DistributedCache,
    loader: Arc<L>,
    window: Arc<PrefetchWindow>,
    in_flight: Arc<Mutex<HashSet<i64>>>,
    stats: Arc<PrefetchStats>,
    /// 每次观察后最多预取的键数
    pub lookahead: usize,
    /// 缓存 TTL（None = cache 默认）
    pub ttl: Option<Duration>,
}

/// 手写 Clone：字段全 Arc/值类型，L 无需 Clone（derive 会错误加 `L: Clone` bound）
impl<L: PrefetchLoader> Clone for BackgroundPrefetcher<L> {
    fn clone(&self) -> Self {
        Self {
            cache: self.cache.clone(),
            loader: Arc::clone(&self.loader),
            window: Arc::clone(&self.window),
            in_flight: Arc::clone(&self.in_flight),
            stats: Arc::clone(&self.stats),
            lookahead: self.lookahead,
            ttl: self.ttl,
        }
    }
}

impl<L: PrefetchLoader + 'static> BackgroundPrefetcher<L> {
    pub fn new(cache: DistributedCache, loader: L, lookahead: usize) -> Self {
        Self {
            cache,
            loader: Arc::new(loader),
            window: Arc::new(PrefetchWindow::new()),
            in_flight: Arc::new(Mutex::new(HashSet::new())),
            stats: Arc::new(PrefetchStats::default()),
            lookahead,
            ttl: None,
        }
    }

    pub fn stats(&self) -> (u64, u64, u64, u64) {
        (
            self.stats.observed.load(Ordering::Relaxed),
            self.stats.planned.load(Ordering::Relaxed),
            self.stats.loaded.load(Ordering::Relaxed),
            self.stats.failed.load(Ordering::Relaxed),
        )
    }

    /// 记录一次访问：检测到步进模式则派发后台预取任务（不阻塞调用方）
    pub fn observe(&self, key: i64) -> AlphaResult<Vec<i64>> {
        let keys = self.window.observe(key, 8);
        self.stats.observed.fetch_add(1, Ordering::Relaxed);

        let Some(step) = detect_step(&keys) else {
            return Ok(Vec::new());
        };
        let plan = plan_prefetch(key, step, self.lookahead);
        if plan.is_empty() {
            return Ok(Vec::new());
        }

        let mut in_flight = self.in_flight.lock().unwrap_or_else(|e| e.into_inner());
        let dispatch: Vec<i64> = plan.into_iter().filter(|k| in_flight.insert(*k)).collect();
        drop(in_flight);
        self.stats
            .planned
            .fetch_add(dispatch.len() as u64, Ordering::Relaxed);

        for key in dispatch.iter().copied() {
            // 显式限定：&self 上的 self.clone() 会解析成 Clone for &Self（借引用
            // 进 spawn 触发 E0521），必须取 <Self as Clone> 的拥有值克隆
            let this = <Self as Clone>::clone(self);
            tokio::spawn(async move {
                // cache-aside 回填：装载成功即写缓存；失败计 failed 并放行重试
                let outcome = this
                    .cache
                    .get_or_load(&slot_key(key), this.ttl, || {
                        let loader = this.loader.clone();
                        async move { loader.load(key).await }
                    })
                    .await;
                match outcome {
                    Ok(_) => this.stats.loaded.fetch_add(1, Ordering::Relaxed),
                    Err(_) => this.stats.failed.fetch_add(1, Ordering::Relaxed),
                };
                this.in_flight
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&key);
            });
        }
        Ok(dispatch)
    }
}

/// 预取槽键（缓存命名空间与业务键拼装）
pub fn slot_key(key: i64) -> String {
    format!("prefetch:slot:{key}")
}

/// 缓存直读（测试/观测预取是否已落缓存）
pub async fn peek_cached(cache: &DistributedCache, key: i64) -> AlphaResult<Option<i64>> {
    cache.get_json::<i64>(&slot_key(key)).await
}

/// loader 错误统一构造（供 mock 与文档示例）
pub fn loader_error(message: &str) -> AlphaError {
    AlphaError::DataNotFound(message.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_step_requires_strict_arithmetic_progression() {
        assert_eq!(detect_step(&[]), None, "空序列无模式");
        assert_eq!(detect_step(&[5]), None, "单点无模式");
        assert_eq!(detect_step(&[1, 2]), Some(1), "两点即成步进");
        assert_eq!(detect_step(&[2, 4, 6, 8]), Some(2));
        assert_eq!(detect_step(&[10, 7, 4, 1]), Some(-3), "负步进（倒序扫描）");
        assert_eq!(detect_step(&[1, 2, 3, 5]), None, "断点破坏等差");
        assert_eq!(detect_step(&[4, 4, 4]), None, "零公差=静止，不预取");
        // 只看最近 4 个：历史噪声不干扰
        assert_eq!(detect_step(&[100, 50, 1, 2, 3, 4]), Some(1));
    }

    #[test]
    fn plan_prefetch_bounds_and_zero_step() {
        assert_eq!(plan_prefetch(10, 2, 3), vec![12, 14, 16]);
        assert_eq!(plan_prefetch(10, -1, 2), vec![9, 8]);
        assert!(plan_prefetch(10, 0, 5).is_empty());
        assert!(plan_prefetch(10, 2, 0).is_empty());
    }

    #[test]
    fn prefetch_window_keeps_recent_keys_only() {
        let window = PrefetchWindow::new();
        for k in 1..=5i64 {
            window.observe(k, 3);
        }
        assert_eq!(window.observe(6, 3), vec![4, 5, 6], "只保留最近 3 个");
    }

    struct CountingLoader {
        calls: AtomicU64,
        fail_on: Vec<i64>,
    }

    impl CountingLoader {
        fn new(fail_on: Vec<i64>) -> Self {
            Self {
                calls: AtomicU64::new(0),
                fail_on,
            }
        }
    }

    #[async_trait]
    impl PrefetchLoader for CountingLoader {
        async fn load(&self, key: i64) -> AlphaResult<i64> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            if self.fail_on.contains(&key) {
                return Err(loader_error("boom"));
            }
            Ok(key * 10)
        }
    }

    async fn wait_until(pred: impl Fn() -> bool, tries: usize) -> bool {
        for _ in 0..tries {
            if pred() {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        pred()
    }

    /// 后台预取端到端（REDIS_TEST_URL 门控）：步进观察后未来键被回填缓存，
    /// 在飞去重保证同键 loader 至多一次，失败键计数不入缓存且可重试
    #[tokio::test]
    async fn background_prefetch_fills_cache_with_dedup() -> AlphaResult<()> {
        let Some(url) = std::env::var("REDIS_TEST_URL").ok() else {
            return Ok(());
        };
        let prefix = format!("alpha:test:prefetch:{}:", uuid::Uuid::new_v4());
        let cache = DistributedCache::connect(&url, &prefix, Duration::from_secs(60)).await?;
        let loader = CountingLoader::new(vec![6]); // 键 6 装载失败
        let prefetcher = BackgroundPrefetcher::new(cache.clone(), loader, 3);

        // 派发时序：k=2 起两点成步进；后续观察的计划键与在飞集合求交去重
        assert_eq!(prefetcher.observe(1)?, Vec::<i64>::new(), "单点无模式");
        assert_eq!(prefetcher.observe(2)?, vec![3, 4, 5], "两点成步进即预取");
        assert_eq!(prefetcher.observe(3)?, vec![6], "4/5 在飞被去重");
        assert_eq!(prefetcher.observe(4)?, vec![7], "5/6 在飞被去重");

        // 等后台收敛：派发 3/4/5/6/7 共 5 次装载，其中键 6 失败
        assert!(
            wait_until(
                || prefetcher.stats().2 == 4 && prefetcher.stats().3 == 1,
                40
            )
            .await,
            "统计未收敛: {:?}",
            prefetcher.stats()
        );
        assert_eq!(prefetcher.stats().1, 5, "计划数 = 累计派发数");

        assert_eq!(peek_cached(&cache, 5).await?, Some(50), "键 5 已回填");
        assert_eq!(peek_cached(&cache, 7).await?, Some(70), "键 7 已回填");
        assert_eq!(peek_cached(&cache, 6).await?, None, "失败键不入缓存");

        // 完成后在飞标记已清除：再次观察，失败键 6 可被重新派发重试
        assert_eq!(
            prefetcher.observe(5)?,
            vec![6, 7, 8],
            "失败键放行重试、已完成键可复用"
        );
        assert!(
            wait_until(
                || prefetcher.stats().2 == 6 && prefetcher.stats().3 == 2,
                40
            )
            .await,
            "重试未收敛: {:?}",
            prefetcher.stats()
        );

        Ok(())
    }
}
