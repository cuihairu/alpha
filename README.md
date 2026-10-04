# alpha

Internal A-share (China) market data platform focused on low-latency ingestion, cleaning, and distribution of freely available public data. Positioning: **self-hosted financial data & quant research foundation**（采集 → 标准化 → 存储 → 查询 → 实时分发 → 指标/分析 → 回测）; scope boundaries in `docs/architecture-review.md`. The current implementation is Rust-first, with a staged queue strategy that starts from Redis Streams and can evolve to NATS JetStream or Kafka as scale increases.  
中文方案说明见 `docs/architecture.md`。

## Goals
- Collect equities data (行情、公告、财报、新闻、舆情) from public/free sources with per-source rate limit control.
- Normalize, enrich, and store both time-series (quotes, indicators) and document-style data (announcements, news).
- Offer consistent APIs/WebSocket streams to internal tools and potential downstream quant pipelines.
- Run entirely inside a self-hosted LAN environment; external publishing (e.g. Cloudflare Tunnel) is planned, not yet configured.

## High-Level Architecture
1. **Data Collectors**  
   - Rust collector (`services/collector`) is the default: YAML/JSON task templates + cron scheduler + per-source rate limiter + retry with jittered backoff.  
   - `crawlers/python/` holds a standard-library reference script (eastmoney quote fetch); multi-language execution (Python/Node/Go/Rust/Shell) is supported by the template runner.  
   - Proxy pool component exists; fingerprint rotation and schedule-level jitter are not implemented.

2. **Message Queue Layer**  
   - Phase 1 uses Redis Streams as the low-cost default for collector-to-consumer decoupling.  
   - Phase 2 can move to NATS JetStream when multi-consumer persistence and replay become mandatory.  
   - Kafka remains the high-throughput option only after data domains and audit/replay requirements justify the cost.  
   - Streams currently in use: `quotes.raw`, `quotes.normalized`, `quotes.dlq`. The announcements/news domains are not implemented yet.

3. **Processing Services**  
   - Rust services consume stream messages, handle validation, schema mapping, and enrichment (价格标准化、查询态指标).  
   - Writes to storage targets:
     - TimescaleDB/PostgreSQL for tick/daily bars (optional mirror, off by default).
     - ClickHouse for market-data Parquet export/archival (no write path yet).
     - Redis for hot caches, deduplication locks, and rate-limit tokens.
   - Batch recomputation currently lives inside data-engine (MemTable refresh before `/query`); no separate ETL jobs.

4. **Public/Private APIs**  
   - `services/api-gateway` exposes REST (`/api/v1/*` proxy) + WebSocket (`/ws`) + health/metrics.  
   - gRPC is served directly by data-engine (`:50051`), not by the gateway.  
   - Auth (all off by default): data-engine X-Api-Key gate, gateway JWT (HS256 self-signed / OIDC) + RBAC, Redis rate limiting.  
   - Web UI is a quote/analysis dashboard; monitoring dashboards live in Grafana.

5. **Deployment & Networking**  
   - Rust binaries built statically and run via docker-compose or systemd (`scripts/deploy-ubuntu.sh`).  
   - Internal network hosts the entire pipeline; publishing only the API gateway through Cloudflare Tunnel is a planned option.  
   - CI/CD via GitHub Actions (lint/test/wasm/build/e2e/security + release pipelines).

6. **Observability & Ops**  
   - Prometheus scrapes the four services' `/metrics` endpoints.  
   - Grafana dashboard (`config/grafana/`) covers ingestion latency, queue lag, API latency; Loki collects container logs.  
   - Alertmanager + `alert-webhook` notify DingTalk/WeChat Work/Slack/PagerDuty on alert rules (`config/alpha-alerts.yml`, 9 rules).

## Technology Decisions
- **Core Language**: Rust for processing services, schedulers, and APIs.
- **Crawlers**: Rust collector by default; `crawlers/python` for ad-hoc reference scripts.
- **Queue**: Redis Streams by default, with planned upgrade paths to NATS JetStream and Kafka.
- **Databases**: TimescaleDB/PostgreSQL + ClickHouse (export only) + Redis (cache/rate-limit). The S3/MinIO storage backend exists in `packages/storage`; raw-dump archival on top of it is not wired yet.
- **Schema Definition**: serde JSON (Envelope v2, additive evolution) for queue/REST; Protobuf only for the gRPC surface.

## Roadmap
1. Define Protobuf schemas and topic contracts.
2. Keep the Redis Streams pipeline minimal and reliable before introducing heavier MQ infrastructure.
3. ✅ Build crawler framework with scheduling + proxy rotation — done via collector task templates (YAML/JSON source definitions) + cron dispatcher + optional proxy pool.
4. Establish observability stack and tunnel configuration.
5. Expand dataset coverage (沪深两市, 港股延伸, alternative data).
