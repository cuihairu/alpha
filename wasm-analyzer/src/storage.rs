//! 混合存储层（IndexedDB 持久化 + WASM 内存 LRU）
//!
//! TODO「构建混合存储架构（WASM 内存 + IndexedDB + 服务端缓存）」wasm 侧落地件。
//! 分层与职责（三层 read-through 组合逻辑在 alpha-core `hybrid_cache`，单测覆盖）：
//! * L1 内存层：alpha_core::hybrid_cache::LruCache（本模块 [`HybridStorage`] 内嵌）；
//! * L2 IndexedDB：浏览器异步 JS API——真实读写由 JS glue 实现
//!   `alpha_core::hybrid_cache::PersistentStore` 契约（语义由 core 契约测试锁定，
//!   本模块保留库/表元信息导出供 glue 对齐 schema）；
//! * L3 服务端缓存：`RemoteSource` trait 缝，HTTP 细节属 L1 服务层（依赖黑名单禁
//!   reqwest），前端经既有 REST/WS 通道拉数后 writeThrough 写入本层。

use alpha_core::hybrid_cache::{LruCache, PriceSeries};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

const DB_NAME: &str = "alpha_market_data";
const DB_VERSION: u32 = 1;

/// IndexedDB 存储管理器
#[wasm_bindgen]
pub struct IndexedDBStorage {
    db_name: String,
}

impl Default for IndexedDBStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl IndexedDBStorage {
    /// 创建新的存储管理器
    #[wasm_bindgen(constructor)]
    pub fn new() -> IndexedDBStorage {
        IndexedDBStorage {
            db_name: DB_NAME.to_string(),
        }
    }

    /// 使用自定义数据库名称
    #[wasm_bindgen(js_name = withName)]
    pub fn with_name(name: &str) -> IndexedDBStorage {
        IndexedDBStorage {
            db_name: name.to_string(),
        }
    }

    /// 初始化数据库（需要在 JavaScript 中调用）
    #[wasm_bindgen(js_name = initDatabase)]
    pub fn init_database(&self) -> JsValue {
        let info = serde_json::json!({
            "database": self.db_name,
            "version": DB_VERSION,
            "status": "ready",
            "message": "请在 JavaScript 中使用 IndexedDB API 进行初始化"
        });

        serde_wasm_bindgen::to_value(&info).unwrap_or(JsValue::NULL)
    }

    /// 获取数据库名称
    #[wasm_bindgen(js_name = getDatabaseName)]
    pub fn get_database_name(&self) -> String {
        self.db_name.clone()
    }

    /// 获取数据库版本
    #[wasm_bindgen(js_name = getDatabaseVersion)]
    pub fn get_database_version(&self) -> u32 {
        DB_VERSION
    }

    /// 获取统计信息
    #[wasm_bindgen(js_name = getStats)]
    pub fn get_stats(&self) -> JsValue {
        let stats = serde_json::json!({
            "database": self.db_name,
            "version": DB_VERSION,
            "stores": ["market_data", "indicators"],
        });

        serde_wasm_bindgen::to_value(&stats).unwrap_or(JsValue::NULL)
    }
}

/// 存储的市场数据结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredMarketData {
    pub symbol: String,
    pub timestamp: i64,
    pub price: f64,
    pub volume: u64,
}

#[wasm_bindgen]
pub struct StoredMarketDataWrapper {
    data: StoredMarketData,
}

#[wasm_bindgen]
impl StoredMarketDataWrapper {
    #[wasm_bindgen(constructor)]
    pub fn new(symbol: String, timestamp: i64, price: f64, volume: u64) -> StoredMarketDataWrapper {
        StoredMarketDataWrapper {
            data: StoredMarketData {
                symbol,
                timestamp,
                price,
                volume,
            },
        }
    }

    #[wasm_bindgen(js_name = toJSON)]
    pub fn to_json(&self) -> JsValue {
        serde_wasm_bindgen::to_value(&self.data).unwrap_or(JsValue::NULL)
    }

