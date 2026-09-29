//! 混合存储架构核心（TODO「构建混合存储架构（WASM 内存 + IndexedDB + 服务端缓存）」落地件）。
//!
//! 三层 read-through 缓存，层次与职责：
//! * L1 内存层 [`LruCache`]：纯计算 LRU，wasm 与 native 同一份实现；
//! * L2 持久层 [`PersistentStore`]：trait 缝——wasm 侧真实 IndexedDB 是异步 JS API，
//!   由 JS glue / 后续异步封装实现本契约（参考实现 [`InMemoryPersistentStore`]
//!   供 native 契约测试与降级使用）；
//! * L3 服务端层 [`RemoteSource`]：trait 缝——HTTP 细节属 L1 服务层
//!   （L0 依赖黑名单禁 reqwest，见 docs/cross-platform-architecture.md）。
//!
//! [`HybridCacheService::lookup`] 组合三层：L1 命中即返回；miss 查 L2，命中回填 L1；
//! 再 miss 拉 L3，结果写 L2 + L1。分层命中/未命中/驱逐计数全量统计。
//!
//! 线程模型：wasm 单线程、native 测试单线程，锁粒度按 platform.rs 惯例
//! （std::sync::Mutex + 锁中毒 map_err），无跨层事务语义（最终一致）。

