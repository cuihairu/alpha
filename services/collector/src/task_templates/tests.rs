use super::*;
use crate::main_simple::SimpleCollector;
use crate::types::{
    AShareDataSource, NewsDataSource, ParserType, StorageType, TaskPriority, TaskSource,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const VALID_YAML: &str = r#"
version: 1
tasks:
  - id: ashare-kline
    name: A股日K线
    source_type: ashare
    url: "https://push2his.eastmoney.com/api/qt/stock/kline/get"
    symbols: ["600519", "000001"]
    params:
      klt: "101"
      fqt: "1"
    schedule: "0 30 9 * * 1-5"
    priority: high
    timeout_secs: 30
    max_retries: 5
    request_interval_ms: 2000
    tags: [kline, daily]
    parser:
      type: json
      format: json
      field_mapping:
        close: close
      rules:
        - name: price
          regex: '"close":([0-9.]+)'
    storage:
      type: database
      target: timescale
      table: kline_daily
      batch_size: 500
  - id: sina-news
    name: 新浪要闻
    source_type: news
    url: "https://feed.mix.sina.com.cn/api/roll/get"
    schedule: "0 */10 * * * *"
"#;

/// 与 VALID_YAML 首任务等价的 JSON 形态（跨格式等价性锚）
const EQUIV_JSON_TASK: &str = r#"{
  "version": 1,
  "tasks": [{
    "id": "ashare-kline",
    "name": "A股日K线",
    "source_type": "ashare",
    "url": "https://push2his.eastmoney.com/api/qt/stock/kline/get",
    "symbols": ["600519", "000001"],
    "params": {"klt": "101", "fqt": "1"},
    "schedule": "0 30 9 * * 1-5",
    "priority": "high",
    "timeout_secs": 30,
    "max_retries": 5,
    "request_interval_ms": 2000,
    "tags": ["kline", "daily"],
    "parser": {"type": "json", "format": "json", "field_mapping": {"close": "close"},
               "rules": [{"name": "price", "regex": "\"close\":([0-9.]+)"}]},
    "storage": {"type": "database", "target": "timescale", "table": "kline_daily", "batch_size": 500}
  }]
}"#;

fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "alpha-collector-templates-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&dir).expect("创建临时目录");
    dir
}

fn write(path: &Path, content: &str) {
    std::fs::write(path, content).expect("写模板文件");
}

fn a_share_source() -> TaskSource {
    TaskSource::AShare {
        source: AShareDataSource::EastMoney,
        symbols: vec!["000001".to_string()],
    }
}

