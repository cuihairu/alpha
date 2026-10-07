//! /query 自定义聚合函数（`market_data` 表查询面）
//!
//! data-engine 启动时把三个聚合 UDAF 注册进 DataFusion 会话
//! （main.rs `register_custom_functions` 调 [`register`]），SQL 面
//! （POST `/query`、网关 `/api/v1/*` 反代）即可直接调用：
//!
//! - `vwap(price, volume)`：成交量加权均价 Σ(p·v)/Σ(v)
//! - `range_pct(price)`：区间振幅 (max−min)/min×100（百分数口径）
//! - `avg_spread(bid, ask)`：平均买卖价差 mean(ask−bid)
//!
//! 三者均为聚合（分组内行序不影响结果），NULL 行跳过、无有效输入
//! 返回 NULL（JSON 面为 null）；`range_pct` 遇 min≤0 视为非法价格域
//! 返回 NULL 而非发出负值/除零伪结果。签名精确匹配 market_data 列
//! 类型（price/bid/ask Float64、volume UInt64，后三列可空）。
//!
//! 本模块只建函数与累加器（纯逻辑，单测直喂 ArrayRef）；经
//! execute_query 的集成面由 main.rs 测试覆盖。

use std::sync::Arc;

use datafusion::{
    arrow::{
        array::{Array, ArrayRef, Float64Array, Int64Array, UInt64Array},
        datatypes::DataType,
    },
    common::{DataFusionError, Result, ScalarValue},
    logical_expr::{create_udaf, Accumulator, AggregateUDF, Volatility},
    prelude::SessionContext,
};

/// 按下标取 Float64 列（UDAF 精确签名下恒成立，错型/缺参回内部错）
fn f64_col<'a>(values: &'a [ArrayRef], idx: usize, fn_name: &str) -> Result<&'a Float64Array> {
    arg(values, idx, fn_name)?
        .as_any()
        .downcast_ref::<Float64Array>()
        .ok_or_else(|| DataFusionError::Internal(format!("{fn_name}: arg {idx} 不是 Float64")))
}

/// 按下标取 UInt64 列（vwap 的 volume 实参）
fn u64_col<'a>(values: &'a [ArrayRef], idx: usize, fn_name: &str) -> Result<&'a UInt64Array> {
    arg(values, idx, fn_name)?
        .as_any()
        .downcast_ref::<UInt64Array>()
        .ok_or_else(|| DataFusionError::Internal(format!("{fn_name}: arg {idx} 不是 UInt64")))
}

/// 按下标取 Int64 列（部分聚合的 count 中间态）
fn i64_col<'a>(values: &'a [ArrayRef], idx: usize, fn_name: &str) -> Result<&'a Int64Array> {
    arg(values, idx, fn_name)?
        .as_any()
        .downcast_ref::<Int64Array>()
        .ok_or_else(|| DataFusionError::Internal(format!("{fn_name}: arg {idx} 不是 Int64")))
}

fn arg<'a>(values: &'a [ArrayRef], idx: usize, fn_name: &str) -> Result<&'a ArrayRef> {
    values
        .get(idx)
        .ok_or_else(|| DataFusionError::Internal(format!("{fn_name}: 缺第 {idx} 个实参")))
}

/// `vwap(price, volume)`：Σ(p·v)/Σ(v)。
/// price 或 volume 为 NULL 的行不参与（如 volume 缺失的历史点）；
/// Σv=0（全零量/全 NULL 量）→ NULL，不做除零。
#[derive(Debug, Default, Clone)]
struct Vwap {
    sum_pv: f64,
    sum_v: f64,
}

impl Accumulator for Vwap {
    fn update_batch(&mut self, values: &[ArrayRef]) -> Result<()> {
        let prices = f64_col(values, 0, "vwap")?;
        let volumes = u64_col(values, 1, "vwap")?;
        for i in 0..prices.len() {
            if prices.is_null(i) || volumes.is_null(i) {
                continue;
            }
            let v = volumes.value(i) as f64;
            self.sum_pv += prices.value(i) * v;
            self.sum_v += v;
        }
        Ok(())
    }