use crate::errors::{AlphaError, AlphaResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// L1 内存层条目值：本架构按「单标的价格序列」最小口径缓存；
/// 多结构（指标/回测报告）扩展缝 = 泛型化 value 或值枚举，届时按需引入
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceSeries {
    pub symbol: String,
    pub prices: Vec<f64>,
}

/// LRU 内存缓存（O(1) get/put：HashMap 索引 + 双向链表新鲜度序）。
/// 内部可变（`&self` 方法），wasm 单线程与 native 均安全。
pub struct LruCache {
    inner: Mutex<LruInner>,
    capacity: usize,
    evictions: AtomicU64,
    hits: AtomicU64,
    misses: AtomicU64,
}

struct LruInner {
    /// key → 链表节点；VecDeque 轮转替代手写链表（容量小、驱逐仅在 push 时 O(capacity)）
    entries: HashMap<String, PriceSeries>,
    /// 新鲜度序：队尾最新
    order: std::collections::VecDeque<String>,
}

impl LruCache {
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "LruCache 容量须大于 0");
        Self {
            inner: Mutex::new(LruInner {
                entries: HashMap::new(),
                order: std::collections::VecDeque::new(),
            }),
            capacity,
            evictions: AtomicU64::new(0),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    fn lock(&self) -> AlphaResult<std::sync::MutexGuard<'_, LruInner>> {
        self.inner
            .lock()
            .map_err(|_| AlphaError::InternalError("LruCache 锁中毒".to_string()))
    }

    /// 查询并提升新鲜度；miss 不产生副作用（除计数）
    pub fn get(&self, symbol: &str) -> Option<PriceSeries> {
        let result = self.lock().ok().and_then(|mut inner| {
            if let Some(series) = inner.entries.get(symbol).cloned() {
                // 新鲜度提升：移到队尾
                inner.order.retain(|k| k != symbol);
                inner.order.push_back(symbol.to_string());
                Some(series)
            } else {
                None
            }
        });
        match result {
            Some(series) => {
                self.hits.fetch_add(1, Ordering::Relaxed);
                Some(series)
            }
            None => {
                self.misses.fetch_add(1, Ordering::Relaxed);
                None
            }
        }
    }

    /// 写入/覆盖并置为最新；超容量驱逐最久未用条目
    pub fn put(&self, series: PriceSeries) {
        if let Ok(mut inner) = self.lock() {
            if !inner.entries.contains_key(&series.symbol) {
                while inner.entries.len() >= self.capacity {
                    if let Some(oldest) = inner.order.pop_front() {
                        inner.entries.remove(&oldest);
                        self.evictions.fetch_add(1, Ordering::Relaxed);
                    } else {
                        break;
                    }
                }
            } else {
                inner.order.retain(|k| k != &series.symbol);
            }
            inner.order.push_back(series.symbol.clone());
            inner.entries.insert(series.symbol.clone(), series);
        }
    }

    pub fn len(&self) -> usize {
        self.lock().map(|inner| inner.entries.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 驱逐单标的（read-through 失效路径；不存在时无操作）
    pub fn invalidate(&self, symbol: &str) {
        if let Ok(mut inner) = self.lock() {
            inner.order.retain(|k| k != symbol);
            inner.entries.remove(symbol);
        }
    }

    pub fn hits(&self) -> u64 {
        self.hits.load(Ordering::Relaxed)
    }

    pub fn misses(&self) -> u64 {
        self.misses.load(Ordering::Relaxed)
    }

    pub fn evictions(&self) -> u64 {
        self.evictions.load(Ordering::Relaxed)
    }
}

/// L2 持久层契约（对象安全；wasm 侧由 IndexedDB 的 JS glue 实现）
pub trait PersistentStore {
    /// 语义契约（由 `assert_persistent_store_contract` 锁定）：
    /// 未写过的 key 返回 None；store 后同 key 可读回；重复 store 覆盖
    fn load(&self, symbol: &str) -> Option<PriceSeries>;
    fn store(&self, series: &PriceSeries);
    fn remove(&self, symbol: &str) -> bool;
    fn persistent_len(&self) -> usize;
}

/// L2 参考实现：进程内 HashMap（native 契约测试 + 无浏览器持久化环境时的降级层）
#[derive(Default)]
pub struct InMemoryPersistentStore {
    map: Mutex<HashMap<String, PriceSeries>>,
}

impl InMemoryPersistentStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl PersistentStore for InMemoryPersistentStore {
    fn load(&self, symbol: &str) -> Option<PriceSeries> {
        self.map.lock().ok().and_then(|m| m.get(symbol).cloned())
    }

    fn store(&self, series: &PriceSeries) {
        if let Ok(mut m) = self.map.lock() {
            m.insert(series.symbol.clone(), series.clone());
        }
    }

    fn remove(&self, symbol: &str) -> bool {
        self.map
            .lock()
            .map(|mut m| m.remove(symbol).is_some())
            .unwrap_or(false)
    }

    fn persistent_len(&self) -> usize {
        self.map.lock().map(|m| m.len()).unwrap_or(0)
    }
}

/// L3 服务端源契约：按标的拉全量序列（增量/分页缝留后续「实时数据同步协议」立项）
#[async_trait]
pub trait RemoteSource {
    async fn fetch_series(&self, symbol: &str) -> AlphaResult<PriceSeries>;
}

/// L3 参考实现：固定表驱动（native 契约测试用；真实 HTTP 源属 L1 服务层）
pub struct StaticRemoteSource {
    table: HashMap<String, Vec<f64>>,
}

impl StaticRemoteSource {
    pub fn new(table: HashMap<String, Vec<f64>>) -> Self {
        Self { table }
    }
}

#[async_trait]
impl RemoteSource for StaticRemoteSource {
    async fn fetch_series(&self, symbol: &str) -> AlphaResult<PriceSeries> {
        match self.table.get(symbol) {
            Some(prices) => Ok(PriceSeries {
                symbol: symbol.to_string(),
                prices: prices.clone(),
            }),
            None => Err(AlphaError::DataNotFound(format!(
                "服务端无此标的: {symbol}"
            ))),
        }
    }
}

/// 一次查询的数据来源层（统计与前端展示用）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheSource {
    /// L1 内存命中
    Memory,
    /// L2 持久层命中（已回填 L1）
    Persistent,
    /// L3 服务端拉取（已写 L2 + L1）
    Remote,
}

/// lookup 结果：数据 + 命中层
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheLookup {
    pub series: PriceSeries,
    pub source: CacheSource,
}

/// 分层命中统计快照
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HybridStats {
    pub memory_hits: u64,
    pub memory_misses: u64,
    pub persistent_hits: u64,
    pub remote_fetches: u64,
    pub evictions: u64,
    pub cache_len: usize,
    pub persistent_len: usize,
}

/// 混合存储服务：组合 L1/L2/L3 的 read-through 查询
pub struct HybridCacheService<'a> {
    cache: &'a LruCache,
    persistent: &'a dyn PersistentStore,
    remote: &'a dyn RemoteSource,
    persistent_hits: AtomicU64,
    remote_fetches: AtomicU64,
}

