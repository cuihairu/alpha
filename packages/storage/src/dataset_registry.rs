//! 研究数据集与实验登记表（architecture-review §3.5：先落协议与登记表（小），
//! 包级 API 面归 P3）。
//!
//! 契约在 `alpha_protocols::dataset`（纯 serde）；本模块给内存登记表 +
//! 经任意 [`StorageBackend`] 的 KV 持久化（两键：datasets / experiments，
//! serde_json 快照）。登记语义：
//!
//! - 数据集不可变：同 `dataset_id` + `version` 重复注册报冲突（引用语义
//!   建立在「版本内容不变」上，覆盖写入会偷偷换掉回测引用的数据）；
//! - 实验记录只增不改（登记簿不是工作区）；
//! - 持久化是**快照写**（整表序列化覆盖 KV 键），适合登记量级（条目小、
//!   频次低）；巨量目录归 P3 包级实现（parquet dataset 面）。

use std::collections::BTreeMap;
use std::sync::Mutex;

use alpha_core::{AlphaError, AlphaResult};
use alpha_protocols::dataset::{DatasetDescriptor, ExperimentRecord};

use crate::StorageBackend;

/// datasets 快照 KV 键
pub const DATASETS_SNAPSHOT_KEY: &str = "alpha/registry/datasets";
/// experiments 快照 KV 键
pub const EXPERIMENTS_SNAPSHOT_KEY: &str = "alpha/registry/experiments";

/// 内存登记表（Mutex 粒度 = 表级；登记是低频管理面操作，锁竞争不构成问题）
#[derive(Default)]
pub struct DatasetRegistry {
    datasets: Mutex<BTreeMap<String, DatasetDescriptor>>,
    experiments: Mutex<BTreeMap<String, ExperimentRecord>>,
}

fn dataset_key(dataset_id: &str, version: &str) -> String {
    format!("{dataset_id}@{version}")
}

