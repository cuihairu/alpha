//! 数据质量：sequence 断档告警（architecture-review §3.1 sequence + §5 P2）
//!
//! `EventEnvelope.sequence`（v2 契约字段，per-source 单调序列号）在数据面观察：
//! 跳号即断档告警——采集端丢消息、写队列失败、生产进程异常都无法「跳过」序号。
//!
//! 观察语义（每 (stream, source) 一条基线）：
//! - **FirstSeen**：首见记基线，不告警（上游早已开跑，无法区分启动首跳与断档）
//! - **Continuous**：`got == last + 1`，更新基线
//! - **Gap**：`got > last + 1`，缺失 `got - last - 1` 条 → 告警面由调用方打
//! - **Regression**：`got <= last`，上游重启（序号归零）或重放 → 重置基线，
//!   按 info 记而不是断档告警：重启归零是采集端已知形态；重放由去重窗口兜底。
//!
//! 本模块只做判定（纯状态机，可单测）；告警输出（tracing/metrics）由调用方
//! 在 process_normalizer_message 处组装，与判定解耦。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// 一次断档的详情
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SequenceGap {
    /// 期望到达的下一序号（last + 1）
    pub expected: u64,
    /// 实际到达序号
    pub got: u64,
    /// 缺失条数（got - expected）
    pub missing: u64,
}

/// 观测结果（None 分支在调用方按各自语义处理）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SequenceObservation {
    /// 该 (stream, source) 基线首次建立
    FirstSeen,
    /// 严格连续（got == last + 1）
    Continuous,
    /// 跳号断档
    Gap(SequenceGap),
    /// 序号回退（got <= last；典型：生产端重启归零或历史重放）
    Regression { last: u64, got: u64 },
}

/// per-(stream, source) 序列基线；last=None 表示「尚未建立基线」
/// 用 Option 而非 0 哨兵——真实序列号若为 0 不能被误判为首见。
#[derive(Debug, Default)]
struct SequenceBaseline {
    last: Option<u64>,
}

/// 线程安全的数据面序列观察器
#[derive(Clone, Default)]
pub struct SequenceGapMonitor {
    baselines: Arc<Mutex<HashMap<(String, String), SequenceBaseline>>>,
}

impl SequenceGapMonitor {
    /// 首见基线时是否告警由调用方决定（预置 false：默认不惊动，见模块文档）
    pub fn new() -> Self {
        Self::default()
    }

    /// 观察一条消息的序列号；返回判定结果。
    pub fn observe(&self, stream: &str, source: &str, sequence: u64) -> SequenceObservation {
        let mut baselines = self
            .baselines
            .lock()
            .expect("sequence baseline mutex poisoned");
        let baseline = baselines
            .entry((stream.to_string(), source.to_string()))
            .or_default();

        let Some(last) = baseline.last else {
            baseline.last = Some(sequence);
            return SequenceObservation::FirstSeen;
        };

        if sequence == last + 1 {
            baseline.last = Some(sequence);
            return SequenceObservation::Continuous;
        }

        if sequence > last + 1 {
            let gap = SequenceGap {
                expected: last + 1,
                got: sequence,
                missing: sequence - last - 1,
            };
            baseline.last = Some(sequence);
            return SequenceObservation::Gap(gap);
        }

        // 回退：基线重置（生产端重启归零或历史重放）
        baseline.last = Some(sequence);
        SequenceObservation::Regression {
            last,
            got: sequence,
        }
    }

    /// 当前基线数（测试断言用；非 test 构建不参与）
    #[cfg(test)]
    pub fn baseline_count(&self) -> usize {
        self.baselines
            .lock()
            .expect("sequence baseline mutex poisoned")
            .len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m() -> SequenceGapMonitor {
        SequenceGapMonitor::new()
    }

    #[test]
    fn first_seen_sets_baseline_without_gap() {
        let monitor = m();
        assert_eq!(
            monitor.observe("quotes.raw", "eastmoney", 1),
            SequenceObservation::FirstSeen
        );
        assert_eq!(
            monitor.observe("quotes.raw", "eastmoney", 2),
            SequenceObservation::Continuous
        );
    }

    #[test]
    fn gap_detects_missing_sequence_count() {
        let monitor = m();
        assert_eq!(
            monitor.observe("quotes.raw", "eastmoney", 10),
            SequenceObservation::FirstSeen
        );
        // 一次跳 3 条：11、12、13 缺失
        assert_eq!(
            monitor.observe("quotes.raw", "eastmoney", 14),
            SequenceObservation::Gap(SequenceGap {
                expected: 11,
                got: 14,
                missing: 3,
            })
        );
        // 断档后续恢复连续
        assert_eq!(
            monitor.observe("quotes.raw", "eastmoney", 15),
            SequenceObservation::Continuous
        );
    }

    #[test]
    fn regression_resets_baseline_without_gap_alert() {
        let monitor = m();
        assert_eq!(
            monitor.observe("quotes.raw", "eastmoney", 100),
            SequenceObservation::FirstSeen
        );
        // 生产端重启归零：从 1 重新开始 → 回退重基线，不按断档告警
        assert_eq!(
            monitor.observe("quotes.raw", "eastmoney", 1),
            SequenceObservation::Regression { last: 100, got: 1 }
        );
        // 新基线建立后连续
        assert_eq!(
            monitor.observe("quotes.raw", "eastmoney", 2),
            SequenceObservation::Continuous
        );
    }

    #[test]
    fn baselines_are_per_stream_and_per_source() {
        let monitor = m();
        assert_eq!(
            monitor.observe("quotes.raw", "eastmoney", 5),
            SequenceObservation::FirstSeen
        );
        // 同 stream 不同 source 独立
        assert_eq!(
            monitor.observe("quotes.raw", "sina", 1),
            SequenceObservation::FirstSeen
        );
        // 同 source 不同 stream 独立（normalized 面未来接入时）
        assert_eq!(
            monitor.observe("quotes.normalized", "eastmoney", 1),
            SequenceObservation::FirstSeen
        );
        assert_eq!(monitor.baseline_count(), 3);
        // 不串扰：eastmoney/raw 后续序列不受其他基线影响
        assert_eq!(
            monitor.observe("quotes.raw", "eastmoney", 6),
            SequenceObservation::Continuous
        );
    }

    #[test]
    fn zero_or_repeated_sequence_is_regression() {
        let monitor = m();
        assert_eq!(
            monitor.observe("quotes.raw", "eastmoney", 3),
            SequenceObservation::FirstSeen
        );
        // 完全重复的序列号（同上一条）：回退处理（重放），不误报断档
        assert_eq!(
            monitor.observe("quotes.raw", "eastmoney", 3),
            SequenceObservation::Regression { last: 3, got: 3 }
        );
    }
}