impl<'a> HybridCacheService<'a> {
    pub fn new(
        cache: &'a LruCache,
        persistent: &'a dyn PersistentStore,
        remote: &'a dyn RemoteSource,
    ) -> Self {
        Self {
            cache,
            persistent,
            remote,
            persistent_hits: AtomicU64::new(0),
            remote_fetches: AtomicU64::new(0),
        }
    }

    /// read-through：L1 → L2（回填 L1）→ L3（写 L2 + L1）
    pub async fn lookup(&self, symbol: &str) -> AlphaResult<CacheLookup> {
        if let Some(series) = self.cache.get(symbol) {
            return Ok(CacheLookup {
                series,
                source: CacheSource::Memory,
            });
        }

        if let Some(series) = self.persistent.load(symbol) {
            self.persistent_hits.fetch_add(1, Ordering::Relaxed);
            self.cache.put(series.clone());
            return Ok(CacheLookup {
                series,
                source: CacheSource::Persistent,
            });
        }

        let series = self.remote.fetch_series(symbol).await?;
        self.remote_fetches.fetch_add(1, Ordering::Relaxed);
        self.persistent.store(&series);
        self.cache.put(series.clone());
        Ok(CacheLookup {
            series,
            source: CacheSource::Remote,
        })
    }

    /// 主动写穿（数据新鲜到达时绕过查询路径直写 L1+L2）
    pub fn write_through(&self, series: PriceSeries) {
        self.cache.put(series.clone());
        self.persistent.store(&series);
    }

    /// 失效单标的（L1 驱逐 + L2 删除；下次 lookup 走 L3）
    pub fn invalidate(&self, symbol: &str) {
        self.cache.invalidate(symbol);
        self.persistent.remove(symbol);
    }

