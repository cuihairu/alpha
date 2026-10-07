# 身份认证

口径：**工程项**——网关 JWT 强制校验 + bootstrap 签发端点 +
OIDC JWKS 接线（拉取/定时刷新/分流，oct 键联调形态，见 §4）。

## 1. 模式

`--auth-mode off|jwt`（env `ALPHA_GATEWAY_AUTH_MODE` 优先，默认 off；
解析另接受 `required`/`on` 作为 `jwt` 别名）：

- `off`：零行为变化，所有路由直通（既有部署/测试不受影响）。
- `jwt`：`/api/*` 强制 `Authorization: Bearer <jwt>` 校验；
  `/ws*` 升级握手同样强制持票（见 §3.1）；`/health`、`/metrics`、
  `/auth/token` 公开（探活/指标/签发不能要求先登录）。

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

### 3.1 WS 握手鉴权（jwt 模式）

`/ws` 与 `/ws/<path>` 升级请求在拨上游之前校验 token，未认证流量不触发
real-time-feed 连接：

- 取票顺序：`Authorization: Bearer` 头优先，回落 `?token=` 查询参数
  （浏览器 WebSocket 无法自定义握手头；JWS compact 全部是未保留字符，
  查询参数无需 percent 解码）；
- 校验与 REST 同一分流核（`verify_any`，kid→OIDC / 无 kid→自签，见 §4），
  401 同体同 `WWW-Authenticate`；
  WS 是只读订阅面，任意已认证角色放行（与 REST 读路径同口径，不走
  authorize）；
- off 模式零行为变化；指标 `alpha_gateway_auth_total{mode="ws_allowed"|
  "ws_denied"}` 与 REST 面同源可告警；
- 校验失败先于上游连接——上游不可达时 jwt 模式仍返回 401 而非 502。

边界（如实登记）：本节鉴权只作用于**网关 `/ws` 面**。直连 real-time-feed
的路径——web 默认 `ws://<host>:8082/ws`、生产 nginx `/ws/` → feed 9081——
不经网关，不受此校验；feed 侧自身鉴权归 WS hardening 后续项（web 客户端
经网关走 JWT 的接线同批归前端增量）。

## 4. OIDC 路径（L502 接线：JWKS 拉取 + 定时刷新）

- 校验核 `auth.rs::verify_oidc_token`：按 token 头 `kid` 选键 →
  验签名 → 验 iss/aud（为空即跳过，开发联调不断言）。
- 键表拉取 `--auth-jwks-url`（env `ALPHA_GATEWAY_AUTH_JWKS_URL`；
  jwt 模式下启用）：URL 直接指向 JWKS 文档，OIDC discovery 文档解析
  （`/.well-known/openid-configuration` → `jwks_uri`）归生产硬化项。
  只支持 `oct` 对称键（`k` 按 RFC 7517 base64url 解码）；RSA/EC 的
  x5c 链验证归生产硬化项。
- 启动 fail-fast：初始拉取失败或键表无 oct 键 → 拒绝启动（空表起来
  等于认证面全拒，属配置错误不是运行态）。
- 定时刷新 `--auth-jwks-refresh-secs`（默认 600，0 = 仅启动取一次）：
  失败/空文档保旧键继续服务并 warn——IdP 轮换期验签不中断；认证面
  fail-closed 语义不受影响（无键票据照旧 401）。
- 分流规则 `verify_any`：JWKS 在位且票带 `kid` → OIDC 核（unknown kid
  直接拒绝，不回退自签，防降级混淆）；无 `kid` 票（bootstrap 自签）
  仍走 `--auth-secret` 自签核——IdP 票据与 bootstrap 并存；未配 JWKS
  的既有部署零行为变化。REST 与 WS 握手（§3.1）同一分流核。
- 边界（如实登记）：oct 是「网关校验自家/测试 IdP 签发」的联调形态，
  主流 IdP（Keycloak/Auth0）默认签 RSA——接真实 IdP 需先落 x5c 链
  验证（硬化项），本节接线不掩盖该缺口。

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
3. WS 握手鉴权已接线（§3.1）：jwt 模式下 `/ws*` 升级需持票（头或
   `?token=`）；升级后的帧级鉴权（订阅粒度控制）未做——连接内任意
   Subscribe 均放行，通道级 ACL 归 WS hardening 后续项。
4. 刷新令牌：自签路径客户端用 provision_key 重新签发（bootstrap 语义）；
   生产走 IdP refresh_token 轮换。