#[test]
fn loads_yaml_file_with_full_fields() {
    let dir = temp_dir();
    let file = dir.join("tasks.yaml");
    write(&file, VALID_YAML);
    let templates = load_path(&file).expect("装载成功");
    assert_eq!(templates.len(), 2);

    let first = &templates[0];
    assert_eq!(first.id, "ashare-kline");
    assert_eq!(
        first.symbols,
        Some(vec!["600519".to_string(), "000001".to_string()])
    );
    assert_eq!(first.params.get("klt").map(String::as_str), Some("101"));
    assert_eq!(first.schedule.as_deref(), Some("0 30 9 * * 1-5"));
    assert_eq!(first.priority.as_deref(), Some("high"));
    assert_eq!(first.storage.target.as_deref(), Some("timescale"));
    assert_eq!(first.storage.batch_size, Some(500));
    assert_eq!(first.parser.parser_type.as_deref(), Some("json"));
    assert!(first.enabled);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn json_and_yaml_produce_equivalent_tasks() {
    let source = a_share_source();
    let yaml_task = load_path_str(VALID_YAML, "t.yaml")
        .expect("yaml 装载")
        .remove(0)
        .into_task_definition(source.clone())
        .expect("yaml 转换");
    let json_task = load_path_str(EQUIV_JSON_TASK, "t.json")
        .expect("json 装载")
        .remove(0)
        .into_task_definition(source)
        .expect("json 转换");

    assert_eq!(yaml_task.id, json_task.id);
    assert_eq!(yaml_task.name, json_task.name);
    assert_eq!(yaml_task.config.request.url, json_task.config.request.url);
    assert_eq!(
        yaml_task.config.request.params,
        json_task.config.request.params
    );
    assert_eq!(
        yaml_task.config.request.request_interval,
        json_task.config.request.request_interval
    );
    assert_eq!(yaml_task.config.parser, json_task.config.parser);
    assert_eq!(yaml_task.config.storage, json_task.config.storage);
    assert_eq!(yaml_task.priority, json_task.priority);
    assert_eq!(yaml_task.schedule, json_task.schedule);
    assert_eq!(yaml_task.source, json_task.source);
}

/// 直接从字符串内容装载（落盘后走同一条 load_path 路径）
fn load_path_str(content: &str, file_name: &str) -> Result<Vec<TaskTemplate>, TemplateError> {
    let dir = temp_dir();
    let file = dir.join(file_name);
    write(&file, content);
    let result = load_path(&file);
    std::fs::remove_dir_all(&dir).ok();
    result
}

#[test]
fn rejects_unknown_source_type() {
    let err = load_path_str(
        "tasks:\n  - id: a\n    name: a\n    source_type: martian\n    url: https://x.io\n",
        "bad.yaml",
    )
    .expect_err("应拒绝未知 source_type");
    assert!(
        err.to_string().contains("source_type"),
        "报错应指明字段：{err}"
    );
}

#[test]
fn rejects_duplicate_ids_within_file() {
    let content = VALID_YAML.replace("sina-news", "ashare-kline");
    let err = load_path_str(&content, "dup.yaml").expect_err("应拒绝文件内重复 ID");
    assert!(
        matches!(err, TemplateError::DuplicateId { .. }),
        "实际：{err}"
    );
}

#[test]
fn rejects_duplicate_ids_across_files() {
    let dir = temp_dir();
    write(&dir.join("a.yaml"), VALID_YAML);
    write(
        &dir.join("b.json"),
        r#"{"tasks": [{"id": "sina-news", "name": "x", "source_type": "news",
                 "url": "https://x.io"}]}"#,
    );
    let err = load_path(&dir).expect_err("应拒绝跨文件重复 ID");
    match err {
        TemplateError::DuplicateId { ids } => {
            assert!(ids.contains("sina-news"));
            assert!(ids.contains("a.yaml"));
            assert!(ids.contains("b.json"));
        }
        other => panic!("应为 DuplicateId，实际 {other:?}"),
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn rejects_invalid_url() {
    for url in ["ftp://x.io", "not-a-url", "https://"] {
        let content = format!(
            "tasks:\n  - id: a\n    name: a\n    source_type: custom\n    url: \"{url}\"\n"
        );
        let err = load_path_str(&content, "u.yaml").expect_err("应拒绝非法 url");
        assert!(
            err.to_string().contains("url"),
            "url={url} 的报错应指明字段：{err}"
        );
    }
}

#[test]
fn rejects_invalid_priority_parser_and_storage() {
    let base = "tasks:\n  - id: a\n    name: a\n    source_type: custom\n    url: https://x.io";
    for extra in [
        "\n    priority: urgent",
        "\n    parser:\n      type: yaml",
        "\n    storage:\n      type: tape",
        "\n    storage:\n      type: database",
        "\n    storage:\n      compression: zip",
        "\n    request_interval_ms: 0",
        "\n    timeout_secs: 0",
        "\n    symbols: []",
    ] {
        let Err(err) = load_path_str(&format!("{base}{extra}\n"), "v.yaml") else {
            panic!("应拒绝 {extra:?}")
        };
        assert!(!err.to_string().is_empty());
    }
}

#[test]
fn schedule_takes_five_or_six_cron_fields() {
    for ok in ["0 9 * * 1-5", "0 */10 * * * *", "30 8-16/2 * * 1-5"] {
        let content = format!(
                "tasks:\n  - id: a\n    name: a\n    source_type: custom\n    url: https://x.io\n    schedule: \"{ok}\"\n"
            );
        let Ok(loaded) = load_path_str(&content, "s.yaml") else {
            panic!("合法 schedule {ok} 应装载")
        };
        assert_eq!(loaded.len(), 1);
    }
    for bad in ["0 9 *", "0 9 * * * * *", "0 9 * * * ;", "每五分钟"] {
        let content = format!(
                "tasks:\n  - id: a\n    name: a\n    source_type: custom\n    url: https://x.io\n    schedule: \"{bad}\"\n"
            );
        let Err(err) = load_path_str(&content, "s.yaml") else {
            panic!("非法 schedule {bad} 应被拒")
        };
        assert!(err.to_string().contains("schedule"), "{bad} 报错：{err}");
    }
}

#[test]
fn rejects_unknown_top_or_task_field() {
    let err = load_path_str(
            "version: 1\nshedule: x\ntasks:\n  - id: a\n    name: a\n    source_type: custom\n    url: https://x.io\n",
            "f.yaml",
        )
        .expect_err("未知顶层字段应被拒");
    assert!(err.to_string().contains("unknown field"), "实际：{err}");

    let err = load_path_str(
            "tasks:\n  - id: a\n    name: a\n    source_type: custom\n    url: https://x.io\n    shedule: x\n",
            "f.yaml",
        )
        .expect_err("未知任务字段应被拒");
    assert!(err.to_string().contains("unknown field"), "实际：{err}");
}

#[test]
fn rejects_invalid_id_name_and_version() {
    for bad_id in ["UpperCase", "-leading", "带中文", ""] {
        let content = format!(
                "tasks:\n  - id: \"{bad_id}\"\n    name: a\n    source_type: custom\n    url: https://x.io\n"
            );
        assert!(
            load_path_str(&content, "i.yaml").is_err(),
            "id={bad_id:?} 应被拒"
        );
    }
    assert!(load_path_str(
        "tasks:\n  - id: a\n    name: \" \"\n    source_type: custom\n    url: https://x.io\n",
        "n.yaml"
    )
    .is_err());
    assert!(load_path_str(
            "version: 2\ntasks:\n  - id: a\n    name: a\n    source_type: custom\n    url: https://x.io\n",
            "v.yaml"
        )
        .is_err());
}

#[test]
fn empty_and_missing_paths_error() {
    assert!(load_path_str("tasks: []\n", "e.yaml").is_err());
    assert!(matches!(
        load_path(Path::new("/nonexistent/alpha-templates.yaml")),
        Err(TemplateError::Io { .. })
    ));
    assert!(load_path_str("", "empty.yaml").is_err());
    assert!(matches!(
        load_path_str("tasks:\n  - id: a\n", "x.toml"),
        Err(TemplateError::Parse { .. })
    ));
}

#[test]
fn directory_loading_is_sorted_and_ext_filtered() {
    let dir = temp_dir();
    write(&dir.join("b-second.yaml"), VALID_YAML);
    write(
        &dir.join("a-first.json"),
        r#"{"tasks": [{"id": "aa-first", "name": "x", "source_type": "custom",
                 "url": "https://x.io"}]}"#,
    );
    write(&dir.join("notes.txt"), "不是模板");
    let templates = load_path(&dir).expect("目录装载");
    // txt 不入装载：仅 2 个模板文件、共 3 个任务（b-second.yaml 含 2 个）
    assert_eq!(templates.len(), 3);
    assert_eq!(templates[0].id, "aa-first", "按文件名序：a-first.json 在前");
    assert_eq!(templates[1].id, "ashare-kline");
    assert_eq!(templates[2].id, "sina-news", "文件内保持声明顺序");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn minimal_task_maps_to_api_path_defaults() {
    let templates = load_path_str(
            "tasks:\n  - id: bare\n    name: 最小任务\n    source_type: custom\n    url: https://x.io/api\n",
            "m.yaml",
        )
        .expect("最小模板装载");
    let task = templates[0]
        .clone()
        .into_task_definition(TaskSource::Custom {
            source_type: "custom".to_string(),
            endpoint: "https://x.io/api".to_string(),
            params: HashMap::new(),
        })
        .expect("转换");
    assert_eq!(task.config.request.method, "GET");
    assert_eq!(task.config.request.request_interval, 1000);
    assert_eq!(task.config.parser.parser_type, ParserType::JSON);
    assert_eq!(task.config.storage.storage_type, StorageType::Memory);
    assert_eq!(
        task.config.storage.target, "default",
        "memory 缺省目标与 API 路径一致"
    );
    assert_eq!(task.retry_policy.max_retries, 3);
    assert_eq!(task.priority, TaskPriority::Medium);
    assert_eq!(task.status, crate::types::TaskStatus::Pending);
}

