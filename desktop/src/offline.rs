//! 本地数据库同步与离线模式（TODO L116）
//!
//! 口径：**降级判定与增量语义全在框架层**（Linux 门禁可测），网络 IO 只有
//! 两处且都是最小实现——`/health` 连通探测（[`probe_health`]）与远端取数缝
//! （[`QuoteRemote`]，生产实现 [`SyntheticRemote`] 即演示行情口径）。本地数据库
//! 复用 [`crate::kv::FileKeyValueStore`]（kv 目录，每键一文件）。
//!
//! 非交互假设（自行判定，已注明）：
//! 1. 「本地数据库」= FileKeyValueStore 的文件 KV（L38 适配层注释「文件/SQLite」
//!    的文件路线）；SQLite/查询型存储等出现范围查询需求再换，读写面已收拢在本模块；
//! 2. 「增量同步」按**记录内容指纹**（价格+成交量 → [`content_seq`]）比对：远端
//!    尚无版本号/水位协议，指纹一致的记录跳过、不一致才落库；接入真实协议时
//!    只换 `QuoteRemote` 实现并让 seq 来自服务端水位，[`sync_from`] 语义不变；
//! 3. 纯 TCP 探测做不了 TLS（https 目标保守判离线，走缓存降级）；演示配置
//!    `api_url` 默认 `http://localhost:8080`，方括号 IPv6 不支持（`host:port` 口径）；
//! 4. 离线读缓存缺失的标的不报错也不编造——列入 `missing` 如实上报（宁缺毋滥）；
//! 5. 同步方向是「远端 → 本地」只读行情；本地告警/配置无对应远端契约，不上传。

use crate::error::{DesktopError, DesktopResult};
use crate::kv::FileKeyValueStore;
use crate::market;
use alpha_core::errors::{AlphaError, AlphaResult};
use alpha_core::models::MarketData;
use alpha_core::platform::KeyValueStore;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// 本地数据库中行情快照的键前缀（kv 目录内按 symbol 派生）
pub const QUOTE_KEY_PREFIX: &str = "quotes/";

/// 连通探测超时（含 TCP 连接 + HTTP 往返；演示后端在同机/局域网，500ms 足够）
pub const DEFAULT_PROBE_TIMEOUT: Duration = Duration::from_millis(500);

/// 远端拉取成功且已落库的记录来源标记
pub const SOURCE_LIVE: &str = "live";
/// 命中本地缓存（离线或远端失败降级）的记录来源标记
pub const SOURCE_CACHE: &str = "cache";

/// 内容指纹：价格 + 成交量的确定性哈希（增量比对的水位）
///
/// 远端无版本号时的兜底口径：内容不变 → 指纹不变 → 同步跳过；内容变化 →
/// 指纹必变 → 触发落库。价格按 f64 位模式参与（NaN 在行情域不出现）。
pub fn content_seq(price: f64, volume: u64) -> u64 {
    // FNV-1a 64 位
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in price
        .to_le_bytes()
        .iter()
        .chain(volume.to_le_bytes().iter())
    {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    hash
}

/// 本地数据库的行情快照行（持久化契约：字段名即 kv 值的 JSON 键）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuoteRecord {
    /// 快照数据（整体存储，避免读缓存时丢 open/high/low 等展示字段）
    pub data: MarketData,
    /// 内容指纹（[`content_seq`]；接入真实协议后由服务端水位替代）
    pub seq: u64,
    /// 最近一次落库时刻
    pub synced_at: DateTime<Utc>,
}

impl QuoteRecord {
    /// 由远端快照构造（`at` 为落库时刻）
    pub fn from_market(data: MarketData, at: DateTime<Utc>) -> Self {
        Self {
            seq: content_seq(data.price, data.volume),
            data,
            synced_at: at,
        }
    }
}

/// 远端取数缝（L3；HTTP 实现属后续——演示口径 [`SyntheticRemote`]，真实后端
/// 接入时替换本 trait 实现即可，同步/降级语义不动）
#[async_trait]
pub trait QuoteRemote: Send + Sync + std::fmt::Debug {
    /// 拉取单个标的的快照；失败（网络/服务端错误）由调用方降级
    async fn fetch(&self, symbol: &str) -> AlphaResult<MarketData>;
}

