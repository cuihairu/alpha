import { describe, expect, it } from 'vitest'
import {
  accountApiBase,
  applyResponse,
  applyToView,
  buildProfilePatch,
  buildRequest,
  liveRecord,
  LOCAL_ACCOUNT_ID,
  newSyncState,
  noteLocalChange,
  noteLocalDelete,
  parseProfile,
  parseSyncResponse,
  pendingPushes,
  planSync,
  sameContent,
  tombstone,
  validateRecord,
  type SyncRecord,
  type SyncState,
} from './account'

/** 契约锚点：`packages/core/src/account.rs` + 网关 `/api/v1/account/*` */

const rec = (
  key: string,
  rev: number,
  payload: unknown,
  updatedAtMs = 100,
): SyncRecord => liveRecord(key, rev, updatedAtMs, payload)

describe('validateRecord（与服务端同口径）', () => {
  it('合法记录通过；键形状/墓碑载荷/rev 各自拦截', () => {
    expect(validateRecord(rec('workspace:a', 1, { n: 1 }))).toBeNull()
    expect(validateRecord(tombstone('workspace:a', 2, 100))).toBeNull()
    expect(validateRecord(rec('nope', 1, {}))).toMatch(/键非法/)
    expect(validateRecord(rec('Workspace:a', 1, {}))).toMatch(/键非法/)
    expect(validateRecord({ ...tombstone('workspace:a', 2, 100), payload: { x: 1 } })).toMatch(
      /墓碑/,
    )
    expect(validateRecord(rec('workspace:a', 0, {}))).toMatch(/rev/)
  })
  it('超大载荷在客户端就拦（与服务端 64 KiB 上限一致）', () => {
    const big = rec('workspace:a', 1, { blob: 'x'.repeat(64 * 1024) })
    expect(validateRecord(big)).toMatch(/载荷超长/)
  })
})

describe('planSync（三方合并）', () => {
  it('单侧改动是普通增量不是冲突', () => {
    const pushPlan = planSync(
      { 'workspace:a': rec('workspace:a', 1, { n: 1 }) },
      {},
      {},
    )
    expect(pushPlan.push).toHaveLength(1)
    expect(pushPlan.conflicts).toHaveLength(0)

    const pullPlan = planSync({}, { 'workspace:a': rec('workspace:a', 2, { n: 2 }) }, {
      'workspace:a': 1,
    })
    expect(pullPlan.pull).toHaveLength(1)
    expect(pullPlan.conflicts).toHaveLength(0)
  })

  it('并发同改同值：取 rev 高者推进基线，不记冲突', () => {
    const plan = planSync(
      { 'workspace:a': rec('workspace:a', 2, { n: 7 }) },
      { 'workspace:a': rec('workspace:a', 3, { n: 7 }) },
      { 'workspace:a': 1 },
    )
    expect(plan.push).toHaveLength(0)
    expect(plan.pull[0]!.rev).toBe(3)
    expect(plan.conflicts).toHaveLength(0)
  })

  it('双侧异值裁冲突：newestWins 按时间戳，并列取服务端（两端收敛前提）', () => {
    const local = { 'workspace:a': rec('workspace:a', 2, { n: 'L' }, 300) }
    const remote = { 'workspace:a': rec('workspace:a', 2, { n: 'R' }, 200) }
    const plan = planSync(local, remote, { 'workspace:a': 1 })
    expect(plan.push).toHaveLength(1)
    expect(plan.conflicts[0]!.winner).toBe('local')

    // 时间戳并列 → 服务端胜：与 Rust 侧同一裁决，两端不会互相覆盖
    const tie = planSync(local, { 'workspace:a': rec('workspace:a', 2, { n: 'R' }, 300) }, {
      'workspace:a': 1,
    })
    expect(tie.pull).toHaveLength(1)
    expect(tie.conflicts[0]!.winner).toBe('remote')
  })

  it('显式策略覆盖时序：localWins/remoteWins', () => {
    const local = { 'workspace:a': rec('workspace:a', 2, { n: 'L' }, 500) }
    const remote = { 'workspace:a': rec('workspace:a', 2, { n: 'R' }, 100) }
    expect(planSync(local, remote, { 'workspace:a': 1 }, 'remoteWins').pull).toHaveLength(1)
    expect(planSync(local, remote, { 'workspace:a': 1 }, 'localWins').push).toHaveLength(1)
  })

  it('墓碑传播；缺键只是无意见（删除只走墓碑）', () => {
    const localTomb = { 'workspace:a': tombstone('workspace:a', 4, 500) }
    const remoteLive = { 'workspace:a': rec('workspace:a', 3, { n: 1 }, 100) }
    const plan = planSync(localTomb, remoteLive, { 'workspace:a': 3 })
    expect(plan.push[0]!.deleted).toBe(true)
    expect(plan.conflicts).toHaveLength(0)

    // 服务端删了、本地无该键 → 无传输（下一轮按 cursor 自然补墓碑）
    expect(planSync({}, {}, { 'workspace:gone': 2 })).toEqual({
      push: [],
      pull: [],
      conflicts: [],
    })
  })

  it('双端并发收敛模拟：A 推送后 B 拉到权威副本，两端视图一致', () => {
    const aView = { 'workspace:a': rec('workspace:a', 2, { n: 'A' }, 400) }
    const bView = { 'workspace:a': rec('workspace:a', 2, { n: 'B' }, 100) }
    const base = { 'workspace:a': 1 }

    const aPlan = planSync(aView, bView, base)
    expect(aPlan.push).toHaveLength(1)

    // 服务端接受 A 的推送 → 权威副本 rev=3
    const server = { 'workspace:a': rec('workspace:a', 3, { n: 'A' }, 400) }
    // A 应用服务端回执（本地 rev 从乐观 2 推进到权威 3）
    const accepted: SyncRecord[] = [server['workspace:a']!]
    const aAfter = applyToView(aView, accepted)

    const bPlan = planSync(bView, server, base)
    const bAfter = applyToView(bView, bPlan.pull)
    expect(aAfter).toEqual(bAfter)
    // B 的本地改动被覆盖 = 真冲突留痕
    expect(bPlan.conflicts).toHaveLength(1)
    expect(bPlan.conflicts[0]!.winner).toBe('remote')
  })
})

