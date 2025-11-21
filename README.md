# alpha

Internal A-share (China) market data platform focused on low-latency ingestion, cleaning, and distribution of freely available public data. The platform combines lightweight Python/Node crawlers with a Go-based processing and delivery pipeline exposed over gRPC/REST and Cloudflare Tunnel.  
中文方案说明见 `docs/architecture.md`。

## Goals
- Collect equities data (行情、公告、财报、新闻、舆情) from public/free sources with per-source rate limit control.
- Normalize, enrich, and store both time-series (quotes, indicators) and document-style data (announcements, news).
- Offer consistent APIs/WebSocket streams to internal tools and potential downstream quant pipelines.
- Run entirely inside a self-hosted LAN environment while publishing selected services through Cloudflare Tunnel.

## High-Level Architecture
1. **Data Collectors**  
   - Language-flexible, but Python crawlers (requests/Playwright) are the default because of ecosystem richness.  
   - Responsible for fetching raw data, parsing, and pushing structured JSON payloads into the message queue.  
   - Include pluggable proxy pools, fingerprint rotation, and per-source schedule definitions (cron + jitter).

2. **Message Queue Layer**  
   - Kafka (preferred) or NATS JetStream separates crawling from downstream processing.  
   - Topics per data domain: `quotes`, `announcements`, `financials`, `news`, etc.  
   - Enables replay, back-pressure handling, and multi-consumer fan-out.

3. **Go Processing Services**  
   - Multiple Go consumers per topic handle validation, schema mapping, and enrichment (复权价、均线、行业映射).  
   - Writes to storage targets:
     - TimescaleDB/PostgreSQL for tick/daily bars and indicators.
     - ClickHouse (or Elasticsearch) for text-heavy announcements/news with full-text search.
     - Redis for hot caches, deduplication locks, and rate-limit tokens.
   - ETL/Batch jobs (also Go) perform periodic recomputation and integrity checks.

4. **Public/Private APIs**  
   - Go-based gateway (e.g., Gin/Fiber + gRPC) exposes REST/gRPC/WebSocket endpoints for downstream systems.  
   - Authentication via API keys + HMAC, quota enforced through Redis.  
   - Web UI (optional) for dataset catalog and monitoring dashboards.

5. **Deployment & Networking**  
   - All Go binaries built statically and supervised via systemd or containerized stack.  
   - Internal network hosts the entire pipeline; Cloudflare Tunnel publishes only the API gateway.  
   - CI/CD (GitHub Actions or Drone) handles tests, builds, and artifact delivery to the LAN server.

6. **Observability & Ops**  
   - Prometheus scrapes exporters from crawlers, queue consumers, and databases.  
   - Grafana dashboards cover ingestion latency, queue lag, API latency, and storage growth.  
   - Alertmanager (email/Slack) notifies on crawler failure rates, queue backups, or API errors.  
   - Daily audits reconcile data counts between staging tables and canonical storage.

## Technology Decisions
- **Core Language**: Go for processing services, schedulers, and APIs due to concurrency model, deployment simplicity, and static binaries.
- **Crawlers**: Primarily Python, with flexibility to swap to Go/Node for specific sources.
- **Queue**: Kafka for durability and replay; NATS JetStream supported for lightweight deployments.
- **Databases**: TimescaleDB/PostgreSQL + ClickHouse + Redis (cache/rate-limit). Object storage (S3/MinIO) holds compressed raw dumps.
- **Schema Definition**: Protobuf (for queue/APIs) ensures forward compatibility across services.

## Roadmap
1. Define Protobuf schemas and topic contracts.
2. Scaffold Go services (ingestion consumer, API gateway) with CI/testing.
3. Build crawler framework with scheduling + proxy rotation.
4. Establish observability stack and tunnel configuration.
5. Expand dataset coverage (沪深两市, 港股延伸, alternative data).