    fn evaluate(&self) -> Result<ScalarValue> {
        Ok(ScalarValue::Float64(
            (self.sum_v > 0.0).then(|| self.sum_pv / self.sum_v),
        ))
    }

    fn size(&self) -> usize {
        std::mem::size_of_val(self)
    }

    fn state(&self) -> Result<Vec<ScalarValue>> {
        Ok(vec![
            ScalarValue::Float64(Some(self.sum_pv)),
            ScalarValue::Float64(Some(self.sum_v)),
        ])
    }

    fn merge_batch(&mut self, states: &[ArrayRef]) -> Result<()> {
        let partial_pv = f64_col(states, 0, "vwap merge")?;
        let partial_v = f64_col(states, 1, "vwap merge")?;
        for i in 0..partial_pv.len() {
            if partial_pv.is_null(i) || partial_v.is_null(i) {
                continue;
            }
            self.sum_pv += partial_pv.value(i);
            self.sum_v += partial_v.value(i);
        }
        Ok(())
    }
}

/// `range_pct(price)`：区间振幅 (max−min)/min×100。
/// 单行组 → 0.0；无有效行 → NULL；min≤0 → NULL（非法价格域）。
#[derive(Debug, Default, Clone)]
struct RangePct {
    min: Option<f64>,
    max: Option<f64>,
}

impl RangePct {
    fn observe(&mut self, price: f64) {
        self.min = Some(match self.min {
            Some(m) => m.min(price),
            None => price,
        });
        self.max = Some(match self.max {
            Some(m) => m.max(price),
            None => price,
        });
    }
}

impl Accumulator for RangePct {
    fn update_batch(&mut self, values: &[ArrayRef]) -> Result<()> {
        let prices = f64_col(values, 0, "range_pct")?;
        for i in 0..prices.len() {
            if prices.is_null(i) {
                continue;
            }
            self.observe(prices.value(i));
        }
        Ok(())
    }

    fn evaluate(&self) -> Result<ScalarValue> {
        Ok(ScalarValue::Float64(match (self.min, self.max) {
            (Some(min), Some(max)) if min > 0.0 && min.is_finite() => {
                Some((max - min) / min * 100.0)
            }
            _ => None,
        }))
    }

    fn size(&self) -> usize {
        std::mem::size_of_val(self)
    }

    fn state(&self) -> Result<Vec<ScalarValue>> {
        Ok(vec![
            ScalarValue::Float64(self.min),
            ScalarValue::Float64(self.max),
        ])
    }

    fn merge_batch(&mut self, states: &[ArrayRef]) -> Result<()> {
        let partial_min = f64_col(states, 0, "range_pct merge")?;
        let partial_max = f64_col(states, 1, "range_pct merge")?;
        for i in 0..partial_min.len() {
            // 空的部分组状态为 NULL，跳过；非空则并入全局 min/max
            if partial_min.is_null(i) || partial_max.is_null(i) {
                continue;
            }
            self.observe(partial_min.value(i));
            self.observe(partial_max.value(i));
        }
        Ok(())
    }
}

/// `avg_spread(bid, ask)`：mean(ask−bid)。
/// bid/ask 任一为 NULL 的行跳过（点无盘口报价）；无有效行 → NULL。
#[derive(Debug, Default, Clone)]
struct AvgSpread {
    sum: f64,
    count: i64,
}

impl Accumulator for AvgSpread {
    fn update_batch(&mut self, values: &[ArrayRef]) -> Result<()> {
        let bids = f64_col(values, 0, "avg_spread")?;
        let asks = f64_col(values, 1, "avg_spread")?;
        for i in 0..bids.len() {
            if bids.is_null(i) || asks.is_null(i) {
                continue;
            }
            self.sum += asks.value(i) - bids.value(i);
            self.count += 1;
        }
        Ok(())
    }