describe('SyncState（发件箱/水位/基线）', () => {
  it('同键后写覆盖，条数受上限约束', () => {
    let state = newSyncState()
    for (let i = 0; i < 105; i++) {
      state = noteLocalChange(state, `workspace:${String(i).padStart(3, '0')}`, { i }, 100 + i)
    }
    expect(pendingPushes(state)).toHaveLength(100)
    state = noteLocalChange(state, 'workspace:000', { i: 999 }, 999)
    expect(state.outbox['workspace:000']!.payload).toEqual({ i: 999 })
    expect(Object.keys(state.outbox)).toHaveLength(105)
  })

  it('buildRequest 带 cursor + base 快照（乐观并发基线）', () => {
    let state: SyncState = { ...newSyncState(), base: { 'workspace:a': 4 } }
    state = noteLocalChange(state, 'workspace:a', { n: 1 }, 10)
    state = noteLocalDelete(state, 'workspace:b', 11)
    const req = buildRequest(state)
    expect(req.base['workspace:a']).toBe(4)
    const a = req.pushes.find((p) => p.key === 'workspace:a')!
    expect(a.base_rev).toBe(4)
    expect(a.deleted).toBe(false)
    const b = req.pushes.find((p) => p.key === 'workspace:b')!
    expect(b.deleted).toBe(true)
    expect(b.payload).toBeNull()
  })

  it('applyResponse 出队/推基线/收冲突；墓碑落地即从视图移除', () => {
    let state = newSyncState()
    state = noteLocalChange(state, 'workspace:a', { n: 1 }, 10)
    state = noteLocalChange(state, 'workspace:b', { n: 2 }, 10)

    const view: Record<string, SyncRecord> = { 'workspace:old': rec('workspace:old', 8, { n: 0 }) }
    const { state: next, outcome } = applyResponse(state, {
      cursor: 42,
      accepted: [rec('workspace:a', 1, { n: 1 }, 20)],
      rejected: [
        {
          key: 'workspace:b',
          reason: 'conflict',
          server: rec('workspace:b', 9, { n: 'server' }, 5),
        },
      ],
      changes: [rec('workspace:z', 7, { n: 3 }, 30), tombstone('workspace:old', 8, 31)],
    })
    expect(next.cursor).toBe(42)
    expect(next.base['workspace:a']).toBe(1)
    expect(outcome.acked).toEqual(['workspace:a'])
    expect(outcome.applied).toHaveLength(3)
    expect(outcome.conflicts[0]!.server!.rev).toBe(9)
    expect(next.outbox['workspace:b'], '冲突项留队待裁决').toBeDefined()

    const after = applyToView(view, outcome.applied)
    expect(after['workspace:old'], '墓碑 = 本地删除').toBeUndefined()
    expect(after['workspace:z']).toBeDefined()
  })

  it('水位回退被忽略（服务端重启重发旧增量不得被当新数据）', () => {
    const state: SyncState = { ...newSyncState(), cursor: 100 }
    const { state: next, outcome } = applyResponse(state, {
      cursor: 5,
      accepted: [],
      rejected: [],
      changes: [rec('workspace:a', 1, { stale: true })],
    })
    expect(outcome.applied).toHaveLength(0)
    expect(next.cursor).toBe(100)
  })
})

