# 实时数据同步协议设计要点（WebSocket 增量更新 + 版本控制）

> 对应 TODO「实现实时数据同步协议（WebSocket 增量更新 + 版本控制）」最小可用版本。
> 分层：协议核与内存态版本表在 L0 纯计算层（`packages/core/src/sync.rs`），
> 线上帧定义在 `packages/protocols/src/websocket.rs`，服务端发布/重同步在
> `services/real-time-feed`，浏览器侧绑定在 `wasm-analyzer`（wasm 薄绑定 +
> JS 主线程传输）。本文记录设计要点与已知边界，非部署/接线文档。

## 1. 帧模型与版本契约

服务端 → 客户端同步帧 `SyncMessage { channel, seq, op, data, timestamp }`，
`op ∈ { Full, Delta }`；客户端 → 服务端 `ResyncRequest { channel, from_seq }`。

* **单调版本号**：`seq` 在**通道内**单调递增（服务端发布路径在锁内统一分配，
  全连接共享同一序列）。客户端记录 `last_seq`，校验 `seq == last_seq + 1`。
* **Full**：携带通道权威全量快照。首帧（连接基线）与 Resync 响应必为 Full。
  连续性校验对 Full 不生效——Resync 回的 Full 可能直接跳到最新 seq，
  照搬 Delta 校验会永远 Gap（基线无法恢复）；除旧帧幂等忽略外一律接受。
* **Delta**：只携带相对上一帧的差异字段。必须严格连续（`seq == last_seq + 1`）。
* **Gap**：`seq > last_seq + 1` 即丢帧，状态机返回
  `Gap { expected, got }` 且**不推进任何状态**；客户端据此发 Resync
  （携带 `from_seq = resync_from()`，即已连续应用的版本）。
* **幂等**：`seq <= last_seq` 的重放/乱序旧帧直接忽略（`Idempotent`），
  快照与版本不受污染。
* **协议违约**：Delta 到达时通道尚无本地快照 → 报错而非静默（客户端应先收
  Full 建基线）。

## 2. 增量（差量）计算

`build_delta(prev, next)`：同为 object 时取深度 1 字段差集（新增或值变化，
值整体替换）；任一非 object 则整体取 `next`。`apply_delta(base, delta)`：
同为 object 时逐字段覆盖合并，否则整体取 `delta`。二者互为逆操作
（`base ⊕ build_delta(base, next) == next`，单测锁定）。

服务端发布与客户端合入复用**同一份实现**（`alpha_core::sync`），
real-time-feed 广播路径与 wasm 侧绑定均调用它，两端协议不漂移。

已知最小口径边界（刻意简化，扩展留后续立项）：不做嵌套（深度 >1）递归 diff、
不做数组 diff、**不表达字段删除**（深度 1 差集无法编码删除，若需删除语义
需 tombstone 或路径寻址补丁格式）。

## 3. 客户端追平与内存态版本表

`SyncHistory { retention }`：服务端发布侧逐通道环形保留最近 `retention` 帧
（seq 升序，超窗淘汰最旧）。客户端重连时按已连续应用到的 `last_seq` 调
`catch_up(channel, last_seq)`：

* `CatchUp::Frames`：保留帧完整覆盖 `last_seq+1..=latest`（窗口首帧
  `seq <= last_seq+1` 且帧间无空洞）→ 返回缺失帧按序重放即拉齐；
  已追平返回空序列。
* `CatchUp::Resync { latest_seq }`：落后超出保留窗口或通道无历史 →
  服务端以当前最新状态回 Full 快照（携带 `latest_seq`；通道未知/无历史时为 `None`）。

关键不变量：**窗口边缘追平要求 `last_seq` 恰好落在窗口首帧前一版**——
重放的 Delta 依赖其前置状态，缺前置必须走 Full（回环集成测试
`publish_catch_up_round_trip_converges` 同时锁定两条路径与收敛性）。

## 4. 服务端应答语义（real-time-feed）

* 发布：`next_versioned_frame` 在锁内 `seq += 1`，首帧 Full、后续 Delta
  （`build_delta` 基于最近快照），快照与版本一并更新。
* Resync：已知通道回当前版本 Full 快照；**未知通道/尚无快照必须显式回
  Error 帧（code 404）**——静默丢弃会让客户端无从判断结果、只能空等重传。

## 5. 测试地图

| 契约 | 锁定位置 |
| --- | --- |
| 版本推进/Gap/幂等/违约/多通道隔离/Resync 恢复 | `alpha_core::sync` 单测（16） |
| 追平重放/超窗 Resync/未知通道/回环收敛 | `alpha_core::sync` `catch_up_*` + `publish_catch_up_round_trip_converges` |
| 线上帧 JSON 形态（type tag/op/round-trip/旧帧兼容） | `alpha_protocols::websocket` 单测（4） |
| 服务端 Full→Delta 序列/逐通道 seq/Resync 应答/服务端 Delta × 客户端引擎对账 | `alpha-real-time-feed` 单测（含 `test_server_delta_applies_on_client_engine`） |
| JS 边界（buildSyncDelta/applySyncDelta/WasmSyncEngine） | `wasm-analyzer` `#[wasm_bindgen_test]`（浏览器跑，门禁编译验证） |
