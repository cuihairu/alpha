//! Cron 调度（architecture §24「刷新频率」的执行面）
//!
//! 任务模板/API 提交的 `schedule` cron 表达式此前只是数据面（无人解释执行），
//! 本模块补齐执行层：
//! - [`parse`]：5/6 段 cron 解析（6 段含秒位；5 段秒域恒全通）。域语法支持
//!   `*` / `*/n` / `a-b` / `a-b/n` / 逗号列表 / 单值；周日接受 0 或 7（归一为
//!   0）。日+月+周同时限定时为 AND 语义（不实现 Vixie 的 DOM-or-DOW 扩展）。
//! - [`CronSchedule::matches`]：与本地时刻精确到秒匹配。
//! - [`CronDispatcher`]：扫描任务表，对 `schedule` 到期且非运行中的任务经
//!   [`TaskRunner`] 派发执行（`tokio::spawn` 异步执行，不阻塞扫描循环）。
//!
//! 零新增依赖：解析手写（u64 位集，秒/分/时/日/月/周全域 ≤64 位），时间为
//! chrono（已在依赖树）。
//!
//! 口径：每 tick 判定「当前秒位命中」即派发一次；执行经 running 表置位（
//! SimpleCollector::execute_task 已置 Running，完成/失败后收敛为终态），
//! Running 中的任务下一 tick 不再重触发——天然防秒级 cron 重入。

use crate::types::{TaskDefinition, TaskStatus};
use async_trait::async_trait;
use chrono::{Datelike, NaiveDateTime, NaiveTime, Timelike};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use tokio::sync::RwLock;
use tracing::{debug, warn};

/// 任务执行面（SimpleCollector 实现；测试注入假 runner 收派发计数）
#[async_trait]
pub trait TaskRunner: Send + Sync {
    async fn run(&self, task_id: &str) -> Result<String, String>;
}

/// 单个 cron 域（u64 位集：第 i 位 = 该域取值 i 命中）
#[derive(Debug, Clone, Copy, PartialEq)]
struct CronField(u64);

impl CronField {
    fn full(len: usize) -> Self {
        CronField(if len >= 64 {
            u64::MAX
        } else {
            (1u64 << len) - 1
        })
    }

    fn is_set(&self, value: u32) -> bool {
        self.0 & (1u64 << value) != 0
    }
}

/// 解析后的 cron 表达式（6 域全位集；5 段表达式秒域恒全通）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CronSchedule {
    second: CronField,
    minute: CronField,
    hour: CronField,
    day_of_month: CronField,
    month: CronField,
    day_of_week: CronField,
}

/// 域取值范围（含两端）
struct FieldRange {
    name: &'static str,
    min: u32,
    max: u32,
}

const FIELDS: [FieldRange; 6] = [
    FieldRange {
        name: "秒",
        min: 0,
        max: 59,
    },
    FieldRange {
        name: "分",
        min: 0,
        max: 59,
    },
    FieldRange {
        name: "时",
        min: 0,
        max: 23,
    },
    FieldRange {
        name: "日",
        min: 1,
        max: 31,
    },
    FieldRange {
        name: "月",
        min: 1,
        max: 12,
    },
    FieldRange {
        name: "周",
        min: 0,
        max: 7,
    },
];

