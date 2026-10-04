# 市场数据 API 与第三方集成

data-engine（`:8081`）承载行情数据 REST 面；第三方两条接入路径：
**经网关**（JWT，`/api/v1/*` 反代，见 `docs/auth.md`）与**直连 API key**
（本篇，配置化开关）。gRPC 面（`:50051`）归内部消费链。

## 1. 端点一览

| 方法 | 路径 | 说明 |
|---|---|---|
| POST | `/query` | DataFusion SQL（market_data 表） |
| GET | `/stocks/:symbol/history` | 历史序列 JSON（`?days=&limit=`，limit 保留最新窗口） |
| GET | `/stocks/:symbol/history.csv` | 同源 CSV 导出（第三方表格/ETL 消费） |
| GET | `/stocks/:symbol/indicators` | 指标快照（RSI/SMA/MACD…参数可选） |
| POST | `/indicators/calculate` | 指标计算（显式参数集） |
| POST | `/analytics/performance` | 绩效归因（收益/波动/回撤） |
| GET | `/instruments` | Instrument 目录查询（`?exchange=&type=&symbol=&q=` 组合过滤） |
| GET | `/instruments/:id` | 单 Instrument 精确查（`cn.sse.000001` 全局键） |
| GET | `/clickhouse/exports` | Parquet 导出清单 |
| GET | `/clickhouse/export.parquet` | Parquet 导出下载 |
| GET | `/clickhouse/market-data.parquet` | 同上（back-compat 别名，保留既有链接） |
| GET | `/health`、`/metrics` | 运维面（不受 API key 门管控） |

## 2. 第三方直连鉴权：API key

```yaml
# 配置文件（可选源，仓内 services/data-engine/config 不存在，相对进程 CWD）：
#   security:
#     api_keys: ["third-party-key-a", "third-party-key-b"]
# 在库下发走 env：ALPHA__SECURITY__API_KEYS=key-a,key-b（ALPHA 前缀 + __ 分隔）
```

- **空表 = 关闭**（内网默认形态，历史行为不变）；非空后数据面一律要求
  `X-Api-Key: <key>` 头命中其一，否则 `401 {"success":false,"error":...}`；
- 比较走常时形状（逐字节异或走满 + 多 key 全量判定不提前返回），
  命中判定不泄漏「第几个 key 正确」；key 长度不属秘密面（HTTP 头可观测）；
- key 经配置/env 下发，**不入库不进日志**；轮换 = 追加新 key → 通知
  消费方切换 → 移除旧 key（多 key 并存天然支持灰度）。

```bash
curl -H "X-Api-Key: third-party-key-a" \
  "http://<engine>:8081/stocks/600519/history.csv?days=30&limit=250"
```

CSV 面：`timestamp,price,volume` 表头 + RFC3339 行；三列均无逗号类型，
无引号转义；缺 volume 补 0；空数据只回表头。富字段（bid/ask/metadata）
以 JSON `/query` 与 parquet 导出为准。

## 3. 内部链路（经网关）

`web/desktop → :8080 /api/v1/*`（JWT）→ api-gateway 反代 → data-engine
同名路径。第三方若走此路径按 `docs/auth.md` 取票。两路径共享同一
data-engine 面，限流/TLS 归 L486 / L485。

## 4. 边界登记

- **配额/限流**：API key 门只做身份面，不做按 key 配额（L486 范畴）；
- **TLS**：直连路径默认明文（内网/反代终止 TLS 部署形态），服务端
  TLS 归 L485；
- **gRPC 第三方面**：当前内部契约，未纳入对第三方承诺面。
