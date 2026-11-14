---
sidebar_position: 82
---

# Source Layer Analysis

This appendix records implementation responsibilities rather than treating directory names as architecture. The synchronized Chinese version is [源码分层分析](./source-layer-analysis.md); each term is defined in the [Source Glossary](./source-layer-glossary.en.md).

## Seven abstraction layers

| Layer | Responsibility | Representative implementation |
| --- | --- | --- |
| L01 Protocol and shared domain model | Owns REST, WebSocket, Envelope, gRPC, and instrument contracts plus cross-platform market, indicator, backtest, and risk models. | `packages/protocols/src/lib.rs:21`, `packages/core/src/lib.rs:8` |
| L02 Storage and infrastructure | Provides Memory, Redis, Postgres/TimescaleDB, ClickHouse, object storage, local KV/Parquet backends, queues, rate limiting, encryption, and partitioned lake IO. | `packages/storage/src/lib.rs:56` |
| L03 Collection and ingestion | Loads task templates, executes HTTP/scripts, cleans output, tracks health, archives raw evidence, and publishes to ingestion streams. | `services/collector/src/main.rs:21`, `services/collector/src/main_simple.rs:21` |
| L04 Stream processing and computation | Consumes ingestion streams, normalizes quotes, calculates indicators/risk/backtests/SQL, and writes hot data to TimescaleDB, ClickHouse, and the Parquet lake. | `services/data-engine/src/main.rs:190`, `services/data-engine/src/main.rs:245` |
| L05 Distribution, security, and external egress | Exposes REST/WebSocket, authenticates, throttles, proxies, synchronizes accounts, fans out realtime frames, and dispatches alert notifications. | `services/api-gateway/src/main.rs:152`, `services/real-time-feed/src/main.rs:94` |
| L06 Terminal runtimes | Assembles state, local persistence, platform bridges, presentation, and browser engines for Web, desktop, mobile, and WASM. | `web/app/src/App.tsx:34`, `desktop/src/state.rs:15`, `mobile/src/state.rs:58`, `wasm-analyzer/src/lib.rs:52` |
| L07 Operations and delivery | Supplies containers, observability, alerting, compliance, build, and release entry points consumed as configuration and telemetry seams. | `docker-compose.yml:14`, `config/prometheus.yml:1` |

## Call surface

- **Frontend to backend**: the React shell calls realtime `/ws` at `web/app/src/lib/liveFeed.ts:215`; the DuckDB workbench calls data-engine `/query` at `web/app/src/lib/duckdb.ts:42`; desktop panels detect the Tauri injection seam at `web/app/src/lib/desktop.ts:29`.
- **Server mainline**: collector writes Redis Streams at `services/collector/src/main_simple/executor.rs:393`; data-engine consumes it at `services/data-engine/src/normalizer.rs:24`, then writes TimescaleDB, ClickHouse, or Parquet.
- **Egress and cross-cutting**: API gateway connects Redis rate limiting at `services/api-gateway/src/main.rs:204` and optional Postgres account state at `services/api-gateway/src/main.rs:374`; the proxied target is carried in `services/api-gateway/src/main.rs:152`. Realtime consumes Redis at `services/real-time-feed/src/stream.rs:12`, while web clients only connect to `/ws`.
- **Alert chain**: Prometheus scrapes metrics per `config/prometheus.yml:14`; Alertmanager forwards to alert-webhook `/alerts`; alert-webhook reads external channel configuration at `services/alert-webhook/src/main.rs:241` and sends notifications over HTTP.
- **Platform shells**: Android/iOS call `mobile/src/state.rs:58` through UniFFI; the Tauri process wires the desktop framework layer into OS windows and exports at `desktop/src/main.rs:8`.

## State stores

| State | Store | Evidence |
| --- | --- | --- |
| Collector tasks and running status | In-process `RwLock<HashMap>` | `services/collector/src/main_simple.rs:28` |
| Raw collector events | Tokio broadcast | `services/collector/src/main_simple.rs:34` |
| Ingestion messages and DLQ | Redis Streams | `packages/storage/src/redis_streams.rs:23` |
| Data-engine hot series | In-process `TimeSeriesStorage` | `services/data-engine/src/main.rs:253` |
| Normalized mirror / history | TimescaleDB, ClickHouse, Parquet files | `services/data-engine/src/main.rs:193`, `services/data-engine/src/main.rs:217` |
| Realtime connections, alert windows, sync cursors | In-process broadcast/Mutex/HashMap | `services/real-time-feed/src/main.rs:94` |
| Gateway JWKS, abuse state, rate limit | Memory; rate limit in Redis | `services/api-gateway/src/main.rs:164`, `services/api-gateway/src/main.rs:158` |
| Accounts and cross-device sync | In-memory authority with optional Postgres write-through | `services/api-gateway/src/main.rs:170` |
| Web workspace/theme/privacy | Browser localStorage | `web/app/src/App.tsx:41` |
| Desktop config/alerts/window/KV/export | Files under platform directories | `desktop/src/paths.rs:9` |
| Mobile offline/preferences | Host-injected KV; Rust core decisions in memory | `mobile/src/state.rs:70`, `mobile/android/app/src/main/java/com/alpha/finance/mobile/OfflineStore.kt:19` |
| Raw collection evidence | Optional S3/MinIO object storage | `services/collector/src/raw_archive.rs:28` |

