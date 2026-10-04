# 统一账户与跨端数据同步（L476）

领域语义唯一事实源 = `packages/core/src/account.rs`（纯函数）；
服务端权威面 = `services/api-gateway/src/account.rs`（端点 `/api/v1/account/*`）；
客户端面 = `web/app/src/lib/account.ts`。本文说明协议、端点、裁决规则
与已知边界。

## 1. 三层落点

| 层 | 落点 | 职责 |
|---|---|---|
| 领域（Rust） | `packages/core/src/account.rs` | 记录/档案类型、键与载荷校验、三方合并 `plan_sync`、客户端归约器 `SyncState`、wire 形状 |
| 服务端（Rust） | `services/api-gateway/src/account.rs` | 每账户权威状态、rev/水位分配、乐观并发接受与冲突回执、增量下发、内存/Postgres 双形态 |
| 客户端（TS） | `web/app/src/lib/account.ts` | 同协议镜像 + 同语义三方合并 + 发件箱归约 + 响应解析（纯逻辑，网络与持久化在调用方） |

## 2. 协议

### 2.1 同步记录

```json
{ "key": "workspace:9f2c", "rev": 3, "updated_at_ms": 1760000000000,
  "deleted": false, "payload": { "name": "盯盘", "symbols": ["600519"] } }
```

- `key` = `namespace:local_id`，命名空间小写起 ≤16 字符，键总长 ≤64；
- `rev` **服务端权威**（单调递增，从 1 起）。客户端不自增 rev——只提交
  `base_rev`（上次见到的服务端 rev）。自增会让两台离线设备写出同一个
  rev，服务端乐观并发比对直接失效；
- `deleted` 是墓碑位：删除必须留痕，否则删除传不到其他端，其他端会把
  旧值当新值继续用。墓碑 `payload` 恒为 `null`（顺带封住墓碑夹带数据）；
- `payload` 上限 64 KiB（同步包要在移动端弱网下往返）。

### 2.2 上行 `POST /api/v1/account/sync`

```json
{ "cursor": 42,
  "base": { "workspace:9f2c": 3 },
  "pushes": [ { "key": "workspace:9f2c", "base_rev": 3,
                "deleted": false, "payload": {...}, "updated_at_ms": ... } ] }
```

服务端接受条件：**校验通过** 且（该键不存在 或 `base_rev` == 当前权威
`rev`）。不符则回 `rejected`：

```json
{ "key": "...", "reason": "conflict", "server": { ...权威副本... } }
```

`reason` ∈ `conflict` / `invalid_key` / `payload_too_large` /
`tombstone_payload_not_null`（后三者是请求格式问题，`server` 为空）。
单请求推送条数上限 100（客户端已分片，服务端再兜一层——单请求放大是现成
的拒绝服务面）。

### 2.3 下行

```json
{ "cursor": 43,
  "accepted": [ ...带服务端权威 rev 的记录... ],
  "rejected": [ ... ],
  "changes": [ ...cursor 之后被动过的记录（含墓碑）... ] }
```

`changes` 是按 `cursor` 的**增量**（账户内单调水位 `touched_seq` 过滤，
记录级水位不逐条下发——账户级数据量小，全量增量更省事，见 §6）。
接受项同时出现在 `changes` 里是幂等的：客户端据 `accepted` 更新基线，
据 `changes` 更新视图。

### 2.4 档案 `GET/PUT /api/v1/account/profile`

```json
{ "account_id": "alice", "display_name": "Alice", "email": "...", "locale": "zh", "rev": 2 }
```

`PUT` 是字段级部分更新：缺省字段不改，**空邮箱串 = 清除**。校验失败回
400（显示名非空且 ≤64 字符、邮箱形状、locale 为 BCP 47 形状）。服务端
`sub` 为空时落 `local`（本机缺省账户）。

### 2.5 删除 `DELETE /api/v1/account`

整账户清除（档案 + 同步记录 + 同步水位），data-privacy §4（GDPR Art.17 /
CCPA 删除权）的服务端落实面。语义：

- **先删持久层、后删内存**：后端删除失败时内存态保持原样并回 500——
  绝不出现「声称已删而快照还在持久层、懒加载一碰就复活」的半删状态；
- **幂等**：重复删或删无数据的账户都回 `204`（无数据可删不是错误）；
- **审计留痕**：删除是状态变更，记 `AccountDataDeleted` 审计事件
  （`alpha_gateway_audit_total{event="account_data_deleted"}`）；
- 删后同账户再访问即复建缺省档案（`rev` 归 1），旧 `rev` 不复用。

## 3. 账户身份与隔离