impl DatasetRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 登记数据集：同 id+version 重复注册报冲突（不可变引用语义）
    pub fn register_dataset(&self, descriptor: DatasetDescriptor) -> AlphaResult<()> {
        if descriptor.dataset_id.trim().is_empty() || descriptor.version.trim().is_empty() {
            return Err(AlphaError::InvalidInput(
                "dataset_id 与 version 均不得为空（引用定位的两半）".to_string(),
            ));
        }
        let mut datasets = self
            .datasets
            .lock()
            .map_err(|_| AlphaError::StorageError("dataset registry mutex poisoned".into()))?;
        let key = dataset_key(&descriptor.dataset_id, &descriptor.version);
        if datasets.contains_key(&key) {
            return Err(AlphaError::InvalidInput(format!(
                "数据集已登记，版本内容视为不可变，不接受覆盖：{key}"
            )));
        }
        datasets.insert(key, descriptor);
        Ok(())
    }

    pub fn get_dataset(&self, dataset_id: &str, version: &str) -> Option<DatasetDescriptor> {
        self.datasets
            .lock()
            .ok()?
            .get(&dataset_key(dataset_id, version))
            .cloned()
    }

    /// 全量数据集（按 `dataset_id@version` 字典序；登记表不承诺时序序）
    pub fn list_datasets(&self) -> Vec<DatasetDescriptor> {
        match self.datasets.lock() {
            Ok(map) => map.values().cloned().collect(),
            Err(_) => Vec::new(),
        }
    }

    /// 登记实验记录：同 experiment_id 重复登记报冲突（登记簿只增不改）
    pub fn record_experiment(&self, record: ExperimentRecord) -> AlphaResult<()> {
        if record.experiment_id.trim().is_empty() {
            return Err(AlphaError::InvalidInput(
                "experiment_id 不得为空".to_string(),
            ));
        }
        let mut experiments = self
            .experiments
            .lock()
            .map_err(|_| AlphaError::StorageError("dataset registry mutex poisoned".into()))?;
        if experiments.contains_key(&record.experiment_id) {
            return Err(AlphaError::InvalidInput(format!(
                "实验记录已存在（登记簿只增不改）：{}",
                record.experiment_id
            )));
        }
        experiments.insert(record.experiment_id.clone(), record);
        Ok(())
    }

    pub fn get_experiment(&self, experiment_id: &str) -> Option<ExperimentRecord> {
        self.experiments.lock().ok()?.get(experiment_id).cloned()
    }

    /// 某数据集版本下的全部实验（复现面：给一版数据，列出所有跑过的实验）
    pub fn experiments_for_dataset(
        &self,
        dataset_id: &str,
        version: &str,
    ) -> Vec<ExperimentRecord> {
        match self.experiments.lock() {
            Ok(map) => map
                .values()
                .filter(|r| r.dataset_id == dataset_id && r.dataset_version == version)
                .cloned()
                .collect(),
            Err(_) => Vec::new(),
        }
    }

    /// 快照持久化：两键整表覆盖写（登记量级小、频次低；调用方持有 backend）
    pub async fn persist(&self, backend: &dyn StorageBackend) -> AlphaResult<()> {
        let (datasets, experiments) =
            {
                let datasets = self.datasets.lock().map_err(|_| {
                    AlphaError::StorageError("dataset registry mutex poisoned".into())
                })?;
                let experiments = self.experiments.lock().map_err(|_| {
                    AlphaError::StorageError("dataset registry mutex poisoned".into())
                })?;
                (
                    serde_json::to_vec(&*datasets)?,
                    serde_json::to_vec(&*experiments)?,
                )
            };
        backend.store(DATASETS_SNAPSHOT_KEY, datasets).await?;
        backend.store(EXPERIMENTS_SNAPSHOT_KEY, experiments).await
    }

    /// 从快照装载（键缺失 = 空表，不报错——首次部署零初始化）
    pub async fn load(&mut self, backend: &dyn StorageBackend) -> AlphaResult<()> {
        let mut loaded_datasets = BTreeMap::new();
        let mut loaded_experiments = BTreeMap::new();
        if let Some(bytes) = backend.retrieve(DATASETS_SNAPSHOT_KEY).await? {
            loaded_datasets = serde_json::from_slice(&bytes)?;
        }
        if let Some(bytes) = backend.retrieve(EXPERIMENTS_SNAPSHOT_KEY).await? {
            loaded_experiments = serde_json::from_slice(&bytes)?;
        }
        *self
            .datasets
            .lock()
            .map_err(|_| AlphaError::StorageError("dataset registry mutex poisoned".into()))? =
            loaded_datasets;
        *self
            .experiments
            .lock()
            .map_err(|_| AlphaError::StorageError("dataset registry mutex poisoned".into()))? =
            loaded_experiments;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryStorage;

    fn dataset(id: &str, version: &str) -> DatasetDescriptor {
        DatasetDescriptor {
            dataset_id: id.into(),
            version: version.into(),
            ..Default::default()
        }
    }

    fn experiment(id: &str, dataset_id: &str, version: &str) -> ExperimentRecord {
        ExperimentRecord {
            experiment_id: id.into(),
            dataset_id: dataset_id.into(),
            dataset_version: version.into(),
            ..Default::default()
        }
    }

    /// 登记与读取：id+version 组合定位；空 id / 空 version 拒绝
    #[tokio::test]
    async fn register_and_get_dataset() {
        let registry = DatasetRegistry::new();
        registry.register_dataset(dataset("kline", "v1")).unwrap();
        assert!(registry.get_dataset("kline", "v1").is_some());
        assert!(registry.get_dataset("kline", "v2").is_none());
        assert!(registry.get_dataset("quotes", "v1").is_none());

        assert!(registry.register_dataset(dataset("", "v1")).is_err());
        assert!(registry.register_dataset(dataset("kline", "")).is_err());
    }

    /// 不可变语义：同 id+version 重复注册报冲突；不同版本并存
    #[tokio::test]
    async fn duplicate_dataset_registration_conflicts() {
        let registry = DatasetRegistry::new();
        registry.register_dataset(dataset("kline", "v1")).unwrap();
        let err = registry
            .register_dataset(dataset("kline", "v1"))
            .unwrap_err();
        assert!(err.to_string().contains("不可变"), "{err}");
        // 不同版本不冲突
        registry.register_dataset(dataset("kline", "v2")).unwrap();
        assert_eq!(registry.list_datasets().len(), 2);
    }

    /// 实验登记只增不改 + 按数据集版本过滤（复现面）
    #[tokio::test]
    async fn experiments_record_only_and_filter_by_dataset() {
        let registry = DatasetRegistry::new();
        registry
            .record_experiment(experiment("exp-1", "kline", "v1"))
            .unwrap();
        registry
            .record_experiment(experiment("exp-2", "kline", "v2"))
            .unwrap();
        registry
            .record_experiment(experiment("exp-3", "quotes", "v1"))
            .unwrap();

        let matched = registry.experiments_for_dataset("kline", "v1");
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].experiment_id, "exp-1");

        // 重复登记冲突
        let err = registry
            .record_experiment(experiment("exp-1", "kline", "v1"))
            .unwrap_err();
        assert!(err.to_string().contains("只增不改"), "{err}");
        // 空 experiment_id 拒绝
        assert!(registry
            .record_experiment(experiment("", "kline", "v1"))
            .is_err());
    }

    /// persist/load 快照回环：经 MemoryStorage 两键落取，空表 load 不报错
    #[tokio::test]
    async fn persist_load_roundtrip_via_storage_backend() {
        let backend = MemoryStorage::new();
        let registry = DatasetRegistry::new();
        registry.register_dataset(dataset("kline", "v1")).unwrap();
        registry
            .record_experiment(experiment("exp-1", "kline", "v1"))
            .unwrap();
        registry.persist(&backend).await.unwrap();

        // 独立实例装回
        let mut restored = DatasetRegistry::new();
        assert!(restored.list_datasets().is_empty());
        restored.load(&backend).await.unwrap();
        assert!(restored.get_dataset("kline", "v1").is_some());
        assert_eq!(restored.experiments_for_dataset("kline", "v1").len(), 1);

        // 空后端 load：键缺失 = 空表，不报错
        let mut fresh = DatasetRegistry::new();
        fresh.load(&MemoryStorage::new()).await.unwrap();
        assert!(fresh.list_datasets().is_empty());
    }
}