/// 演示行情口径的远端实现（确定性合成数据，指纹恒定——首拉后同步恒报 unchanged；
/// 可变内容的行为由单测桩覆盖）
#[derive(Debug, Clone, Copy, Default)]
pub struct SyntheticRemote;

#[async_trait]
impl QuoteRemote for SyntheticRemote {
    async fn fetch(&self, symbol: &str) -> AlphaResult<MarketData> {
        if symbol.trim().is_empty() {
            return Err(AlphaError::invalid_input("标的代码不能为空"));
        }
        Ok(market::synthetic_quote(symbol))
    }
}

/// 解析 `http://host[:port]` 目标（探测用）。https 与未知 scheme 返回 `None`
/// （纯 TCP 无 TLS 能力，保守判离线）；路径/查询串剥离，端口缺省 80。
fn parse_http_target(api_base: &str) -> Option<(String, u16)> {
    let rest = api_base.trim().strip_prefix("http://")?;
    let authority = rest.split(['/', '?', '#']).next()?;
    if authority.is_empty() {
        return None;
    }
    match authority.rsplit_once(':') {
        Some((host, port)) => Some((host.to_string(), port.parse().ok()?)),
        None => Some((authority.to_string(), 80)),
    }
}

/// 连通探测：对 `api_base` 的 `/health` 发最小 HTTP GET（真实网络 IO，超时
/// [`DEFAULT_PROBE_TIMEOUT`] 或调用方指定）。任何失败（连接拒绝/超时/非 200）
/// 都按「离线」处理——探测只服务降级决策，不上报错误细节。
pub async fn probe_health(api_base: &str, timeout: Duration) -> bool {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let Some((host, port)) = parse_http_target(api_base) else {
        return false;
    };
    let request =
        format!("GET /health HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\n\r\n");
    let io = async {
        let mut stream = tokio::net::TcpStream::connect((host.as_str(), port))
            .await
            .ok()?;
        stream.write_all(request.as_bytes()).await.ok()?;
        stream.shutdown().await.ok()?;
        let mut buf = Vec::new();
        let mut chunk = [0u8; 512];
        loop {
            let n = stream.read(&mut chunk).await.ok()?;
            if n == 0 || buf.len() > 4096 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        Some(buf)
    };
    match tokio::time::timeout(timeout, io).await {
        Ok(Some(buf)) => {
            // 状态行形如 "HTTP/1.1 200 OK"，取第二个空格分隔段与 "200" 精确比对
            buf.starts_with(b"HTTP/")
                && buf
                    .split(|b| *b == b'\r')
                    .next()
                    .and_then(|line| line.split(|b| *b == b' ').nth(1))
                    == Some(&b"200"[..])
        }
        _ => false,
    }
}

/// 本地数据库读取：缺失或值损坏 → `None`（容错同 config/alerts 口径——
/// 半截文件不应让离线读路径崩掉）；存储层真实 IO 错误照常上抛
pub async fn load_quote(
    store: &FileKeyValueStore,
    symbol: &str,
) -> DesktopResult<Option<QuoteRecord>> {
    let key = format!("{QUOTE_KEY_PREFIX}{symbol}");
    let Some(bytes) = store.get(&key).await.map_err(DesktopError::from)? else {
        return Ok(None);
    };
    Ok(serde_json::from_slice(&bytes).ok())
}

/// 本地数据库写入（值 = JSON 序列化的 [`QuoteRecord`]）
pub async fn save_quote(store: &FileKeyValueStore, record: &QuoteRecord) -> DesktopResult<()> {
    let key = format!("{QUOTE_KEY_PREFIX}{}", record.data.symbol);
    let json = serde_json::to_vec(record)?;
    store.set(&key, &json).await.map_err(DesktopError::from)
}

/// 一次离线感知读取的载荷（前端契约：字段名即 DOM/渲染依据）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuotesPayload {
    /// 拿到的行情（live = 远端实拉，cache = 本地缓存降级），保持请求顺序
    pub quotes: Vec<QuoteView>,
    /// 无数据可用的标的（在线拉取失败且无缓存 / 离线且无缓存）
    pub missing: Vec<String>,
    /// 本次探测的连通性（探测失败 ≠ 每条记录降级原因，逐条看 source）
    pub online: bool,
}