- 认证关闭（`--auth-mode off`，默认）→ 全部请求归 `local`，单账户本机形态；
- 认证开启 → 身份取 auth 中间件**已校验过**的 `Claims`（经请求扩展下传，
  处理器不二次验签——再验一次只会多一处失败分支）；
- 存储键 = `normalize_account_id(sub)` 的 slug：小写 + 非安全字符折叠 +
  **折叠后追加原串摘要尾巴**。「a l i c e」与「a-l-i-c-e」折叠成同一 slug，
  不加尾巴就撞键——撞键等于两个账户互相看见数据。

## 4. 冲突裁决

三方合并（基线 = 上次同步的 rev 快照）：

| 本地 | 服务端 | 判定 |
|---|---|---|
| 未动 | 未动 | 无传输 |
| 动过 | 未动 | 普通增量：上行 |
| 未动 | 动过 | 普通增量：下发 |
| 动过 | 动过，内容一致 | 并发同改同值：下发 rev 高者推进基线，**不记冲突** |
| 动过 | 动过，内容不同 | 冲突，按策略裁决 |

`ConflictPolicy`：`newestWins`（默认，比 `updated_at_ms`，**并列取服务端**）、
`localWins` / `remoteWins`（用户显式偏好）。并列取服务端不是随便定的——
两端跑同一套规则才可能算出**同一裁决**，否则 A 覆盖 B、B 又覆盖 A，永不
收敛。`updated_at_ms` 服务端以自己的时钟落（客户端时钟不可信），故该值
始终单调可比。

缺键 = 「无意见」而非删除：某端没有该键时其 rev 视作等于基线。删除只能
走墓碑，否则「服务端删了但本地没同步到」与「本地主动删了」无法区分。

## 5. 客户端归约器

`SyncState { cursor, base, outbox }`（Rust 与 TS 双实现，语义逐条对齐并各
有测试锁定）：

- `noteLocalChange` / `noteLocalDelete` 入队，同键后写覆盖前写（连续编辑
  不堆条目）；
- `pendingPushes` 按 100 条分片，超出留队下一轮；
- `applyResponse`：接受项出队 + 推基线、增量落基线、拒绝项转冲突留队；
  **水位回退直接忽略**（服务端重启可能从低水位重发，旧增量不得被当新
  数据应用）。

## 6. 已知边界（诚实登记）

- **持久化形态**：默认纯内存（零外部依赖，单机形态即可跑）。配
  `ALPHA_GATEWAY_ACCOUNT_STORE_URL` 时经 alpha-storage 的 Postgres KV 表
  写穿账户快照（整账户一个键），**连接失败即退出**——静默降级到内存会让
  多副本部署各持一份互相看不见的账户数据，比启动失败更难排查。写失败
  只告警不拒绝请求（内存是权威 serving 层，与 data-engine 的 Timescale
  镜像口径一致）。快照粒度是整账户，非逐记录行——账户级数据量下够用，
  量大后改逐记录表（登记项）；
- **多副本并发**：写穿是「读时整份快照覆盖」，两个副本同时改同一账户会
  后写覆盖先写。单副本/主备形态正确；真多副本需行级 CAS 或数据库侧事务
  （登记项）；
- **锁与 await 的边界**：服务端 `std::sync::Mutex` 只护内存结构（临界区
  纯计算），持久化的 `retrieve`/`store` 一律锁外 `.await`——把网络往返
  放进临界区会让一个慢查询把所有账户请求一起堵住，`block_in_place` 在
  current_thread 运行时上还会直接 panic。代价是写穿为「快照后覆盖写」：
  并发改动时后到者覆盖先到者（内存已收敛，快照不会撕裂）；
- **冲突不做自动 UI 裁决**：服务端只回权威副本与原因，客户端怎么选
  （覆盖/保留/另存）是产品决策，当前只有纯函数裁决面（`planSync` /
  `plan_sync`）可调，UI 接线登记后续；
- **同步触发时机**：当前只有纯逻辑面与端点，没有后台自动同步器
  （Web SW / 移动前台任务），登记后续；
- **加密**：传输与静态加密归 L485（对端在做），账户快照落库形态随之
  调整；
- **移动端客户端**：Android 侧纯逻辑面已交付（`AccountSync.kt`：三方合并
  + 发件箱 + wire 编解码 + 状态持久化，15 用例）；iOS 目录属 L119 交付面
  不动，故 iOS 侧同步登记后续。

## 7. 相邻项

- L483/L484：JWT + RBAC（身份与授权的前置，本项复用其 `Claims`）
- L488/L520：GDPR/CCPA 权利面（本地导出/清除见 data-privacy §2；同步上去
  的服务端数据删除走 §2.5 的 `DELETE /api/v1/account`，两侧已闭环）
- L504：API key 门（第三方接入面；账户面走 JWT 而非 API key，两者并存）
- L485：端到端加密