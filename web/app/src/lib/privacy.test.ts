import { describe, expect, it } from 'vitest'
import {
  APP_KEY_PREFIX,
  clearUserData,
  exportUserData,
  listUserDataKeys,
  USER_DATA_KEYS,
  type KeyValueStore,
} from './privacy'

/** Map 后备的最小存储实现（与 localStorageStore 同接口的测试替身） */
function fakeStore(entries: Record<string, string> = {}): KeyValueStore {
  const map = new Map(Object.entries(entries))
  return {
    getItem: (key) => map.get(key) ?? null,
    setItem: (key, value) => void map.set(key, value),
    removeItem: (key) => void map.delete(key),
    ownKeys: () => [...map.keys()],
  }
}

describe('listUserDataKeys（显式清单 ∪ 前缀扫描）', () => {
  it('空存储不产出键；显式清单键按存在性收录', () => {
    expect(listUserDataKeys(fakeStore())).toEqual([])
    expect(listUserDataKeys(fakeStore({ 'alpha.theme': 'dark' }))).toEqual(['alpha.theme'])
    expect(
      listUserDataKeys(fakeStore({ 'alpha.theme': 'dark', 'alpha.workspaces': '{}' })),
    ).toEqual(['alpha.workspaces', 'alpha.theme'])
  })

  it('alpha. 前缀扫描覆盖未登记的应用键（未来键自动纳入权利面）', () => {
    const store = fakeStore({ 'alpha.future-thing': 'x', 'other.app.key': 'y', beta: 'z' })
    expect(listUserDataKeys(store)).toEqual(['alpha.future-thing'])
  })

  it('清单常量与文档口径同步（alpha.workspaces / alpha.theme）', () => {
    expect(USER_DATA_KEYS).toContain('alpha.workspaces')
    expect(USER_DATA_KEYS).toContain('alpha.theme')
    expect(APP_KEY_PREFIX).toBe('alpha.')
  })
})

describe('exportUserData（可携权：机器可读 JSON 快照）', () => {
  it('全量键值原样快照 + schema/时间戳自描述', () => {
    const now = new Date('2026-10-02T08:00:00.000Z')
    const store = fakeStore({ 'alpha.theme': 'dark', 'alpha.workspaces': '{"workspaces":[]}' })
    expect(exportUserData(store, now)).toEqual({
      schema: 'alpha.user-data',
      version: 1,
      exportedAt: '2026-10-02T08:00:00.000Z',
      data: { 'alpha.theme': 'dark', 'alpha.workspaces': '{"workspaces":[]}' },
    })
  })

  it('空存储产出合法空载荷；非应用键不进入导出面', () => {
    const out = exportUserData(fakeStore({ external: 'keep' }))
    expect(out.data).toEqual({})
    expect(out.schema).toBe('alpha.user-data')
  })
})

describe('clearUserData（被遗忘权：只删应用键，幂等）', () => {
  it('删除全部应用维度键并返回清单；非应用键不动', () => {
    const store = fakeStore({
      'alpha.theme': 'dark',
      'alpha.workspaces': '{}',
      external: 'keep-me',
    })
    const removed = clearUserData(store)
    expect(removed).toEqual(['alpha.workspaces', 'alpha.theme'])
    expect(store.getItem('alpha.theme')).toBeNull()
    expect(store.getItem('alpha.workspaces')).toBeNull()
    expect(store.getItem('external')).toBe('keep-me')
  })

  it('幂等：再跑一次返回空清单', () => {
    const store = fakeStore({ 'alpha.theme': 'light' })
    expect(clearUserData(store)).toEqual(['alpha.theme'])
    expect(clearUserData(store)).toEqual([])
  })
})