## Configuration, lifecycle, and artifacts

Services use explicit startup assembly. Collector consumes `ALPHA_WORKSPACE_ROOT`, `ALPHA_COLLECTOR_TASKS`, `ALPHA_COLLECTOR_BIND`, and optional raw-archive variables; data-engine defines file-plus-environment loading at `services/data-engine/src/settings.rs:10`; gateway combines CLI defaults with `ALPHA_GATEWAY_*` at `services/api-gateway/src/main.rs:64`; realtime and alert-webhook consume `ALPHA_*`/external webhook variables. Desktop builds `web/app/dist` through `desktop/tauri.conf.json:3` and injects the directory contract from `desktop/src/paths.rs:9`; mobile receives FFI configuration at construction and later wires host KV.

Lifecycle is startup assembly, background execution, then health/metrics exposure. Collector starts cron and HTTP after state assembly (`services/collector/src/main.rs:22`); data-engine initializes hot storage, TimescaleDB, ClickHouse, lake tables, and stream tasks (`services/data-engine/src/main.rs:433`); realtime degrades to a simulated feed without Redis (`services/real-time-feed/src/main.rs:137`); gateway connects optional external state before HTTP/TLS (`services/api-gateway/src/main.rs:176`); desktop/mobile hand host paths or platform bridges to Rust core state.

Protocol schemas live in `packages/protocols/proto/data_engine.proto:1`; REST/WS/Envelope Rust contracts live in `packages/protocols/src/lib.rs:21`. Service-specific YAML/JSON schemas are defined in code: collector templates at `services/collector/src/task_templates/schema.rs:40`, data-engine settings at `services/data-engine/src/settings.rs:10`, and gateway CLI/env defaults at `services/api-gateway/src/main.rs:64`. Copy lives in `web/app/src/lib/i18n.ts:1` and Android resources at `mobile/android/app/src/main/res/values/strings.xml:1`; desktop reuses `web/app`, while iOS copy is embedded in SwiftUI views. Dedicated operations artifacts are rooted at `docker-compose.yml:1` and `config/`.

## Why these boundaries exist

1. Protocol contracts must be shareable: REST, WS, Envelope, and instrument symbols export from `packages/protocols/src/lib.rs:21`, while data-engine's wire contract is separate at `packages/protocols/proto/data_engine.proto:12`, preventing copied field semantics.
2. Storage backends must be swappable rather than service-hardcoded: `packages/storage/src/lib.rs:56` defines six storage operations; the factory supports five backends at `packages/storage/src/lib.rs:76`; queue, limiter, ClickHouse, and lake keep specialized interfaces for their access patterns.
3. Collection and processing need a replayable boundary: collector state and events are in-process (`services/collector/src/main_simple.rs:21`), output crosses Redis Streams (`services/collector/src/main_simple/executor.rs:393`), and data-engine can degrade across TimescaleDB/ClickHouse modes (`services/data-engine/src/bootstrap.rs:46`).
4. Realtime fan-out is separate from persistent processing: realtime uses process-local broadcast and connection state (`services/real-time-feed/src/main.rs:94`), frames use the shared contract (`packages/protocols/src/websocket.rs:33`), and Redis is only upstream (`services/real-time-feed/src/stream.rs:12`).
5. Security and routing must cross-cut before proxying: gateway composes middleware at `services/api-gateway/src/main.rs:406`, keeps `/v1/*path`, account, and WS proxy routes distinct (`services/api-gateway/src/main.rs:392`), and encapsulates account semantics in `services/api-gateway/src/account.rs:160`.
6. Algorithms are shared across terminals; only bridges/shells differ: mobile states that computation belongs to alpha-core (`mobile/src/lib.rs:11`) and exports one UniFFI seam (`mobile/src/lib.rs:57`); desktop separates the Tauri-free framework from wiring (`desktop/src/lib.rs:5`) and directly reuses `web/app/dist` from `desktop/tauri.conf.json:6`; WASM exposes browser analysis and sync seams (`wasm-analyzer/src/lib.rs:46`).
7. Notification delivery is an independent boundary: Alertmanager payload models and four sender adapters are isolated at `services/alert-webhook/src/main.rs:79` and `services/alert-webhook/src/main.rs:471`; the service carries no long-lived business state (`services/alert-webhook/src/main.rs:37`).
