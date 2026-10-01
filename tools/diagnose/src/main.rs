//! 智能故障诊断 CLI（L464）：Prometheus 指标快照 → 规则推理 → 根因建议报告。
//!
//! 数据流：Prometheus HTTP API 即时查询（`/api/v1/query`）→ [`Snapshot`] →
//! [`alpha_core::diagnosis::diagnose`] 纯函数推理 → 文本/JSON 报告。
//!
//! 退出码（供 cron/CI 分级处置）：
//! - `0` 健康（无发现）；`1` 有 WARNING；`2` 有 CRITICAL；`3` 数据源不可达
//!   （**连不上 Prometheus ≠ 健康**——单独码避免把监控故障判成业务正常）
//!
//! 指标缺失（查询空结果）对应规则跳过：缺数据 ≠ 故障（引擎层语义）。

use std::collections::HashMap;
use std::process::ExitCode;

use alpha_core::diagnosis::{diagnose, Diagnosis, ServiceMetrics, Snapshot};
use clap::Parser;

#[derive(Parser)]
#[command(
    name = "alpha-diagnose",
    about = "对 Prometheus 指标快照做规则推理，输出根因建议报告（L464）"
)]
struct Args {
    /// Prometheus HTTP API 地址
    #[arg(
        long,
        env = "ALPHA_PROMETHEUS_URL",
        default_value = "http://127.0.0.1:9090"
    )]
    prometheus_url: String,

    /// 输出 JSON（供自动化消费）；默认人类可读文本
    #[arg(long)]
    json: bool,
}

/// Prometheus 即时查询响应（`status/data.result` 向量样本）
#[derive(Debug, serde::Deserialize)]
struct QueryResponse {
    status: String,
    #[serde(default)]
    data: QueryData,
}

#[derive(Debug, Default, serde::Deserialize)]
struct QueryData {
    #[serde(default)]
    result: Vec<QuerySample>,
}

#[derive(Debug, serde::Deserialize)]
struct QuerySample {
    #[serde(default)]
    metric: HashMap<String, String>,
    /// `[timestamp, "value"]`（向量样本的当前值）
    value: Option<(f64, String)>,
}

/// 解析样本值：`"1"`→`1.0`；`NaN`/`+Inf`/空串 → None（比较语义下不触发规则）
fn parse_sample(raw: &str) -> Option<f64> {
    let v: f64 = raw.trim().parse().ok()?;
    v.is_finite().then_some(v)
}

