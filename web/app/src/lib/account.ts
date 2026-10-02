/**
 * 账户与跨端数据同步的客户端面（L476）。
 *
 * 协议与语义对齐 `packages/core/src/account.rs`（服务端权威面在
 * `services/api-gateway/src/account.rs`，端点 `/api/v1/account/*`）：
 * - 同步记录 `{key, rev, updatedAtMs, deleted, payload}`，`deleted` 是
 *   **墓碑位**——删除必须留痕，否则删除传不到其他端，他端把旧值当新值；
 * - 客户端只提交 `baseRev`（上次见到的服务端 rev），**不自增 rev**，
 *   否则两台设备会写出同一个 rev，服务端乐观并发比对直接失效；
 * - 三方合并：基线 = 上次同步的 rev 快照。单侧改动是普通增量，双侧异值
 *   才算冲突，按策略裁决（`newestWins` 并列取服务端——两端同规则才能
 *   算出同一结果，否则互相覆盖永不收敛）。
 *
 * 本模块是纯逻辑面（无 fetch、无 localStorage、无时钟读取）：网络在
 * `hooks/useAccountSync.ts` 装配，持久化在调用方，测试在 node 环境跑。
 */

/** 单条载荷上限（字节；与服务端一致） */
export const MAX_PAYLOAD_BYTES = 64 * 1024

/** 单次请求推送条数上限（超出留队下一轮——弱网下同步包不能无限大） */
export const MAX_PUSHES_PER_REQUEST = 100

/** 认证关闭时的本机账户 id（服务端同一常量） */
export const LOCAL_ACCOUNT_ID = 'local'

/** 同步记录（`key` 形如 `workspace:9f2c`） */
export interface SyncRecord {
  key: string
  rev: number
  updatedAtMs: number
  deleted: boolean
  payload: unknown
}

/** 上行推送项（`baseRev` = 客户端最后见到的服务端 rev，0 = 新建） */
export interface SyncPush {
  key: string
  base_rev: number
  deleted: boolean
  payload: unknown
  updated_at_ms: number
}

/** 拒绝原因（服务端判别式，snake_case 序列化） */
export type RejectionReason =
  | 'conflict'
  | 'invalid_key'
  | 'payload_too_large'
  | 'tombstone_payload_not_null'

/** 拒绝项（`server` 为服务端权威副本，冲突时在位） */
export interface SyncRejection {
  key: string
  reason: RejectionReason
  server?: SyncRecord
}

/** 上行请求 */
export interface SyncRequest {
  cursor: number
  base: Record<string, number>
  pushes: SyncPush[]
}

/** 下行响应 */
export interface SyncResponse {
  cursor: number
  accepted: SyncRecord[]
  rejected: SyncRejection[]
  changes: SyncRecord[]
}

/** 账户档案（服务端响应形状，`rev` 服务端权威） */
export interface AccountProfile {
  account_id: string
  display_name: string
  email?: string
  locale: string
  rev: number
}

/** 冲突裁决策略 */
export type ConflictPolicy = 'newestWins' | 'localWins' | 'remoteWins'

/** 冲突裁决胜方 */
export type ConflictWinner = 'local' | 'remote'

/** 一次冲突裁决的留痕（可展示「这条被覆盖了」） */
export interface SyncConflict {
  key: string
  localRev: number
  remoteRev: number
  winner: ConflictWinner
}

/** 三方合并计划 */
export interface SyncPlan {
  push: SyncRecord[]
  pull: SyncRecord[]
  conflicts: SyncConflict[]
}

/** 一次下行应用的结果 */
export interface SyncOutcome {
  /** 落地的服务端记录 */
  applied: SyncRecord[]
  /** 出队的本地推送键 */
  acked: string[]
  /** 需客户端裁决的冲突 */
  conflicts: SyncRejection[]
}

const KEY_PATTERN = /^([a-z][a-z0-9_-]{0,15}):([A-Za-z0-9._-]{1,48})$/

/** 记录校验（与服务端 `SyncRecord::validate` 同口径，提前在客户端拦） */
export function validateRecord(record: SyncRecord): string | null {
  if (!KEY_PATTERN.test(record.key)) return `同步键非法: ${record.key}`
  if (record.key.length > 64) return `同步键超长: ${record.key}`
  if (!Number.isInteger(record.rev) || record.rev < 1) return `rev 非法: ${record.rev}`
  if (record.deleted) {
    return record.payload === null ? null : '墓碑记录必须携带空载荷'
  }
  const bytes = new TextEncoder().encode(JSON.stringify(record.payload ?? null)).length
  return bytes > MAX_PAYLOAD_BYTES ? `载荷超长（${bytes} > ${MAX_PAYLOAD_BYTES} 字节）` : null
}