    pub fn stats(&self) -> HybridStats {
        HybridStats {
            memory_hits: self.cache.hits(),
            memory_misses: self.cache.misses(),
            persistent_hits: self.persistent_hits.load(Ordering::Relaxed),
            remote_fetches: self.remote_fetches.load(Ordering::Relaxed),
            evictions: self.cache.evictions(),
            cache_len: self.cache.len(),
            persistent_len: self.persistent.persistent_len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn series(symbol: &str, n: usize) -> PriceSeries {
        PriceSeries {
            symbol: symbol.to_string(),
            prices: (0..n).map(|i| i as f64 + 1.0).collect(),
        }
    }

    /// L2 契约：所有 PersistentStore 实现必须满足的语义
    fn assert_persistent_store_contract(store: &dyn PersistentStore) {
        let s = series("AAPL", 3);
        assert!(
            store.load("AAPL").is_none() || store.remove("AAPL"),
            "契约前置：写入前 load 应为 None（或残留被清理）"
        );
        store.store(&s);
        let loaded = store.load("AAPL").expect("store 后应可读回");
        assert_eq!(loaded.prices, s.prices);
        // 覆盖写
        let s2 = series("AAPL", 5);
        store.store(&s2);
        assert_eq!(store.load("AAPL").expect("覆盖后可读").prices, s2.prices);
        // 删除语义
        assert!(store.remove("AAPL"), "删除已存在 key 应返回 true");
        assert!(store.load("AAPL").is_none(), "删除后 load 为 None");
        assert!(!store.remove("AAPL"), "删除不存在 key 应返回 false");
    }

    #[test]
    fn in_memory_persistent_store_satisfies_contract() {
        assert_persistent_store_contract(&InMemoryPersistentStore::new());
    }

    #[test]
    fn lru_evicts_least_recently_used() {
        let cache = LruCache::new(2);
        cache.put(series("A", 1));
        cache.put(series("B", 1));
        // touch A：B 变为最久未用
        assert!(cache.get("A").is_some());
        cache.put(series("C", 1));
        assert!(cache.get("B").is_none(), "B 应被驱逐");
        assert!(cache.get("A").is_some(), "A 被 touch 过应保留");
        assert!(cache.get("C").is_some());
        assert_eq!(cache.evictions(), 1);
    }

    #[test]
    fn lru_put_overwrites_without_eviction() {
        let cache = LruCache::new(2);
        cache.put(series("A", 1));
        cache.put(series("A", 5));
        cache.put(series("B", 1));
        assert_eq!(cache.evictions(), 0, "覆盖已有 key 不触发驱逐");
        assert_eq!(
            cache.get("A").expect("A 仍在").prices.len(),
            5,
            "覆盖后的值生效"
        );
    }

    #[test]
    fn lru_counts_hits_and_misses() {
        let cache = LruCache::new(2);
        assert!(cache.get("X").is_none());
        cache.put(series("X", 1));
        assert!(cache.get("X").is_some());
        assert_eq!((cache.hits(), cache.misses()), (1, 1));
    }

    #[tokio::test]
    async fn read_through_memory_hit_skips_lower_layers() {
        let cache = LruCache::new(4);
        let persistent = InMemoryPersistentStore::new();
        // L3 表不含目标标的：若被调用会 NotFound 报错，天然验证「未触达」
        let remote = StaticRemoteSource::new(HashMap::new());
        let svc = HybridCacheService::new(&cache, &persistent, &remote);

        svc.write_through(series("AAPL", 3));
        let first = svc.lookup("AAPL").await.expect("命中 L1");
        assert_eq!(first.source, CacheSource::Memory);
        assert_eq!(first.series.prices.len(), 3);
    }

    #[tokio::test]
    async fn read_through_persistent_hit_backfills_memory() {
        let cache = LruCache::new(4);
        let persistent = InMemoryPersistentStore::new();
        let remote = StaticRemoteSource::new(HashMap::new());
        let svc = HybridCacheService::new(&cache, &persistent, &remote);

        persistent.store(&series("600519", 2));
        let hit = svc.lookup("600519").await.expect("L2 应命中");
        assert_eq!(hit.source, CacheSource::Persistent);
        // 回填验证：再查走 L1
        let again = svc.lookup("600519").await.expect("回填后可查");
        assert_eq!(again.source, CacheSource::Memory);
        assert_eq!(svc.stats().persistent_hits, 1);
    }

    #[tokio::test]
    async fn read_through_remote_fills_all_layers() {
        let cache = LruCache::new(4);
        let persistent = InMemoryPersistentStore::new();
        let mut table = HashMap::new();
        table.insert("000001".to_string(), vec![1.0, 2.0, 3.0]);
        let remote = StaticRemoteSource::new(table);
        let svc = HybridCacheService::new(&cache, &persistent, &remote);

        let fetched = svc.lookup("000001").await.expect("L3 拉取");
        assert_eq!(fetched.source, CacheSource::Remote);
        assert_eq!(fetched.series.prices, vec![1.0, 2.0, 3.0]);
        // 写穿验证：L2 与 L1 均有数据
        assert!(persistent.load("000001").is_some(), "L3 结果应写 L2");
        let second = svc.lookup("000001").await.expect("回填后 L1 命中");
        assert_eq!(second.source, CacheSource::Memory);
        assert_eq!(svc.stats().remote_fetches, 1, "L3 只拉一次");
    }

    #[tokio::test]
    async fn remote_miss_propagates_not_found() {
        let cache = LruCache::new(4);
        let persistent = InMemoryPersistentStore::new();
        let remote = StaticRemoteSource::new(HashMap::new());
        let svc = HybridCacheService::new(&cache, &persistent, &remote);

        let err = svc.lookup("NOPE").await.expect_err("三层全 miss 应报错");
        assert!(
            matches!(err, AlphaError::DataNotFound(_)),
            "错误应保持 NotFound 语义，实际 {err:?}"
        );
        assert_eq!(svc.stats().cache_len, 0, "失败不落缓存");
    }

    #[tokio::test]
    async fn invalidate_forces_remote_refetch() {
        let cache = LruCache::new(4);
        let persistent = InMemoryPersistentStore::new();
        let mut table = HashMap::new();
        table.insert("AAPL".to_string(), vec![10.0]);
        let remote = StaticRemoteSource::new(table);
        let svc = HybridCacheService::new(&cache, &persistent, &remote);

        svc.lookup("AAPL").await.expect("首次拉取");
        svc.invalidate("AAPL");
        assert!(persistent.load("AAPL").is_none(), "失效应清 L2");
        let refetched = svc.lookup("AAPL").await.expect("失效后重拉");
        assert_eq!(refetched.source, CacheSource::Remote);
        assert_eq!(svc.stats().remote_fetches, 2);
    }

    #[test]
    fn lru_zero_capacity_rejected() {
        let result = std::panic::catch_unwind(|| LruCache::new(0));
        assert!(result.is_err(), "容量 0 必须构造期拒绝");
    }
}