    #[wasm_bindgen(getter)]
    pub fn symbol(&self) -> String {
        self.data.symbol.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn timestamp(&self) -> i64 {
        self.data.timestamp
    }

    #[wasm_bindgen(getter)]
    pub fn price(&self) -> f64 {
        self.data.price
    }

    #[wasm_bindgen(getter)]
    pub fn volume(&self) -> f64 {
        self.data.volume as f64
    }
}

/// 混合存储策略管理器（L1 内存 LRU 真行为 + L2/L3 元信息与契约缝）
#[wasm_bindgen]
pub struct HybridStorage {
    /// IndexedDB 存储（元信息 + JS glue 对齐 schema 用）
    indexed_db: IndexedDBStorage,
    /// L1 内存层（逻辑在 alpha_core::hybrid_cache，native/wasm 同一份实现）
    lru: LruCache,
}

#[wasm_bindgen]
impl HybridStorage {
    /// 创建混合存储管理器；`cache_limit` 为 L1 条目数上限（须 > 0）
    #[wasm_bindgen(constructor)]
    pub fn new(cache_limit: usize) -> HybridStorage {
        HybridStorage {
            indexed_db: IndexedDBStorage::new(),
            lru: LruCache::new(cache_limit),
        }
    }

    /// 初始化存储
    #[wasm_bindgen(js_name = init)]
    pub fn init(&self) -> JsValue {
        self.indexed_db.init_database()
    }

    /// 获取数据库名称
    #[wasm_bindgen(js_name = getDatabaseName)]
    pub fn get_database_name(&self) -> String {
        self.indexed_db.get_database_name()
    }

    /// 获取缓存限制
    #[wasm_bindgen(js_name = getCacheLimit)]
    pub fn get_cache_limit(&self) -> usize {
        self.lru.capacity()
    }

    /// 写穿 L1（数据由服务端/WS 到达时调用；L2 持久化由 JS glue 同步执行）
    #[wasm_bindgen(js_name = putPrices)]
    pub fn put_prices(&self, symbol: &str, prices_js: &js_sys::Float64Array) {
        let series = PriceSeries {
            symbol: symbol.to_string(),
            prices: prices_js.to_vec(),
        };
        self.lru.put(series);
    }

    /// 查 L1：命中返回 Float64Array，miss 返回 NULL
    /// （miss 后的 L2/L3 回填组合逻辑见 alpha_core::hybrid_cache::HybridCacheService）
    #[wasm_bindgen(js_name = getPrices)]
    pub fn get_prices(&self, symbol: &str) -> JsValue {
        match self.lru.get(symbol) {
            Some(series) => JsValue::from(js_sys::Float64Array::from(&series.prices[..])),
            None => JsValue::NULL,
        }
    }

    /// 失效单标的（L1 驱逐；L2 删除由 JS glue 对 PersistentStore 契约实现）
    #[wasm_bindgen(js_name = invalidateSymbol)]
    pub fn invalidate_symbol(&self, symbol: &str) {
        self.lru.invalidate(symbol);
    }