/** 内容等价（忽略 rev）：载荷 + 墓碑位一致即两端已收敛到同一份数据 */
export function sameContent(a: SyncRecord, b: SyncRecord): boolean {
  return (
    a.deleted === b.deleted &&
    JSON.stringify(a.payload ?? null) === JSON.stringify(b.payload ?? null)
  )
}

/** 构造存活记录 */
export function liveRecord(
  key: string,
  rev: number,
  updatedAtMs: number,
  payload: unknown,
): SyncRecord {
  return { key, rev, updatedAtMs, deleted: false, payload }
}

/** 构造墓碑（载荷恒为 null） */
export function tombstone(key: string, rev: number, updatedAtMs: number): SyncRecord {
  return { key, rev, updatedAtMs, deleted: true, payload: null }
}

/**
 * 三方合并：本地 vs 服务端，基线为上次同步后各键的 rev 快照。
 *
 * 缺键 = 「无意见」而非删除（删除只走墓碑）；双侧同改同值按 rev 高者
 * 下发推进基线；双侧异值才裁冲突，`newestWins` 时间戳并列取服务端。
 */
export function planSync(
  local: Record<string, SyncRecord>,
  remote: Record<string, SyncRecord>,
  base: Record<string, number>,
  policy: ConflictPolicy = 'newestWins',
): SyncPlan {
  const plan: SyncPlan = { push: [], pull: [], conflicts: [] }
  const keys = new Set([...Object.keys(local), ...Object.keys(remote), ...Object.keys(base)])

  for (const key of [...keys].sort()) {
    const baseRev = base[key] ?? 0
    const l = local[key]
    const r = remote[key]
    const localChanged = l !== undefined && l.rev > baseRev
    const remoteChanged = r !== undefined && r.rev > baseRev

    if (l && r) {
      if (localChanged && !remoteChanged) {
        plan.push.push(l)
      } else if (!localChanged && remoteChanged) {
        plan.pull.push(r)
      } else if (sameContent(l, r)) {
        plan.pull.push(l.rev >= r.rev ? l : r)
      } else {
        const localWins =
          policy === 'localWins' ||
          (policy === 'newestWins' && l.updatedAtMs > r.updatedAtMs)
        plan.conflicts.push({
          key,
          localRev: l.rev,
          remoteRev: r.rev,
          winner: localWins ? 'local' : 'remote',
        })
        if (localWins) plan.push.push(l)
        else plan.pull.push(r)
      }
    } else if (l) {
      if (localChanged) plan.push.push(l)
    } else if (r) {
      if (remoteChanged) plan.pull.push(r)
    }
  }
  return plan
}

/** 客户端同步状态：发件箱 + 水位 + 基线（纯归约器） */
export interface SyncState {
  cursor: number
  base: Record<string, number>
  outbox: Record<string, SyncPush>
}

export function newSyncState(): SyncState {
  return { cursor: 0, base: {}, outbox: {} }
}

/** 本地改动入队（同键后写覆盖——连续编辑不堆条目） */
export function noteLocalChange(
  state: SyncState,
  key: string,
  payload: unknown,
  nowMs: number,
): SyncState {
  return {
    ...state,
    outbox: {
      ...state.outbox,
      [key]: {
        key,
        base_rev: state.base[key] ?? 0,
        deleted: false,
        payload,
        updated_at_ms: nowMs,
      },
    },
  }
}

/** 本地删除入队（墓碑：删除必须传播） */
export function noteLocalDelete(
  state: SyncState,
  key: string,
  nowMs: number,
): SyncState {
  return {
    ...state,
    outbox: {
      ...state.outbox,
      [key]: {
        key,
        base_rev: state.base[key] ?? 0,
        deleted: true,
        payload: null,
        updated_at_ms: nowMs,
      },
    },
  }
}

/** 待推送项（按键序稳定，条数受 MAX_PUSHES_PER_REQUEST 限制） */
export function pendingPushes(state: SyncState): SyncPush[] {
  return Object.keys(state.outbox)
    .sort()
    .slice(0, MAX_PUSHES_PER_REQUEST)
    .map((key) => state.outbox[key]!)
}