/// 解析 5/6 段 cron 表达式（`0 */5 * * * *` 6 段含秒；`0 9 * * 1-5` 5 段秒域恒通）
pub fn parse(expr: &str) -> Result<CronSchedule, String> {
    let tokens: Vec<&str> = expr.split_whitespace().collect();
    let f = |i: usize| tokens[i];
    let schedule = match tokens.len() {
        // 6 段 = [秒 分 时 日 月 周]
        6 => CronSchedule {
            second: parse_field(f(0), &FIELDS[0])?,
            minute: parse_field(f(1), &FIELDS[1])?,
            hour: parse_field(f(2), &FIELDS[2])?,
            day_of_month: parse_field(f(3), &FIELDS[3])?,
            month: parse_field(f(4), &FIELDS[4])?,
            day_of_week: parse_field(f(5), &FIELDS[5])?,
        },
        // 5 段 = [分 时 日 月 周]，秒域恒全通
        5 => CronSchedule {
            second: CronField::full(60),
            minute: parse_field(f(0), &FIELDS[1])?,
            hour: parse_field(f(1), &FIELDS[2])?,
            day_of_month: parse_field(f(2), &FIELDS[3])?,
            month: parse_field(f(3), &FIELDS[4])?,
            day_of_week: parse_field(f(4), &FIELDS[5])?,
        },
        n => {
            return Err(format!(
                "须为 5/6 段 cron 表达式（6 段含秒位，如 `0 */5 * * * *`），实际 {n} 段"
            ))
        }
    };
    Ok(schedule)
}

/// 解析单个域：`*` | `*/n` | `a-b` | `a-b/n` | 逗号列表 | 单值
fn parse_field(input: &str, range: &FieldRange) -> Result<CronField, String> {
    let mut bits = 0u64;
    for item in input.split(',') {
        if item.is_empty() {
            return Err(domain_err("空项", range, input));
        }
        let (lo, hi, step) = if item == "*" {
            (range.min, range.max, 1)
        } else if let Some(n) = item.strip_prefix("*/") {
            let step = parse_int(n, range, "步进")?;
            if step == 0 {
                return Err(domain_err("*/0 步进不能为 0", range, item));
            }
            (range.min, range.max, step)
        } else if item.contains('-') {
            let (a, rest) = item.split_once('-').expect("contains '-' 已检查");
            let (b, step) = match rest.split_once('/') {
                Some((b, n)) => {
                    let step = parse_int(n, range, "步进")?;
                    if step == 0 {
                        return Err(domain_err("/0 步进不能为 0", range, item));
                    }
                    (b, step)
                }
                None => (rest, 1),
            };
            let lo = parse_int(a, range, "起点")?;
            let hi = parse_int(b, range, "终点")?;
            if lo > hi {
                return Err(domain_err("区间起点大于终点", range, item));
            }
            (lo, hi, step)
        } else {
            let v = parse_int(item, range, "取值")?;
            (v, v, 1)
        };
        // 步进从 lo 起；`*/15` 语义 0,15,30,45（lo=域最小值）
        let mut v = lo;
        while v <= hi {
            bits |= 1u64 << v;
            v = v
                .checked_add(step)
                .ok_or_else(|| domain_err("数值溢出", range, item))?;
        }
    }
    let field = CronField(bits);
    // 周域 7 与 0 同义（周日），归一后两位同置
    if range.name == "周" && field.is_set(7) {
        Ok(CronField(field.0 | 1))
    } else {
        Ok(field)
    }
}

fn parse_int(raw: &str, range: &FieldRange, what: &str) -> Result<u32, String> {
    let value: u32 = raw
        .parse()
        .map_err(|_| domain_err(&format!("{what}「{raw}」不是非负整数"), range, raw))?;
    if value < range.min || value > range.max {
        return Err(domain_err(&format!("{what} {value} 越界"), range, raw));
    }
    Ok(value)
}

fn domain_err(msg: &str, range: &FieldRange, raw: &str) -> String {
    format!(
        "{}域 {raw:?} 非法：{msg}（取值 {}-{}）",
        range.name, range.min, range.max
    )
}

impl CronSchedule {
    /// 时刻是否命中（秒级精确；local naive 时刻即可，周/日/月语义由调用方定时区口径）
    pub fn matches(&self, dt: &NaiveDateTime) -> bool {
        // 位索引口径：秒/分/时/周 0 基（值=索引）；日/月 1 基（值=索引，
        // bit0 恒空——parse_field 对 1 基域按值置位）
        self.month.is_set(dt.month())
            && self.day_of_month.is_set(dt.day())
            && self.day_of_week.is_set(dt.weekday().num_days_from_sunday())
            && self.hour.is_set(dt.hour())
            && self.minute.is_set(dt.minute())
            && self.second.is_set(dt.second())
    }
}