#[test]
fn symbols_replace_default_and_reject_unsupported_source() {
    let templates = load_path_str(VALID_YAML, "s.yaml").expect("装载");
    let task = templates[0]
        .clone()
        .into_task_definition(a_share_source())
        .expect("ashare 支持 symbols");
    match task.source {
        TaskSource::AShare { symbols, .. } => {
            assert_eq!(symbols, vec!["600519".to_string(), "000001".to_string()]);
        }
        other => panic!("source 应保持 AShare，实际 {other:?}"),
    }

    let news = templates[1].clone();
    let mut news_with_symbols = news;
    news_with_symbols.symbols = Some(vec!["600519".to_string()]);
    let news_source = TaskSource::News {
        sources: vec![NewsDataSource::Sina],
        keywords: vec![],
        languages: vec![],
    };
    assert!(news_with_symbols.into_task_definition(news_source).is_err());
}

#[test]
fn parse_rules_default_required_to_false() {
    let templates = load_path_str(VALID_YAML, "r.yaml").expect("装载");
    let task = templates[0]
        .clone()
        .into_task_definition(a_share_source())
        .expect("转换");
    assert_eq!(task.config.parser.rules.len(), 1);
    assert_eq!(task.config.parser.rules[0].name, "price");
    assert!(!task.config.parser.rules[0].required, "required 缺省 false");
}