/** 构造上行请求 */
export function buildRequest(state: SyncState): SyncRequest {
  return { cursor: state.cursor, base: { ...state.base }, pushes: pendingPushes(state) }
}

/**
 * 应用下行响应：接受项出队并推进基线、增量落基线、拒绝项转为冲突。
 *
 * 水位单调前进——**回退水位视为协议破坏直接忽略**（服务端重启可能从低
 * 水位重发，旧增量不得被当新数据应用）。
 */
export function applyResponse(state: SyncState, response: SyncResponse): SyncOutcome {
  const outcome: SyncOutcome = { applied: [], acked: [], conflicts: [...response.rejected] }
  const base = { ...state.base }
  const outbox = { ...state.outbox }

  for (const record of response.accepted) {
    base[record.key] = record.rev
    delete outbox[record.key]
    outcome.acked.push(record.key)
    outcome.applied.push(record)
  }
  const cursorRegressed = response.cursor < state.cursor
  for (const record of response.changes) {
    base[record.key] = record.rev
    if (!cursorRegressed) outcome.applied.push(record)
  }
  return {
    outcome,
    state: {
      cursor: cursorRegressed ? state.cursor : response.cursor,
      base,
      outbox,
    },
  }
}

/** 本地视图应用（墓碑 → 从视图移除；其余 → 写入） */
export function applyToView(
  view: Record<string, SyncRecord>,
  applied: SyncRecord[],
): Record<string, SyncRecord> {
  const next = { ...view }
  for (const record of applied) {
    if (record.deleted) delete next[record.key]
    else next[record.key] = record
  }
  return next
}

/** 响应解析（服务端字段是 snake_case；非法形状 → null 不抛） */
export function parseSyncResponse(raw: string): SyncResponse | null {
  let msg: unknown
  try {
    msg = JSON.parse(raw)
  } catch {
    return null
  }
  if (typeof msg !== 'object' || msg === null) return null
  const m = msg as Record<string, unknown>
  const isRecord = (v: unknown): v is SyncRecord =>
    typeof v === 'object' &&
    v !== null &&
    typeof (v as SyncRecord).key === 'string' &&
    Number.isInteger((v as SyncRecord).rev)
  if (!Number.isInteger(m.cursor)) return null
  return {
    cursor: m.cursor as number,
    accepted: Array.isArray(m.accepted) ? m.accepted.filter(isRecord) : [],
    rejected: Array.isArray(m.rejected)
      ? (m.rejected as SyncRejection[]).filter(
          (r) => typeof r === 'object' && r !== null && typeof r.key === 'string',
        )
      : [],
    changes: Array.isArray(m.changes) ? m.changes.filter(isRecord) : [],
  }
}

/** 档案响应解析（缺 display_name/locale 视为非法——档案面没有降级语义） */
export function parseProfile(raw: string): AccountProfile | null {
  try {
    const m = JSON.parse(raw) as Partial<AccountProfile>
    if (
      typeof m?.account_id !== 'string' ||
      typeof m.display_name !== 'string' ||
      typeof m.locale !== 'string'
    ) {
      return null
    }
    return m as AccountProfile
  } catch {
    return null
  }
}

/** 档案更新载荷（缺省字段不改；空邮箱串 = 清除） */
export function buildProfilePatch(patch: {
  displayName?: string
  email?: string
  locale?: string
}): Record<string, string> {
  const body: Record<string, string> = {}
  if (patch.displayName !== undefined) body.display_name = patch.displayName
  if (patch.email !== undefined) body.email = patch.email
  if (patch.locale !== undefined) body.locale = patch.locale
  return body
}

/**
 * 同步端点基址：默认同主机网关（dev `?gw=` 常指 vite 端口，故 dev 下走
 * 显式网关地址；生产同源无 port 覆写时走 8080）。`?gw=` 传的是**基址**
 * 前缀（到 `/api/v1/account` 为止），不再追加路径段——否则覆盖值会被
 * 二次拼接出重复路径。
 */
export function accountApiBase(loc: { hostname: string; port?: string; search?: string }): string {
  const override = new URLSearchParams(loc.search ?? '').get('gw')
  if (override) return override.replace(/\/$/, '')
  const origin = loc.port ? `http://${loc.hostname}:${loc.port}` : `http://${loc.hostname}:8080`
  return `${origin}/api/v1/account`
}