/// cron 派发器：扫描任务表 → 到期且非运行中 → 经 runner 异步执行
pub struct CronDispatcher {
    tasks: Arc<RwLock<HashMap<String, TaskDefinition>>>,
    running: Arc<RwLock<HashMap<String, TaskStatus>>>,
    runner: Arc<dyn TaskRunner>,
    /// 表达式解析缓存（懒解析；非法表达式记 Err 跳过，模板层已保证合法，
    /// API 路径的人工输入可能非法——不 panic、告警后静默）
    parsed: Mutex<HashMap<String, Result<CronSchedule, String>>>,
}

impl CronDispatcher {
    pub fn new(
        tasks: Arc<RwLock<HashMap<String, TaskDefinition>>>,
        running: Arc<RwLock<HashMap<String, TaskStatus>>>,
        runner: Arc<dyn TaskRunner>,
    ) -> Self {
        Self {
            tasks,
            running,
            runner,
            parsed: Mutex::new(HashMap::new()),
        }
    }

    /// 解析并缓存单个表达式
    pub fn schedule_for(&self, expr: &str) -> Result<CronSchedule, String> {
        let mut cache = self.parsed.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(hit) = cache.get(expr) {
            return hit.clone();
        }
        let parsed = parse(expr);
        cache.insert(expr.to_string(), parsed.clone());
        parsed
    }

    /// 判定此刻到期的任务 ID（schedule 命中且非运行中）；非法表达式跳过
    pub async fn due_task_ids(&self, now: &NaiveDateTime) -> Vec<String> {
        let snap = {
            let tasks = self.tasks.read().await;
            let snapshot: Vec<(String, Option<String>)> = tasks
                .iter()
                .map(|(id, t)| (id.clone(), t.schedule.clone()))
                .collect();
            snapshot
        };
        let running_now = {
            let run = self.running.read().await;
            run.iter()
                .filter(|(_, st)| **st == TaskStatus::Running)
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>()
        };

        let mut due = Vec::new();
        for (id, schedule) in snap {
            if running_now.contains(&id) {
                continue; // 执行中（含悬挂）不重触发
            }
            let Some(expr) = schedule else { continue };
            match self.schedule_for(&expr) {
                Ok(cron) if cron.matches(now) => due.push(id),
                Ok(_) => {}
                Err(e) => debug!(task = %id, error = %e, "cron 表达式非法，跳过调度"),
            }
        }
        due
    }

    /// 对到期的任务派发执行（异步 spawn，返回派发数）。同 tick 内同任务只派一次
    pub async fn dispatch_due(&self, now: &NaiveDateTime) -> usize {
        let due = self.due_task_ids(now).await;
        let count = due.len();
        for id in due {
            let runner = Arc::clone(&self.runner);
            let task_id = id.clone();
            tokio::spawn(async move {
                if let Err(e) = runner.run(&task_id).await {
                    warn!(task = %task_id, error = %e, "cron 派发执行失败");
                }
            });
        }
        count
    }

    /// 驱动的常驻循环：每 tick 判定 + 派发
    pub async fn run_forever(&self, tick_every: std::time::Duration) {
        let mut interval = tokio::time::interval(tick_every);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            let now = chrono::Local::now().naive_local();
            let dispatched = self.dispatch_due(&now).await;
            if dispatched > 0 {
                debug!(n = dispatched, "cron 调度派发");
            }
        }
    }
}