    fn evaluate(&self) -> Result<ScalarValue> {
        Ok(ScalarValue::Float64(
            (self.count > 0).then(|| self.sum / self.count as f64),
        ))
    }

    fn size(&self) -> usize {
        std::mem::size_of_val(self)
    }

    fn state(&self) -> Result<Vec<ScalarValue>> {
        Ok(vec![
            ScalarValue::Float64(Some(self.sum)),
            ScalarValue::Int64(Some(self.count)),
        ])
    }

    fn merge_batch(&mut self, states: &[ArrayRef]) -> Result<()> {
        let partial_sum = f64_col(states, 0, "avg_spread merge")?;
        let partial_count = i64_col(states, 1, "avg_spread merge")?;
        for i in 0..partial_sum.len() {
            if partial_sum.is_null(i) || partial_count.is_null(i) {
                continue;
            }
            self.sum += partial_sum.value(i);
            self.count += partial_count.value(i);
        }
        Ok(())
    }
}

/// 注册三个 UDAF 到会话（幂等：DataFusion 注册表按名覆盖同名函数）
pub fn register(session: &SessionContext) {
    session.register_udaf(vwap_udaf());
    session.register_udaf(range_pct_udaf());
    session.register_udaf(avg_spread_udaf());
}

fn vwap_udaf() -> AggregateUDF {
    create_udaf(
        "vwap",
        vec![DataType::Float64, DataType::UInt64],
        Arc::new(DataType::Float64),
        Volatility::Immutable,
        Arc::new(|_| Ok(Box::new(Vwap::default()) as Box<dyn Accumulator>)),
        Arc::new(vec![DataType::Float64, DataType::Float64]),
    )
}

fn range_pct_udaf() -> AggregateUDF {
    create_udaf(
        "range_pct",
        vec![DataType::Float64],
        Arc::new(DataType::Float64),
        Volatility::Immutable,
        Arc::new(|_| Ok(Box::new(RangePct::default()) as Box<dyn Accumulator>)),
        Arc::new(vec![DataType::Float64, DataType::Float64]),
    )
}