/// 执行即时查询。连接失败 → `Err`（数据源不可达，整体退出码 3）；
/// PromQL/响应错误 → `Ok(vec![])`（单指标缺数据，规则跳过）。
async fn query_vector(
    client: &reqwest::Client,
    prometheus_url: &str,
    promql: &str,
) -> Result<Vec<(HashMap<String, String>, f64)>, String> {
    let url = format!("{}/api/v1/query", prometheus_url.trim_end_matches('/'));
    let resp = client
        .get(&url)
        .query(&[("query", promql)])
        .send()
        .await
        .map_err(|e| format!("Prometheus 不可达（{url}）: {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        eprintln!("warn: 查询 HTTP {status}: {promql}");
        return Ok(Vec::new());
    }
    let body: QueryResponse = match resp.json().await {
        Ok(b) => b,
        Err(e) => {
            eprintln!("warn: 响应解析失败: {e}");
            return Ok(Vec::new());
        }
    };
    if body.status != "success" {
        eprintln!("warn: PromQL 执行失败: {promql}");
        return Ok(Vec::new());
    }
    Ok(body
        .data
        .result
        .into_iter()
        .filter_map(|s| {
            let (_, raw) = s.value?;
            Some((s.metric, parse_sample(&raw)?))
        })
        .collect())
}

/// 取向量里第一个样本值（无标签聚合查询的典型形态）
fn first_value(samples: &[(HashMap<String, String>, f64)]) -> Option<f64> {
    samples.first().map(|(_, v)| *v)
}

/// 按标签取样本值
fn value_by_label(samples: &[(HashMap<String, String>, f64)], key: &str, val: &str) -> Option<f64> {
    samples
        .iter()
        .find(|(m, _)| m.get(key).map(String::as_str) == Some(val))
        .map(|(_, v)| *v)
}

/// 采集 Prometheus 快照：7 条查询拼装各服务指标（查询错误逐条降级为空）
async fn collect_snapshot(
    client: &reqwest::Client,
    prometheus_url: &str,
) -> Result<Snapshot, String> {
    let up = query_vector(
        client,
        prometheus_url,
        r#"up{job=~"api-gateway|data-engine|real-time-feed|collector"}"#,
    )
    .await?; // 连接失败在此上抛（exit 3）
    let error_rate = query_vector(
        client,
        prometheus_url,
        r#"sum(rate(alpha_gateway_requests_total{status=~"5.."}[5m])) / sum(rate(alpha_gateway_requests_total[5m]))"#,
    ).await.unwrap_or_else(|e| { eprintln!("warn: {e}"); Vec::new() });
    let p95_ms = query_vector(
        client,
        prometheus_url,
        r#"histogram_quantile(0.95, sum by (le) (rate(alpha_dataengine_query_duration_seconds_bucket[5m]))) * 1000"#,
    ).await.unwrap_or_else(|_| Vec::new());
    let mem_ratio = query_vector(
        client,
        prometheus_url,
        r#"alpha_dataengine_memory_bytes{mode="used"} / alpha_dataengine_memory_bytes{mode="total"}"#,
    ).await.unwrap_or_else(|_| Vec::new());
    let unreleased = query_vector(
        client,
        prometheus_url,
        "alpha_alloc_tracker_total_unreleased",
    )
    .await
    .unwrap_or_else(|_| Vec::new());
    let feed_connected = query_vector(client, prometheus_url, "alpha_realtime_feed_connected")
        .await
        .unwrap_or_else(|_| Vec::new());
    let msg_rate = query_vector(
        client,
        prometheus_url,
        "rate(alpha_realtime_messages_total[1m])",
    )
    .await
    .unwrap_or_else(|_| Vec::new());

    let mut services = Vec::new();
    for job in ["api-gateway", "data-engine", "real-time-feed", "collector"] {
        let mut sm = ServiceMetrics::new(job);
        if let Some(v) = value_by_label(&up, "job", job) {
            sm.up = Some(v >= 1.0);
        }
        services.push(sm);
    }
    // 指标 → 服务归属（下标即服务位，与上面循环顺序一致）
    if let Some(v) = first_value(&error_rate) {
        services[0].error_rate = Some(v);
    }
    if let Some(v) = first_value(&p95_ms) {
        services[1].p95_latency_ms = Some(v);
    }
    if let Some(v) = first_value(&mem_ratio) {
        services[1].memory_used_ratio = Some(v);
    }
    if let Some(v) = first_value(&unreleased) {
        // 未释放分配是进程级信号，挂网关位展示（任一进程暴露即可见）
        services[0].unreleased_allocs = Some(v as u64);
    }
    if let Some(v) = first_value(&feed_connected) {
        services[2].feed_connected = Some(v >= 1.0);
    }
    if let Some(v) = first_value(&msg_rate) {
        services[2].message_rate = Some(v);
    }

    Ok(Snapshot { services })
}

/// 文本报告：级别 + 逐条发现（证据/根因/建议）
fn render_text(d: &Diagnosis) -> String {
    if d.is_healthy() {
        return "诊断: 健康——全部规则通过（指标可用范围内无异常）".to_string();
    }
    let mut out = format!("诊断: {}（{} 条发现）\n", d.level(), d.findings.len());
    for (i, f) in d.findings.iter().enumerate() {
        out.push_str(&format!(
            "{:>2}. [{}] {} · {}\n    证据: {}\n    建议: {}\n",
            i + 1,
            f.severity,
            f.service,
            f.rule,
            f.evidence,
            f.suggestion
        ));
        if let Some(cause) = &f.root_cause_of {
            out.push_str(&format!("    根因关联: {cause}\n"));
        }
    }
    out
}

fn exit_code(d: &Diagnosis) -> u8 {
    if d.has_critical() {
        2
    } else if d.is_healthy() {
        0
    } else {
        1
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = Args::parse();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .expect("reqwest client");

    let snapshot = match collect_snapshot(&client, &args.prometheus_url).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(3);
        }
    };

    let diagnosis = diagnose(&snapshot);
    if args.json {
        match serde_json::to_string_pretty(&diagnosis) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("error: JSON 序列化失败: {e}");
                return ExitCode::from(3);
            }
        }
    } else {
        print!("{}", render_text(&diagnosis));
    }
    ExitCode::from(exit_code(&diagnosis))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sample_handles_nan_and_inf_as_missing() {
        assert_eq!(parse_sample("1"), Some(1.0));
        assert_eq!(parse_sample("0.0512"), Some(0.0512));
        assert_eq!(parse_sample("NaN"), None, "NaN 不应参与规则比较");
        assert_eq!(parse_sample("+Inf"), None);
        assert_eq!(parse_sample(""), None);
        assert_eq!(parse_sample("not-a-number"), None);
    }

    #[test]
    fn query_response_deserializes_vector_samples() {
        let raw = r#"{
            "status": "success",
            "data": {
                "resultType": "vector",
                "result": [
                    {"metric": {"job": "data-engine"}, "value": [1790000000.1, "0"]},
                    {"metric": {"job": "api-gateway"}, "value": [1790000000.1, "1"]}
                ]
            }
        }"#;
        let parsed: QueryResponse = serde_json::from_str(raw).unwrap();
        assert_eq!(parsed.status, "success");
        assert_eq!(parsed.data.result.len(), 2);
        let samples: Vec<(HashMap<String, String>, f64)> = parsed
            .data
            .result
            .into_iter()
            .filter_map(|s| {
                let (_, raw) = s.value?;
                Some((s.metric, parse_sample(&raw)?))
            })
            .collect();
        assert_eq!(value_by_label(&samples, "job", "data-engine"), Some(0.0));
        assert_eq!(value_by_label(&samples, "job", "api-gateway"), Some(1.0));
        assert_eq!(value_by_label(&samples, "job", "missing"), None);
    }

    #[test]
    fn missing_metric_queries_yield_none_not_false_alarms() {
        // 空向量（指标未暴露/查询无数据）→ 全 None 快照 → 零发现
        let snap = Snapshot {
            services: vec![ServiceMetrics::new("api-gateway")],
        };
        let d = diagnose(&snap);
        assert!(d.is_healthy());
        assert_eq!(exit_code(&d), 0);
    }

    #[test]
    fn exit_codes_reflect_severity_levels() {
        let healthy = Diagnosis::default();
        assert_eq!(exit_code(&healthy), 0);

        let warn = diagnose(&Snapshot {
            services: vec![ServiceMetrics {
                error_rate: Some(0.2),
                ..ServiceMetrics::new("api-gateway")
            }],
        });
        assert_eq!(exit_code(&warn), 1);

        let crit = diagnose(&Snapshot {
            services: vec![ServiceMetrics {
                up: Some(false),
                ..ServiceMetrics::new("data-engine")
            }],
        });
        assert_eq!(exit_code(&crit), 2);
    }

    #[test]
    fn render_text_contains_evidence_and_suggestion() {
        let d = diagnose(&Snapshot {
            services: vec![ServiceMetrics {
                up: Some(false),
                ..ServiceMetrics::new("data-engine")
            }],
        });
        let text = render_text(&d);
        assert!(text.contains("CRITICAL"), "{text}");
        assert!(text.contains("data-engine"));
        assert!(text.contains("证据:"));
        assert!(text.contains("docker logs"));

        let healthy_text = render_text(&Diagnosis::default());
        assert!(healthy_text.contains("健康"));
    }

    #[test]
    fn json_mode_serializes_findings() {
        let d = diagnose(&Snapshot {
            services: vec![ServiceMetrics {
                up: Some(false),
                ..ServiceMetrics::new("real-time-feed")
            }],
        });
        let json = serde_json::to_string(&d).unwrap();
        assert!(json.contains("\"rule\":\"ServiceDown\""), "{json}");
        assert!(json.contains("\"severity\":\"Critical\""), "{json}");
        assert!(json.contains("real-time-feed"), "{json}");
    }
}
