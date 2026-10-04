# 身份认证

口径：**工程项**——网关 JWT 强制校验 + bootstrap 签发端点 +
OIDC 校验核（库函数级，IdP 接线归下一增量）。

## 1. 模式

`--auth-mode off|jwt`（env `ALPHA_GATEWAY_AUTH_MODE` 优先，默认 off；
解析另接受 `required`/`on` 作为 `jwt` 别名）：

- `off`：零行为变化，所有路由直通（既有部署/测试不受影响）。
- `jwt`：`/api/*` 强制 `Authorization: Bearer <jwt>` 校验；
  `/health`、`/metrics`、`/ws*`、`/auth/token` 公开（探活/指标/
  长连接升级/签发不能要求先登录）。

启动 fail-fast：`jwt` + 空 secret（`--auth-secret` /
`ALPHA_GATEWAY_AUTH_SECRET`）直接拒绝启动——与限流 fail-fast 同口径，
不带病上线。

## 2. 失败语义（与限流方向相反）

| 组件坏了 | 行为 | 理由 |
|---|---|---|
| 限流 Redis 不可用 | fail-open 放行 + warn | 限流是优化，不拖垮主链路 |
| 认证缺头/坏签/过期 | fail-closed 401 | 认不出来是谁绝不能放 |

401 一律 `{success:false, error:"unauthorized"}` + `WWW-Authenticate:
Bearer`，不区分过期/伪造/缺头（防用户枚举）。

## 3. 自签路径（本单落地）

- 签发：`POST /auth/token` + `X-Provision-Key` 头（bootstrap 口令，
  env `ALPHA_GATEWAY_AUTH_PROVISION_KEY`；为空 → 503 关闭，默认关闭）。
  请求 `{sub, scope?, ttl_secs?}`（ttl 上限 24h），返回标准
  `{access_token, token_type:"Bearer", expires_in}`。
- 校验：HS256，`exp` 必需，leeway 0（签发校验同机同钟，无漂移可容）。
- 指标：`alpha_gateway_auth_total{mode="allowed"|"denied"}`（限流计数
  同模式，可直接复用其告警写法）。
- 中间件顺序：auth 后注册 → 先执行，未鉴权请求不消耗限流配额。

## 4. OIDC 路径（本单：校验核；接线：下一增量）

`auth.rs::verify_oidc_token`（单测覆盖）：按 token 头 `kid` 选键 →
验签名 → 验 iss/aud（为空即跳过，开发联调不断言）。`fetch_jwks`
支持 `oct` 对称键；RSA/EC 的 x5c 链验证 + `--auth-jwks-url` 定时刷新
归下一增量（需后台刷新任务，本单不引入）。

完整 OAuth 2.0 授权码流程的浏览器侧（登录页/回调/刷新令牌轮换）
归前端 + IdP（Keycloak/Auth0），网关只做资源侧校验——网关不存会话、
不签发刷新令牌。

## 5. RBAC（L484）

角色三档：`viewer`（空 roles 旧票据亦然——只读 GET/HEAD/OPTIONS）、
`operator`（写方法 POST/PUT/DELETE）、`admin`（全通）。

- 判定 `authorize(claims, method, path)` 纯函数（单测矩阵锁定）；
  401 管“你是谁”，403（`{error:"forbidden: insufficient role"}`）管
  “你能干什么”——客户端据此区分重登还是找管理员加角色。
- `/auth/token` 接受 `roles: [...]` 原样写入票据：provision_key 即
  root bootstrap，能调签发口令就能授任意角色——该口令保管等级必须
  高于所授最高角色（生产由 IdP 的用户目录替代此口令）。
- `path` 维度预留：当前 /api 下无写危险端点，方法级已满足最小权限；
  逐端点矩阵（如 /query 限 analyst）待出现第一个危险写端点时再立。
- 指标 `auth_total{mode="forbidden"}` 与 allowed/denied 同源，
  可直接告警“403 突增 = 越权探测”。

## 6. 非交互假设

1. 生产 secret 经 env/secret 卷注入，绝不进 repo（compose 占位符为空）。
2. scope 本单只透传不断言；细粒度 RBAC 归 L484。
3. WS 长连接本单不鉴权（握手时 token 校验归 L484 或 WS 网关 hardening 项）。
4. 刷新令牌：自签路径客户端用 provision_key 重新签发（bootstrap 语义）；
   生产走 IdP refresh_token 轮换。