/// 单条行情 + 来源标记（serde flatten：MarketData 字段平铺，`source` 附加）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuoteView {
    /// 行情快照（整体存储口径：open/high/low 等展示字段不丢）
    #[serde(flatten)]
    pub data: MarketData,
    /// `live` / `cache`（[`SOURCE_LIVE`] / [`SOURCE_CACHE`]）
    pub source: String,
}

/// 离线感知读路径（探测 + 读取）：
/// * 在线 → 逐标的远端拉取并**写穿落库**；单标的拉取失败 → 本地缓存降级；
/// * 离线 → 只读本地缓存；
/// * 缓存也缺失 → `missing`（不报错、不编造，见模块假设 4）；
/// * 空标的列表 → `Err`（与 [`crate::analysis::quotes_request`] 同口径）；
/// * 本地落库失败 → `Err`（持久化是本条核心能力，响亮失败好过静默丢缓存）。
pub async fn quotes_request(
    store: &FileKeyValueStore,
    remote: &dyn QuoteRemote,
    symbols: &[String],
    api_base: &str,
    at: DateTime<Utc>,
) -> DesktopResult<QuotesPayload> {
    let online = probe_health(api_base, DEFAULT_PROBE_TIMEOUT).await;
    quotes_from(store, remote, symbols, online, at).await
}

/// 降级矩阵的纯逻辑部分（连通性已定；单测直驱，不用真实网络）
pub async fn quotes_from(
    store: &FileKeyValueStore,
    remote: &dyn QuoteRemote,
    symbols: &[String],
    online: bool,
    at: DateTime<Utc>,
) -> DesktopResult<QuotesPayload> {
    if symbols.is_empty() {
        return Err(DesktopError::InvalidInput("symbols 不能为空".to_string()));
    }
    let mut quotes = Vec::new();
    let mut missing = Vec::new();
    for symbol in symbols {
        let mut view = None;
        if online {
            // 在线但该标的拉取失败：落到下面的缓存降级（部分失败不算整体失败）
            if let Ok(data) = remote.fetch(symbol).await {
                let record = QuoteRecord::from_market(data, at);
                save_quote(store, &record).await?;
                view = Some(QuoteView {
                    data: record.data,
                    source: SOURCE_LIVE.to_string(),
                });
            }
        }
        if view.is_none() {
            match load_quote(store, symbol).await? {
                Some(record) => {
                    view = Some(QuoteView {
                        data: record.data,
                        source: SOURCE_CACHE.to_string(),
                    })
                }
                None => missing.push(symbol.clone()),
            }
        }
        if let Some(v) = view {
            quotes.push(v);
        }
    }
    Ok(QuotesPayload {
        quotes,
        missing,
        online,
    })
}

/// 一次增量同步的报告（前端契约：字段名即渲染依据）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncReport {
    /// 本次探测的连通性；离线时 applied/unchanged/failed 全空（无操作降级，非错误）
    pub online: bool,
    /// 指纹有变化并已落库的标的
    pub applied: Vec<String>,
    /// 指纹一致被跳过的标的数
    pub unchanged: usize,
    /// 拉取失败的标的（保留本地旧值，不回滚已成功的）
    pub failed: Vec<String>,
    /// 同步时刻
    pub synced_at: DateTime<Utc>,
}

/// 联网恢复增量同步（探测 + 同步）：
/// * 离线 → 返回 `online=false` 的空报告（降级语义：无操作不是错误）；
/// * 在线 → 逐标的拉取，指纹与本地一致 → `unchanged`；不同（或本地无此记录）
///   → 落库并 `applied`；拉取失败 → `failed` 且本地保留；
/// * 空标的列表 → `Err`。
pub async fn sync_request(
    store: &FileKeyValueStore,
    remote: &dyn QuoteRemote,
    symbols: &[String],
    api_base: &str,
    at: DateTime<Utc>,
) -> DesktopResult<SyncReport> {
    let online = probe_health(api_base, DEFAULT_PROBE_TIMEOUT).await;
    sync_from(store, remote, symbols, online, at).await
}

