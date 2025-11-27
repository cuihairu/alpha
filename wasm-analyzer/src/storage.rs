//! IndexedDB 存储层
//!
//! 提供浏览器本地持久化存储能力（简化版）

use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

const DB_NAME: &str = "alpha_market_data";
const DB_VERSION: u32 = 1;

/// IndexedDB 存储管理器
#[wasm_bindgen]
pub struct IndexedDBStorage {
    db_name: String,
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


/// 混合存储策略管理器
#[wasm_bindgen]
pub struct HybridStorage {
    /// IndexedDB 存储
    indexed_db: IndexedDBStorage,
    /// 内存缓存大小限制（条目数）
    cache_limit: usize,
}

#[wasm_bindgen]
impl HybridStorage {
    /// 创建混合存储管理器
    #[wasm_bindgen(constructor)]
    pub fn new(cache_limit: usize) -> HybridStorage {
        HybridStorage {
            indexed_db: IndexedDBStorage::new(),
            cache_limit,
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
        self.cache_limit
    }

    /// 获取存储统计
    #[wasm_bindgen(js_name = getStorageStats)]
    pub fn get_storage_stats(&self) -> JsValue {
        let stats = serde_json::json!({
            "database": self.indexed_db.get_database_name(),
            "cache_limit": self.cache_limit,
            "mode": "hybrid",
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
    fn test_hybrid_storage() {
        let storage = HybridStorage::new(1000);
        assert_eq!(storage.cache_limit, 1000);
    }

    #[test]
    fn test_stored_market_data() {
        let wrapper = StoredMarketDataWrapper::new("AAPL".to_string(), 1000, 150.0, 1000);
        assert_eq!(wrapper.symbol(), "AAPL");
        assert_eq!(wrapper.price(), 150.0);
    }
}