fn avg_spread_udaf() -> AggregateUDF {
    create_udaf(
        "avg_spread",
        vec![DataType::Float64, DataType::Float64],
        Arc::new(DataType::Float64),
        Volatility::Immutable,
        Arc::new(|_| Ok(Box::new(AvgSpread::default()) as Box<dyn Accumulator>)),
        Arc::new(vec![DataType::Float64, DataType::Int64]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f64s(vals: Vec<Option<f64>>) -> ArrayRef {
        Arc::new(Float64Array::from(vals))
    }

    fn u64s(vals: Vec<Option<u64>>) -> ArrayRef {
        Arc::new(UInt64Array::from(vals))
    }

    fn evaluate(acc: &dyn Accumulator) -> Option<f64> {
        match acc.evaluate().unwrap() {
            ScalarValue::Float64(v) => v,
            other => panic!("期望 Float64，得到 {other:?}"),
        }
    }

    #[test]
    fn vwap_weights_by_volume_and_skips_null_rows() {
        let mut acc = Vwap::default();
        acc.update_batch(&[
            f64s(vec![Some(100.0), Some(102.0), Some(999.0)]),
            u64s(vec![Some(10), Some(20), None]), // 第三行 volume 缺失 → 跳过
        ])
        .unwrap();
        // (100×10 + 102×20) / 30 = 3040/30
        assert!((evaluate(&acc).unwrap() - 3040.0 / 30.0).abs() < 1e-9);
    }

    #[test]
    fn vwap_zero_volume_group_is_null() {
        let mut acc = Vwap::default();
        acc.update_batch(&[f64s(vec![Some(100.0)]), u64s(vec![Some(0)])])
            .unwrap();
        assert_eq!(evaluate(&acc), None);
    }

    #[test]
    fn range_pct_reports_percent_amplitude() {
        let mut acc = RangePct::default();
        acc.update_batch(&[f64s(vec![Some(100.0), Some(95.0), Some(110.0)])])
            .unwrap();
        // (110−95)/95×100
        assert!((evaluate(&acc).unwrap() - 15.0 / 95.0 * 100.0).abs() < 1e-9);
    }

    #[test]
    fn range_pct_single_row_is_zero_and_invalid_min_is_null() {
        let mut acc = RangePct::default();
        acc.update_batch(&[f64s(vec![Some(100.0)])]).unwrap();
        assert_eq!(evaluate(&acc), Some(0.0));

        let mut bad = RangePct::default();
        bad.update_batch(&[f64s(vec![Some(0.0), Some(-1.0)])])
            .unwrap();
        assert_eq!(evaluate(&bad), None);
    }

    #[test]
    fn avg_spread_averages_only_complete_pairs() {
        let mut acc = AvgSpread::default();
        acc.update_batch(&[
            f64s(vec![Some(99.5), None, Some(101.5)]),
            f64s(vec![Some(100.5), Some(999.0), Some(102.5)]),
        ])
        .unwrap();
        // 两对完整报价：1.0 与 1.0 → 均值 1.0（中间行 bid 缺失跳过）
        assert!((evaluate(&acc).unwrap() - 1.0).abs() < 1e-9);

        let mut empty = AvgSpread::default();
        empty
            .update_batch(&[f64s(vec![None]), f64s(vec![Some(1.0)])])
            .unwrap();
        assert_eq!(evaluate(&empty), None);
    }

    #[test]
    fn accumulators_merge_two_phase_state() {
        // vwap：两段部分组状态 [sum_pv, sum_v] 合并后与单段一致
        let mut a = Vwap::default();
        a.update_batch(&[f64s(vec![Some(100.0)]), u64s(vec![Some(10)])])
            .unwrap();
        let mut b = Vwap::default();
        b.update_batch(&[f64s(vec![Some(102.0)]), u64s(vec![Some(20)])])
            .unwrap();
        let st_a = a.state().unwrap();
        let st_b = b.state().unwrap();
        let cols: Vec<ArrayRef> = (0..st_a.len())
            .map(|i| {
                let va = st_a[i].to_array_of_size(1).unwrap();
                let vb = st_b[i].to_array_of_size(1).unwrap();
                datafusion::arrow::compute::concat(&[&va, &vb]).unwrap()
            })
            .collect();
        let mut merged = Vwap::default();
        merged.merge_batch(&cols).unwrap();
        assert!((evaluate(&merged).unwrap() - 3040.0 / 30.0).abs() < 1e-9);
    }

    #[tokio::test]
    async fn registered_udafs_resolve_in_sql() {
        use datafusion::{
            arrow::{
                datatypes::{Field, Schema},
                record_batch::RecordBatch,
            },
            datasource::MemTable,
        };

        let session = SessionContext::new();
        register(&session);

        // 与 market_data 同构的最小列型（price Float64 / volume UInt64），
        // 验证精确签名在真实列型上可解析并算出值
        let schema = Arc::new(Schema::new(vec![
            Field::new("price", DataType::Float64, false),
            Field::new("volume", DataType::UInt64, true),
        ]));
        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(Float64Array::from(vec![100.0, 102.0])),
                Arc::new(UInt64Array::from(vec![Some(10u64), Some(20u64)])),
            ],
        )
        .unwrap();
        let table = Arc::new(MemTable::try_new(schema, vec![vec![batch]]).unwrap());
        session.register_table("t", table).unwrap();

        let out = session
            .sql("SELECT vwap(price, volume) AS v, range_pct(price) AS r FROM t")
            .await
            .unwrap()
            .collect()
            .await
            .unwrap();
        assert_eq!(out[0].num_rows(), 1);
        let v = out[0]
            .column(0)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        assert!((v.value(0) - 3040.0 / 30.0).abs() < 1e-9);
    }
}