#[tokio::test]
async fn submit_registers_enabled_and_skips_disabled() {
    let workspace = temp_dir();
    let templates_dir = workspace.join("templates");
    std::fs::create_dir_all(&templates_dir).expect("建模板目录");
    write(
        &templates_dir.join("tasks.yaml"),
        &VALID_YAML.replace("  - id: sina-news", "  - id: sina-news\n    enabled: false"),
    );

    let collector = SimpleCollector::new(&workspace);
    let mut events = collector.subscribe_events();
    let (submitted, skipped) = collector
        .submit_task_templates(&templates_dir)
        .await
        .expect("提交模板");
    assert_eq!((submitted, skipped), (1, 1));

    let stats = collector.get_task_stats().await;
    assert_eq!(stats.total, 1, "disabled 任务不入登记");
    assert_eq!(stats.running, 0);

    // 事件面：仅登记的任务发 TaskSubmitted
    match events.try_recv() {
        Ok(crate::main_simple::CollectorEvent::TaskSubmitted { task_id, .. }) => {
            assert_eq!(task_id, "ashare-kline");
        }
        other => panic!("应收到 TaskSubmitted，实际 {other:?}"),
    }
    assert!(events.try_recv().is_err(), "disabled 任务不发事件");
    std::fs::remove_dir_all(&workspace).ok();
}

#[tokio::test]
async fn submit_surfaces_template_errors_as_string() {
    let workspace = temp_dir();
    let file = workspace.join("tasks.yaml");
    write(
        &file,
        "tasks:\n  - id: a\n    name: a\n    source_type: martian\n    url: https://x.io\n",
    );
    let collector = SimpleCollector::new(&workspace);
    let err = collector
        .submit_task_templates(&file)
        .await
        .expect_err("未知 source_type 应报错");
    assert!(err.contains("source_type"), "报错信息：{err}");
    std::fs::remove_dir_all(&workspace).ok();
}

/// 优先级大小写不敏感（HIGH == high）
#[test]
fn priority_matching_is_case_insensitive() {
    let templates = load_path_str(
            "tasks:\n  - id: a\n    name: a\n    source_type: custom\n    url: https://x.io\n    priority: HIGH\n",
            "p.yaml",
        )
        .expect("装载");
    let task = templates[0]
        .clone()
        .into_task_definition(TaskSource::Custom {
            source_type: "custom".to_string(),
            endpoint: "https://x.io".to_string(),
            params: HashMap::new(),
        })
        .expect("转换");
    assert_eq!(task.priority, TaskPriority::High);
}

/// 兜底：validate 侧 HashSet/HashMap 路径无 panic（全量字段合法场景回归）
#[test]
fn validate_covers_all_branches_without_panic() {
    let templates = load_path_str(VALID_YAML, "c.yaml").expect("装载");
    for t in &templates {
        assert!(t.validate().is_ok());
    }
}