    /// 获取存储统计（含 L1 分层计数；口径见 alpha_core::hybrid_cache::HybridStats）
    #[wasm_bindgen(js_name = getStorageStats)]
    pub fn get_storage_stats(&self) -> JsValue {
        let stats = serde_json::json!({
            "database": self.indexed_db.get_database_name(),
            "cache_limit": self.lru.capacity(),
            "mode": "hybrid",
            "lru_len": self.lru.len(),
            "memory_hits": self.lru.hits(),
            "memory_misses": self.lru.misses(),
            "evictions": self.lru.evictions(),
        });

        serde_wasm_bindgen::to_value(&stats).unwrap_or(JsValue::NULL)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_storage_creation() {
        let storage = IndexedDBStorage::new();
        assert_eq!(storage.db_name, DB_NAME);

        let custom_storage = IndexedDBStorage::with_name("custom_db");
        assert_eq!(custom_storage.db_name, "custom_db");
    }

    #[test]
    fn test_stored_market_data() {
        let wrapper = StoredMarketDataWrapper::new("AAPL".to_string(), 1000, 150.0, 1000);
        assert_eq!(wrapper.symbol(), "AAPL");
        assert_eq!(wrapper.price(), 150.0);
    }

    /// L1 真行为：内部切片口径（native 可测；Float64Array 绑定层仅薄转换）
    fn put_slice(storage: &HybridStorage, symbol: &str, prices: &[f64]) {
        storage.lru.put(PriceSeries {
            symbol: symbol.to_string(),
            prices: prices.to_vec(),
        });
    }

    #[test]
    fn hybrid_storage_l1_put_get_roundtrip() {
        let storage = HybridStorage::new(1000);
        put_slice(&storage, "600519", &[1.0, 2.5, 3.0]);
        let hit = storage.lru.get("600519").expect("写入后应命中");
        assert_eq!(hit.prices, vec![1.0, 2.5, 3.0]);
        assert!(storage.lru.get("MISS").is_none());
        assert_eq!((storage.lru.hits(), storage.lru.misses()), (1, 1));
    }

    #[test]
    fn hybrid_storage_l1_evicts_over_capacity() {
        let storage = HybridStorage::new(2);
        put_slice(&storage, "A", &[1.0]);
        put_slice(&storage, "B", &[2.0]);
        put_slice(&storage, "C", &[3.0]);
        assert!(storage.lru.get("A").is_none(), "最久未用的 A 应被驱逐");
        assert!(storage.lru.get("C").is_some());
        assert_eq!(storage.lru.evictions(), 1);
        assert_eq!(storage.lru.len(), 2, "容量恒为 2");
    }

    #[test]
    fn hybrid_storage_invalidate_removes_entry() {
        let storage = HybridStorage::new(4);
        put_slice(&storage, "X", &[9.0]);
        storage.invalidate_symbol("X");
        assert!(storage.lru.get("X").is_none(), "失效后 miss");
        assert_eq!(storage.lru.len(), 0);
        // 失效不存在标的：无操作不 panic
        storage.invalidate_symbol("GHOST");
    }

    #[test]
    fn hybrid_storage_stats_reflect_counters() {
        let storage = HybridStorage::new(3);
        put_slice(&storage, "S1", &[1.0]);
        let _ = storage.lru.get("S1");
        let _ = storage.lru.get("S2");
        // getStorageStats 的 JSON 装配依赖 js_sys（native 下 panic），
        // wasm 边界装配由 wasm_bindgen_test 覆盖；此处验同源内部计数器
        assert_eq!(storage.lru.hits(), 1);
        assert_eq!(storage.lru.misses(), 1);
        assert_eq!(storage.lru.len(), 1);
    }

    /// JS 边界（wasm32 浏览器跑；本地门禁做编译验证）：
    /// putPrices/getPrices/统计装配的 wasm_bindgen 导出真路径
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test::wasm_bindgen_test]
    fn hybrid_storage_wasm_binding_roundtrip() {
        let storage = HybridStorage::new(2);
        storage.put_prices("600519", &js_sys::Float64Array::from(&[1.0, 2.0, 3.0][..]));
        let got = storage.get_prices("600519");
        let arr = js_sys::Float64Array::from(js_sys::Object::from(got));
        assert_eq!(arr.length(), 3, "命中返回序列长度");
        assert!(storage.get_prices("MISS").is_null(), "miss 返回 NULL");

        let stats = storage.get_storage_stats();
        let lru_len = js_sys::Reflect::get(&stats, &wasm_bindgen::JsValue::from_str("lru_len"))
            .expect("stats 应含 lru_len");
        assert_eq!(lru_len.as_f64(), Some(1.0));
    }
}
