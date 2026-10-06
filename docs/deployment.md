---
sidebar_position: 4
---

# 部署指南

本页是运维部署总览，具体步骤各有专属文档（不重复写两遍）。

| 场景 | 看 |
|---|---|
| Ubuntu 单机全栈 | `docs/deployment-runbook.md` + `scripts/deploy-ubuntu.sh` |
| 容器化 | `docs/docker-deployment.md` |
| CDN + 静态资源 | `docs/web-cdn.md`（优化器 → nginx 源站 → S3 发布） |
| 发布流水线 | `.github/workflows/release.yml`（版本门禁 + Web/Desktop×3/Android/iOS/Docker/updater feed 七作业，密钥缺失自动降级） |
| 可观测性 | `docs/alerting-and-diagnosis.md`（Prometheus 规则 + Alertmanager） |
| 合规基线 | `docs/platform-compliance.md` + `scripts/check-compliance.sh` |

## 最小生产拓扑

```
CDN（web dist） ──→ real-time-feed :8082 (/ws)
                 ──→ data-engine（行情 REST + CSV/parquet 导出）
                 ──→ api-gateway（鉴权/限流/护栏/审计，见 docs/auth.md）
Redis Streams（行情总线） + ClickHouse（列存，见 docs/data-lake-parquet.md）
```

生产检查清单：TLS（缺省反代终结；网关服务内 rustls 已接线，`--tls-cert/--tls-key` 双钥启用，见 `docs/deployment-runbook.md`）、密钥全部走环境/secret（本仓零密钥）、
`scripts/check-compliance.sh` 对账通过、告警规则已接入 Alertmanager。
