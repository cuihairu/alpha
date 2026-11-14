---
sidebar_position: 84
---

# Source Glossary

This glossary records abstract concepts by implementation responsibility. The synchronized Chinese version is [源码术语表](./source-layer-glossary.md); the layer overview is [Source Layer Analysis](./source-layer-analysis.en.md).

| Term | English | Definition | file:line |
| --- | --- | --- | --- |
| Protocol package | Protocol package | Shared export surface for REST, WebSocket, event, instrument, and gRPC contracts. | `packages/protocols/src/lib.rs:21` |
| REST contract | REST contract | Shapes for API responses, pagination, search, history requests, and error codes. | `packages/protocols/src/rest.rs:9` |
| WebSocket contract | WebSocket contract | Shared frames for subscribe, authentication, versioned sync, heartbeat, and errors. | `packages/protocols/src/websocket.rs:8` |
| Event envelope | Event envelope | Version, source, dedup, timing, sequence, and trace fields for Redis Streams messages. | `packages/protocols/src/events.rs:29` |
| gRPC contract | gRPC contract | Protobuf schema for data-engine indicators, history, and performance. | `packages/protocols/proto/data_engine.proto:12` |
| Instrument ID | Instrument ID | Stable market/exchange/symbol identifier and parser rules. | `packages/protocols/src/instrument.rs:136` |
| Shared core | Shared core | Cross-platform accounting, indicators, patterns, backtesting, risk, cache, and streaming algorithms. | `packages/core/src/lib.rs:8` |
| Market model | Market model | Domain payloads for quotes, instruments, indicators, and derivatives. | `packages/core/src/models.rs:1` |
| Hybrid cache | Hybrid cache | Core cache strategy combining resident hot data with evictable refill. | `packages/core/src/hybrid_cache.rs:1` |
| Platform adapter | Platform adapter | Core abstraction exposed to WASM/JNI/host environments. | `packages/core/src/platform.rs:1` |
| Storage abstraction | Storage abstraction | Common KV interface and factory with five backend assemblies. | `packages/storage/src/lib.rs:56` |
| Redis KV | Redis KV | TTL-backed Redis string KV backend. | `packages/storage/src/redis_kv.rs:8` |
| Postgres KV | Postgres KV | Postgres KV write-through backend available to gateway account snapshots. | `packages/storage/src/postgres_kv.rs:1` |
| Redis Streams | Redis Streams | Ingestion backend with consumer groups, ACK, pending, and DLQ. | `packages/storage/src/redis_streams.rs:23` |
| Redis rate limiter | Redis rate limiter | Distributed quota component backed by Redis. | `packages/storage/src/rate_limit.rs:1` |
| ClickHouse storage | ClickHouse storage | Columnar history backend for batch market-data writes and exports. | `packages/storage/src/clickhouse.rs:1` |
| Timescale storage | Timescale storage | Postgres time-series backend for quote mirrors and time queries. | `packages/storage/src/timescale.rs:1` |
| Parquet lake | Parquet lake | Partitioned Parquet history with manifests and compaction. | `packages/storage/src/lake.rs:1` |
| Object storage | Object storage | S3/MinIO connection-string, upload, and forensic artifact backend. | `packages/storage/src/cloud.rs:1` |
| Collector | Collector | Process managing task state, execution, events, health, publishing, and HTTP admin. | `services/collector/src/main_simple.rs:21` |
| Collection task template | Collection task template | Declarative YAML/JSON schema and validator for TaskDefinition. | `services/collector/src/task_templates/schema.rs:40` |
| Collection source | Collection source | Concrete Eastmoney, Sina, Tencent, and NetEase fetchers. | `services/collector/src/sources/mod.rs:1` |
| Multilang crawler | Multilang crawler | Process executor for Python/Node/Go/Rust/Shell tasks. | `services/collector/src/multilang_simple.rs:1` |
| Source health ledger | Source health ledger | Derives unknown/healthy/degraded/down state from execution results. | `services/collector/src/source_health.rs:1` |
| Data engine | Data engine | Service that consumes streams, validates data, runs SQL/indicators, and writes backends. | `services/data-engine/src/main.rs:190` |
| Hot series store | Hot series store | In-process queryable series state inside data-engine. | `services/data-engine/src/main.rs:253` |
| API gateway | API gateway | HTTP/WS boundary composing authentication, abuse protection, rate limiting, proxying, and accounts. | `services/api-gateway/src/main.rs:152` |
| Account store | Account store | In-memory authority with optional Postgres write-through for cross-device sync. | `services/api-gateway/src/account.rs:160` |
| JWT/JWKS | JWT/JWKS | External IdP key cache combined with local JWT verification. | `services/api-gateway/src/jwt.rs:1` |
| Abuse shield | Abuse shield | In-process scan, burst, and bot protection middleware. | `services/api-gateway/src/shield.rs:1` |
| Realtime feed | Realtime feed | Redis consumption, simulated fallback, WS subscription, and versioned fan-out. | `services/real-time-feed/src/main.rs:94` |
| Alert webhook | Alert webhook | Alertmanager receiver and adapter boundary for external notifications. | `services/alert-webhook/src/main.rs:37` |
| Terminal runtime | Terminal runtime | Composed state and bridge surface for Web, desktop, mobile, and WASM. | `web/app/src/App.tsx:34` |
| Browser analysis engine | Browser analysis engine | WASM analysis, shared buffer, and streaming entry points. | `wasm-analyzer/src/lib.rs:52` |
| Desktop framework layer | Desktop framework layer | Tauri-free paths, config, state, offline, and export logic. | `desktop/src/lib.rs:5` |
| Desktop wiring layer | Desktop wiring layer | Process surface exposing the framework as Tauri commands/windows/shortcuts. | `desktop/src/gui.rs:1` |
| Mobile core | Mobile core | UniFFI state object for watchlists, analysis, notification/sync/offline decisions. | `mobile/src/state.rs:58` |
| Host offline store | Host offline store | Android shell adapter providing file KV and snapshots to the core. | `mobile/android/app/src/main/java/com/alpha/finance/mobile/OfflineStore.kt:19` |
| Operations config surface | Operations config surface | Declarative containers, collection templates, observability, alerts, and compliance. | `docker-compose.yml:1` |
| Observability pipeline | Observability pipeline | Prometheus scraping, Loki logs, and Grafana presentation. | `config/prometheus.yml:1` |