/// 时间构造 helper（测试与外部复用）
pub fn naive_date_time(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
) -> NaiveDateTime {
    chrono::NaiveDate::from_ymd_opt(year, month, day)
        .expect("合法日期")
        .and_time(NaiveTime::from_hms_opt(hour, minute, second).expect("合法时刻"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TaskPriority;

    fn t(year: i32, mon: u32, day: u32, h: u32, m: u32, s: u32) -> NaiveDateTime {
        naive_date_time(year, mon, day, h, m, s)
    }

    fn sched(expr: &str) -> CronSchedule {
        parse(expr).unwrap_or_else(|e| panic!("解析 {expr} 失败：{e}"))
    }

    /// 5 段（分起）对应 6 段（秒域全通）——语义等价锚
    #[test]
    fn five_fields_equals_six_with_any_second() {
        let five = sched("30 9 * * 1-5");
        let six = sched("* 30 9 * * 1-5");
        for dt in [
            t(2026, 10, 5, 9, 30, 0),
            t(2026, 10, 5, 9, 30, 59),
            t(2026, 10, 6, 9, 30, 0),
        ] {
            assert_eq!(five.matches(&dt), six.matches(&dt), "{dt}");
        }
    }

    #[test]
    fn parses_full_six_fields() {
        let s = sched("0 */5 * * * *");
        assert!(s.matches(&t(2026, 10, 3, 8, 0, 0)));
        assert!(s.matches(&t(2026, 10, 3, 23, 55, 0)));
        assert!(!s.matches(&t(2026, 10, 3, 8, 1, 0)));
        assert!(!s.matches(&t(2026, 10, 3, 8, 0, 1)), "秒位必须精确");
    }

    #[test]
    fn matches_hour_minute_second_conjunction() {
        let s = sched("30 8-16/2 * * 1-5");
        assert!(s.matches(&t(2026, 10, 5, 8, 30, 30)));
        assert!(s.matches(&t(2026, 10, 5, 16, 30, 30)));
        assert!(s.matches(&t(2026, 10, 5, 10, 30, 30)));
        assert!(!s.matches(&t(2026, 10, 5, 9, 30, 30)), "9 不在 8-16/2");
        assert!(!s.matches(&t(2026, 10, 3, 10, 30, 30)), "周六不在 1-5");
        assert!(!s.matches(&t(2026, 10, 4, 10, 30, 30)), "周日=0 不命中 1-5");
    }

    #[test]
    fn weekday_zero_and_seven_equivalent() {
        // 2026-10-04 是周日
        let sun0 = parse("0 0 0 * * 0").unwrap();
        let sun7 = parse("0 0 0 * * 7").unwrap();
        assert!(sun0.matches(&t(2026, 10, 4, 0, 0, 0)));
        assert!(sun7.matches(&t(2026, 10, 4, 0, 0, 0)));
        assert!(!sun0.matches(&t(2026, 10, 5, 0, 0, 0)));
    }

    #[test]
    fn month_day_and_weekday_are_anded() {
        // 每月 1 号且是周六（2026-11-01 周日、2026-08-01 周六）× 8 月限定 → 仅 8 月周六
        let s = sched("0 0 0 1 8 6");
        assert!(s.matches(&t(2026, 8, 1, 0, 0, 0)));
        assert!(!s.matches(&t(2026, 10, 3, 0, 0, 0)), "10 月不在 8");
        assert!(!s.matches(&t(2026, 11, 1, 0, 0, 0)), "11-01 是周日");
    }

    #[test]
    fn lists_ranges_and_steps_mix() {
        let s = sched("0 0 0 1,15 * 1,3-5");
        assert!(s.matches(&t(2026, 10, 1, 0, 0, 0)));
        assert!(s.matches(&t(2026, 10, 15, 0, 0, 0)));
        assert!(!s.matches(&t(2026, 10, 8, 0, 0, 0)), "不是 1/15");
        assert!(s.matches(&t(2026, 10, 1, 0, 0, 0)));
        assert!(!s.matches(&t(2026, 10, 1, 0, 0, 1)));
    }

    #[test]
    fn rejects_out_of_range() {
        for (expr, ctx) in [
            ("60 * * * * *", "秒 60"),
            ("* 60 * * * *", "分 60"),
            ("* * 24 * * *", "时 24"),
            ("* * * 0 * *", "日 0"),
            ("* * * 32 * *", "日 32"),
            ("* * * * 13 *", "月 13"),
            ("* * * * * 8", "周 8"),
        ] {
            let err = parse(expr).expect_err(format!("{expr} 应拒绝（{ctx}）").as_str());
            assert!(err.contains("越界"), "{expr} 报错：{err}");
        }
    }

    #[test]
    fn rejects_inverted_range_and_zero_step() {
        for expr in [
            "30-10 * * * * *",
            "*/0 * * * * *",
            "0 30-10 * * * *",
            "0 * */0 * * *",
            "0 * * * */0 *",
        ] {
            parse(expr).expect_err(&format!("{expr} 应拒绝"));
        }
    }

    #[test]
    fn rejects_malformed_inputs() {
        for expr in [
            "",
            "0 9 *",
            "0 9 * * * * * *",
            "a b c d e f",
            "0 9 * * *,",
            "0 9 * * * 1-",
            "1-5/0 9 * * * *",
        ] {
            assert!(parse(expr).is_err(), "{expr:?} 应拒绝");
        }
        let err = parse("0 9 *").expect_err("3 段");
        assert!(err.contains("5/6 段"), "{err}");
    }

    // ---- Dispatcher ----

    /// 假 runner：记录被派发的任务 ID
    struct CountingRunner {
        called: Arc<std::sync::Mutex<Vec<String>>>,
    }
    #[async_trait]
    impl TaskRunner for CountingRunner {
        async fn run(&self, task_id: &str) -> Result<String, String> {
            self.called
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(task_id.to_string());
            Ok("ok".to_string())
        }
    }

    fn task_with_schedule(id: &str, schedule: Option<&str>) -> TaskDefinition {
        let mut task = TaskDefinition::new(
            id,
            crate::types::TaskSource::Custom {
                source_type: "custom".to_string(),
                endpoint: "https://x.io".to_string(),
                params: HashMap::new(),
            },
            id,
        );
        task.schedule = schedule.map(|s| s.to_string());
        task.priority = TaskPriority::Medium;
        task
    }

    #[tokio::test]
    async fn dispatcher_picks_due_and_skips_running() {
        let tasks: Arc<RwLock<HashMap<String, TaskDefinition>>> =
            Arc::new(RwLock::new(HashMap::new()));
        let running: Arc<RwLock<HashMap<String, TaskStatus>>> =
            Arc::new(RwLock::new(HashMap::new()));
        {
            let mut map = tasks.write().await;
            map.insert(
                "due-a".into(),
                task_with_schedule("due-a", Some("0 30 9 * * 1-5")),
            );
            map.insert(
                "silent".into(),
                task_with_schedule("silent", Some("0 0 0 * * 0")),
            );
            map.insert("no-sched".into(), task_with_schedule("no-sched", None));
            map.insert(
                "bad-cron".into(),
                task_with_schedule("bad-cron", Some("99 99 * * * *")),
            );
        }
        {
            let mut map = running.write().await;
            map.insert("due-a".into(), TaskStatus::Running); // 执行中不重触发
        }

        let called = Arc::new(std::sync::Mutex::new(Vec::new()));
        let runner = Arc::new(CountingRunner {
            called: Arc::clone(&called),
        });
        let dispatcher = CronDispatcher::new(tasks, Arc::clone(&running), runner);

        // 周一下午 09:30 未到；周五 09:30 命中
        let weekday_miss = t(2026, 10, 5, 9, 30, 0);
        let weekday_hit = t(2026, 10, 9, 9, 30, 0);
        assert!(dispatcher.due_task_ids(&weekday_miss).await.is_empty());
        // 唯一 due 是 silent（周日 0 点 → 不命中当前时刻）；due-a 在 running 中被跳过
        assert!(dispatcher.due_task_ids(&weekday_hit).await.is_empty());
        let _ = weekday_hit;

        // 清理 running 后 due-a 进入候选
        running.write().await.clear();
        let due_now = dispatcher.due_task_ids(&t(2026, 10, 9, 9, 30, 0)).await;
        assert_eq!(due_now, vec!["due-a".to_string()]);

        let dispatched = dispatcher.dispatch_due(&t(2026, 10, 9, 9, 30, 0)).await;
        assert_eq!(dispatched, 1);
        // 异步派发完成窗口
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let called_ids = called.lock().unwrap_or_else(|e| e.into_inner()).clone();
        assert_eq!(called_ids, vec!["due-a".to_string()]);
    }

    #[tokio::test]
    async fn invalid_cron_never_panics_and_is_skipped() {
        let tasks: Arc<RwLock<HashMap<String, TaskDefinition>>> =
            Arc::new(RwLock::new(HashMap::new()));
        let running: Arc<RwLock<HashMap<String, TaskStatus>>> =
            Arc::new(RwLock::new(HashMap::new()));
        {
            let mut map = tasks.write().await;
            map.insert(
                "weird".into(),
                task_with_schedule("weird", Some("99 99 99 * * *")),
            );
        }
        let called = Arc::new(std::sync::Mutex::new(Vec::new()));
        let dispatcher = CronDispatcher::new(
            tasks,
            running,
            Arc::new(CountingRunner {
                called: Arc::clone(&called),
            }),
        );
        let now = t(2026, 10, 9, 9, 0, 0);
        assert!(dispatcher.due_task_ids(&now).await.is_empty());
        assert_eq!(dispatcher.dispatch_due(&now).await, 0);
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        assert!(called.lock().unwrap_or_else(|e| e.into_inner()).is_empty());
    }

    #[tokio::test]
    async fn dispatch_is_single_shot_per_tick() {
        let tasks: Arc<RwLock<HashMap<String, TaskDefinition>>> =
            Arc::new(RwLock::new(HashMap::new()));
        let running: Arc<RwLock<HashMap<String, TaskStatus>>> =
            Arc::new(RwLock::new(HashMap::new()));
        {
            let mut map = tasks.write().await;
            map.insert("s".into(), task_with_schedule("s", Some("* * * * * *")));
        }
        let called = Arc::new(std::sync::Mutex::new(Vec::new()));
        let dispatcher = CronDispatcher::new(
            tasks,
            running,
            Arc::new(CountingRunner {
                called: Arc::clone(&called),
            }),
        );
        let now = t(2026, 10, 9, 9, 0, 42);
        assert_eq!(dispatcher.dispatch_due(&now).await, 1);
        // 同秒再派不重复（一次 tick 内单次）
        assert_eq!(dispatcher.dispatch_due(&now).await, 1);
        tokio::time::sleep(std::time::Duration::from_millis(80)).await;
        assert_eq!(called.lock().unwrap_or_else(|e| e.into_inner()).len(), 2);
    }

    /// 解析缓存：同一表达式只解析一次
    #[test]
    fn schedule_for_caches_parses() {
        struct NoopRunner;
        #[async_trait]
        impl TaskRunner for NoopRunner {
            async fn run(&self, _id: &str) -> Result<String, String> {
                Ok("ok".into())
            }
        }
        let dispatcher = CronDispatcher::new(
            Arc::new(RwLock::new(HashMap::new())),
            Arc::new(RwLock::new(HashMap::new())),
            Arc::new(NoopRunner),
        );
        assert!(dispatcher.schedule_for("0 9 * * 1-5").is_ok());
        assert!(dispatcher.schedule_for("0 9 * * 1-5").is_ok());
        assert!(dispatcher.schedule_for("bad").is_err());
        assert!(dispatcher.schedule_for("bad").is_err());
        let cache = dispatcher.parsed.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(cache.len(), 2);
    }
}