/// 增量同步的纯逻辑部分（连通性已定；单测直驱）
pub async fn sync_from(
    store: &FileKeyValueStore,
    remote: &dyn QuoteRemote,
    symbols: &[String],
    online: bool,
    at: DateTime<Utc>,
) -> DesktopResult<SyncReport> {
    let mut report = SyncReport {
        online,
        applied: Vec::new(),
        unchanged: 0,
        failed: Vec::new(),
        synced_at: at,
    };
    if symbols.is_empty() {
        return Err(DesktopError::InvalidInput("symbols 不能为空".to_string()));
    }
    if !online {
        return Ok(report);
    }
    for symbol in symbols {
        match remote.fetch(symbol).await {
            Ok(data) => {
                let fresh = QuoteRecord::from_market(data, at);
                let changed = match load_quote(store, symbol).await? {
                    Some(cached) => cached.seq != fresh.seq,
                    None => true,
                };
                if changed {
                    save_quote(store, &fresh).await?;
                    report.applied.push(symbol.clone());
                } else {
                    report.unchanged += 1;
                }
            }
            Err(_) => report.failed.push(symbol.clone()),
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn at() -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000, 0).expect("固定时间戳")
    }

    fn store(dir: &std::path::Path) -> FileKeyValueStore {
        FileKeyValueStore::new(dir)
    }

    /// 内容可控的远端桩：按标的返回指定快照；`fail` 集合里的标的拉取必败
    #[derive(Debug, Default)]
    struct StubRemote {
        table: Mutex<std::collections::HashMap<String, (f64, u64)>>,
        fail: Vec<String>,
    }

    impl StubRemote {
        fn with(symbol: &str, price: f64, volume: u64) -> Self {
            let mut table = std::collections::HashMap::new();
            table.insert(symbol.to_string(), (price, volume));
            Self {
                table: Mutex::new(table),
                fail: Vec::new(),
            }
        }

        fn failing(symbol: &str) -> Self {
            Self {
                table: Mutex::new(std::collections::HashMap::new()),
                fail: vec![symbol.to_string()],
            }
        }
    }

    #[async_trait]
    impl QuoteRemote for StubRemote {
        async fn fetch(&self, symbol: &str) -> AlphaResult<MarketData> {
            if self.fail.iter().any(|s| s == symbol) {
                return Err(AlphaError::network("桩注入的拉取失败"));
            }
            let (price, volume) = self
                .table
                .lock()
                .expect("桩表锁")
                .get(symbol)
                .copied()
                .unwrap_or((100.0, 1_000));
            Ok(MarketData {
                symbol: symbol.to_string(),
                timestamp: at(),
                price,
                volume,
                bid: None,
                ask: None,
                open: Some(price),
                high: None,
                low: None,
            })
        }
    }

    fn tempdir() -> tempfile::TempDir {
        tempfile::tempdir().expect("临时目录")
    }

    #[test]
    fn content_seq_is_deterministic_and_sensitive() {
        let base = content_seq(100.0, 1_000);
        assert_eq!(base, content_seq(100.0, 1_000), "同内容同指纹");
        assert_ne!(base, content_seq(100.5, 1_000), "价格变化 → 指纹变化");
        assert_ne!(base, content_seq(100.0, 1_001), "成交量变化 → 指纹变化");
        assert_ne!(base, content_seq(1e-300, 1_000), "位模式不同 → 指纹不同");
    }

    #[test]
    fn quote_record_embeds_content_fingerprint() {
        let data = MarketData {
            symbol: "600519".to_string(),
            timestamp: at(),
            price: 1700.0,
            volume: 20_240,
            bid: None,
            ask: None,
            open: None,
            high: None,
            low: None,
        };
        let record = QuoteRecord::from_market(data, at());
        assert_eq!(record.seq, content_seq(1700.0, 20_240));
    }

    #[test]
    fn parse_http_target_accepts_host_port_and_defaults() {
        assert_eq!(
            parse_http_target("http://localhost:8080"),
            Some(("localhost".to_string(), 8080))
        );
        assert_eq!(
            parse_http_target("http://127.0.0.1:1/health?x=1"),
            Some(("127.0.0.1".to_string(), 1))
        );
        assert_eq!(
            parse_http_target("http://backend.internal"),
            Some(("backend.internal".to_string(), 80)),
            "缺省端口 80"
        );
        assert_eq!(
            parse_http_target("https://example.com"),
            None,
            "TLS 不支持 → None"
        );
        assert_eq!(parse_http_target("ftp://x"), None);
        assert_eq!(parse_http_target("http://"), None);
    }

    #[tokio::test]
    async fn probe_health_detects_live_endpoint() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("绑定临时端口");
        let addr = listener.local_addr().expect("端口");
        std::thread::spawn(move || {
            if let Ok((socket, _)) = listener.accept() {
                let mut socket = socket;
                use std::io::{Read, Write};
                let mut buf = [0u8; 512];
                let _ = socket.read(&mut buf);
                let _ = socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
            }
        });
        assert!(
            probe_health(&format!("http://{addr}"), Duration::from_secs(2)).await,
            "200 健康端点应判在线"
        );
    }

    #[tokio::test]
    async fn probe_health_rejects_refused_and_non_200_and_https() {
        // 连接拒绝（本机保留端口 1 无监听）
        assert!(!probe_health("http://127.0.0.1:1", Duration::from_millis(300)).await);
        // 非 200 响应：健康端点返回 5xx 视为不可用
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("绑定临时端口");
        let addr = listener.local_addr().expect("端口");
        std::thread::spawn(move || {
            if let Ok((socket, _)) = listener.accept() {
                let mut socket = socket;
                use std::io::{Read, Write};
                let mut buf = [0u8; 512];
                let _ = socket.read(&mut buf);
                let _ = socket
                    .write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\n\r\n");
            }
        });
        assert!(!probe_health(&format!("http://{addr}"), Duration::from_secs(2)).await);
        // 纯 TCP 无 TLS：https 目标保守判离线
        assert!(!probe_health("https://example.com", Duration::from_millis(300)).await);
    }

    #[tokio::test]
    async fn probe_health_times_out_on_slow_server() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("绑定临时端口");
        let addr = listener.local_addr().expect("端口");
        std::thread::spawn(move || {
            if let Ok((socket, _)) = listener.accept() {
                std::thread::sleep(Duration::from_millis(400));
                drop(socket);
            }
        });
        assert!(
            !probe_health(&format!("http://{addr}"), Duration::from_millis(80)).await,
            "慢于超时的服务端应判离线"
        );
    }

    #[tokio::test]
    async fn quote_record_roundtrips_through_local_db() {
        let tmp = tempdir();
        let db = store(tmp.path());
        assert!(load_quote(&db, "600519")
            .await
            .expect("无记录 → None")
            .is_none());

        let data = market::synthetic_quote_at("600519", at());
        save_quote(&db, &QuoteRecord::from_market(data.clone(), at()))
            .await
            .expect("落库");
        let loaded = load_quote(&db, "600519").await.expect("读取");
        assert_eq!(
            loaded.as_ref().map(|r| &r.data),
            Some(&data),
            "整体快照往返一致"
        );
        assert_eq!(
            loaded.as_ref().map(|r| r.seq),
            Some(content_seq(data.price, data.volume))
        );
    }

    #[tokio::test]
    async fn corrupt_db_value_degrades_to_missing_not_error() {
        let tmp = tempdir();
        let db = store(tmp.path());
        let key = crate::kv::key_filename(&format!("{QUOTE_KEY_PREFIX}600519"));
        std::fs::write(tmp.path().join(&key), b"{half written").expect("写损坏值");
        assert!(
            load_quote(&db, "600519")
                .await
                .expect("损坏值按未命中处理")
                .is_none(),
            "半截 JSON 不应让离线读路径报错"
        );
    }

    #[tokio::test]
    async fn online_read_fetches_and_write_throughs_to_db() {
        let tmp = tempdir();
        let db = store(tmp.path());
        let remote = StubRemote::with("600519", 1700.0, 100);
        let payload = quotes_from(&db, &remote, &["600519".to_string()], true, at())
            .await
            .expect("在线读取");
        assert!(payload.online);
        assert_eq!(payload.quotes.len(), 1);
        assert_eq!(payload.quotes[0].source, SOURCE_LIVE);
        assert!(payload.missing.is_empty());
        // 写穿验证：本地库已有快照
        let cached = load_quote(&db, "600519")
            .await
            .expect("读取")
            .expect("应已落库");
        assert_eq!(cached.data.price, 1700.0);
    }

    #[tokio::test]
    async fn online_fetch_failure_falls_back_to_local_cache() {
        let tmp = tempdir();
        let db = store(tmp.path());
        // 先落一份缓存
        let seed = market::synthetic_quote_at("600519", at());
        save_quote(&db, &QuoteRecord::from_market(seed, at()))
            .await
            .expect("预热缓存");
        // 在线但该标的拉取失败 → 降级到缓存
        let remote = StubRemote::failing("600519");
        let payload = quotes_from(&db, &remote, &["600519".to_string()], true, at())
            .await
            .expect("部分失败仍应返回载荷");
        assert!(payload.online);
        assert_eq!(payload.quotes[0].source, SOURCE_CACHE, "远端失败降级到缓存");
        assert!(payload.missing.is_empty());
    }

    #[tokio::test]
    async fn offline_read_serves_cache_only_and_reports_missing() {
        let tmp = tempdir();
        let db = store(tmp.path());
        let seed = market::synthetic_quote_at("000001", at());
        save_quote(&db, &QuoteRecord::from_market(seed, at()))
            .await
            .expect("预热缓存");

        let remote = StubRemote::with("000001", 1.0, 1);
        let payload = quotes_from(
            &db,
            &remote,
            &["000001".to_string(), "600519".to_string()],
            false,
            at(),
        )
        .await
        .expect("离线读取");
        assert!(!payload.online);
        assert_eq!(payload.quotes.len(), 1, "只回缓存命中的标的");
        assert_eq!(payload.quotes[0].source, SOURCE_CACHE);
        assert_eq!(payload.quotes[0].data.symbol, "000001");
        assert_eq!(payload.missing, ["600519"], "缓存缺失如实上报");
    }

    #[tokio::test]
    async fn quotes_request_rejects_empty_symbol_list() {
        let tmp = tempdir();
        let err = quotes_from(&store(tmp.path()), &SyntheticRemote, &[], true, at())
            .await
            .expect_err("空列表应报错");
        assert_eq!(err.kind(), "invalid_input");
        assert!(err.to_string().contains("symbols"));
    }

    #[tokio::test]
    async fn online_read_fails_loudly_when_db_unwritable() {
        // 落库目标是文件：写穿应响亮失败（持久化是本条核心，静默丢缓存不可接受）
        let tmp = tempdir();
        let blocker = tmp.path().join("blocker");
        std::fs::write(&blocker, b"file").expect("写占位文件");
        let db = FileKeyValueStore::new(&blocker);
        let remote = StubRemote::with("600519", 1.0, 1);
        let err = quotes_from(&db, &remote, &["600519".to_string()], true, at())
            .await
            .expect_err("写穿失败应上抛");
        assert_eq!(
            err.kind(),
            "core",
            "kv 存储错误经 AlphaError 映射，实际 {err:?}"
        );
    }

    #[tokio::test]
    async fn sync_applies_only_changed_records() {
        let tmp = tempdir();
        let db = store(tmp.path());
        let remote = StubRemote::with("600519", 1700.0, 100);

        // 首次同步：本地无记录 → applied
        let first = sync_from(&db, &remote, &["600519".to_string()], true, at())
            .await
            .expect("首次同步");
        assert_eq!(first.applied, ["600519"]);
        assert_eq!(first.unchanged, 0);
        assert!(first.failed.is_empty());
        assert!(first.online);

        // 二次同步：内容未变 → 指纹一致 → unchanged（增量语义的核心断言）
        let second = sync_from(&db, &remote, &["600519".to_string()], true, at())
            .await
            .expect("二次同步");
        assert!(second.applied.is_empty(), "同内容不应重复落库");
        assert_eq!(second.unchanged, 1);

        // 远端内容变化 → 指纹变化 → 再落库（块级作用域确保锁不跨 await）
        {
            let mut table = remote.table.lock().expect("桩表锁");
            table.insert("600519".to_string(), (1800.0, 100));
        }
        let third = sync_from(&db, &remote, &["600519".to_string()], true, at())
            .await
            .expect("变化后同步");
        assert_eq!(third.applied, ["600519"], "内容变化应触发落库");
        let latest = load_quote(&db, "600519")
            .await
            .expect("读取")
            .expect("应有新值");
        assert_eq!(latest.data.price, 1800.0);
    }

    #[tokio::test]
    async fn sync_keeps_local_value_when_remote_fetch_fails() {
        let tmp = tempdir();
        let db = store(tmp.path());
        let seed = market::synthetic_quote_at("600519", at());
        save_quote(&db, &QuoteRecord::from_market(seed, at()))
            .await
            .expect("预热");
        let remote = StubRemote::failing("600519");
        let report = sync_from(&db, &remote, &["600519".to_string()], true, at())
            .await
            .expect("单标的失败不应整体报错");
        assert_eq!(report.failed, ["600519"]);
        assert!(report.applied.is_empty());
        let cached = load_quote(&db, "600519")
            .await
            .expect("读取")
            .expect("本地保留");
        assert_eq!(
            cached.data,
            market::synthetic_quote_at("600519", at()),
            "旧值未被破坏"
        );
    }

    #[tokio::test]
    async fn offline_sync_is_a_no_op_report_not_an_error() {
        let tmp = tempdir();
        let report = sync_from(
            &store(tmp.path()),
            &SyntheticRemote,
            &["600519".to_string()],
            false,
            at(),
        )
        .await
        .expect("离线同步应成功返回空报告");
        assert!(!report.online);
        assert!(report.applied.is_empty());
        assert_eq!(report.unchanged, 0);
        assert!(report.failed.is_empty());
    }

    #[tokio::test]
    async fn sync_request_rejects_empty_symbol_list() {
        let tmp = tempdir();
        let err = sync_from(&store(tmp.path()), &SyntheticRemote, &[], true, at())
            .await
            .expect_err("空列表应报错");
        assert_eq!(err.kind(), "invalid_input");
    }

    #[tokio::test]
    async fn synthetic_remote_is_deterministic_per_symbol() {
        let remote = SyntheticRemote;
        let a = remote.fetch("600519").await.expect("拉取");
        let b = remote.fetch("600519").await.expect("拉取");
        assert_eq!(a.price, b.price, "演示行情确定性（timestamp 字段除外）");
        assert_eq!(
            content_seq(a.price, a.volume),
            content_seq(b.price, b.volume),
            "演示口径下指纹恒定：首拉后同步恒报 unchanged"
        );
        assert!(remote.fetch("  ").await.is_err(), "空白标的应报错");
    }

    /// 前端契约：载荷/报告的字段名即 DOM 渲染依据，serde 往返锁定
    #[test]
    fn payload_and_report_field_names_match_frontend_contract() {
        let data = market::synthetic_quote_at("600519", at());
        let payload = QuotesPayload {
            quotes: vec![QuoteView {
                data,
                source: SOURCE_CACHE.to_string(),
            }],
            missing: vec!["000001".to_string()],
            online: false,
        };
        let json = serde_json::to_value(&payload).expect("序列化");
        for key in ["quotes", "missing", "online"] {
            assert!(json.get(key).is_some(), "载荷应含字段 {key}: {json}");
        }
        let quote = &json["quotes"][0];
        for key in ["symbol", "price", "volume", "source"] {
            assert!(
                quote.get(key).is_some(),
                "行情条目应含字段 {key}（flatten 平铺）: {quote}"
            );
        }
        assert_eq!(quote["source"], "cache");

        let report = SyncReport {
            online: true,
            applied: vec!["600519".to_string()],
            unchanged: 2,
            failed: Vec::new(),
            synced_at: at(),
        };
        let report_json = serde_json::to_value(&report).expect("序列化");
        for key in ["online", "applied", "unchanged", "failed", "synced_at"] {
            assert!(report_json.get(key).is_some(), "报告应含字段 {key}");
        }
        let parsed: QuotesPayload = serde_json::from_value(json).expect("载荷往返");
        assert_eq!(parsed, payload);
    }
}