describe('wire 解析（服务端字段 snake_case）', () => {
  it('合法响应解析；非法形状与坏 JSON → null', () => {
    const raw = JSON.stringify({
      cursor: 7,
      accepted: [{ key: 'workspace:a', rev: 2, updated_at_ms: 5, deleted: false, payload: { n: 1 } }],
      rejected: [{ key: 'workspace:b', reason: 'conflict' }],
      changes: [{ key: 'workspace:z', rev: 3, updated_at_ms: 6, deleted: true, payload: null }],
    })
    const parsed = parseSyncResponse(raw)!
    expect(parsed.cursor).toBe(7)
    expect(parsed.accepted[0]!.key).toBe('workspace:a')
    expect(parsed.rejected[0]!.reason).toBe('conflict')
    expect(parsed.changes[0]!.deleted).toBe(true)
    expect(parseSyncResponse('nope')).toBeNull()
    expect(parseSyncResponse('{"cursor":"x"}')).toBeNull()
    expect(parseSyncResponse('[]')).toBeNull()
  })

  it('档案解析缺字段即非法（档案面无降级语义）', () => {
    expect(
      parseProfile('{"account_id":"a","display_name":"A","locale":"zh","rev":1}'),
    ).toMatchObject({ account_id: 'a', rev: 1 })
    expect(parseProfile('{"account_id":"a"}')).toBeNull()
    expect(parseProfile('{')).toBeNull()
  })

  it('同内容判定忽略 rev', () => {
    expect(sameContent(rec('k', 1, { n: 1 }), rec('k', 5, { n: 1 }))).toBe(true)
    expect(sameContent(rec('k', 1, { n: 1 }), rec('k', 5, { n: 2 }))).toBe(false)
    expect(sameContent(rec('k', 1, { n: 1 }), tombstone('k', 5, 1))).toBe(false)
  })
})

describe('端点与载荷构造', () => {
  it('档案更新载荷：缺省不改，空邮箱串 = 清除', () => {
    expect(buildProfilePatch({ displayName: 'A', email: '' })).toEqual({
      display_name: 'A',
      email: '',
    })
    expect(buildProfilePatch({})).toEqual({})
  })
  it('地址默认同主机 8080 网关，?gw= 覆盖', () => {
    expect(accountApiBase({ hostname: 'demo.example' })).toBe(
      'http://demo.example:8080/api/v1/account',
    )
    expect(accountApiBase({ hostname: 'd', port: '3000' })).toBe(
      'http://d:3000/api/v1/account',
    )
    expect(accountApiBase({ hostname: 'd', search: '?gw=http://gw:9000/api/v1/account' })).toBe(
      'http://gw:9000/api/v1/account',
    )
    expect(LOCAL_ACCOUNT_ID).toBe('local')
  })
})