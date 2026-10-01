import { describe, expect, it } from 'vitest'
import {
  activeWorkspace,
  createWorkspace,
  deleteWorkspace,
  initialWorkspaceState,
  MAX_SYMBOLS_PER_WORKSPACE,
  normalizeSymbol,
  parseWorkspaceState,
  renameWorkspace,
  serializeWorkspaceState,
  setWorkspaceSymbols,
  switchWorkspace,
  type WorkspaceState,
} from './workspaces'

/** 归约器 + 持久化合法性收口（L508；浏览器 localStorage 薄壳不在此测） */
describe('workspaces', () => {
  const seed = (symbols: string[] = ['600519']): WorkspaceState => initialWorkspaceState(symbols)

  it('初始状态：单默认工作区，标的去重', () => {
    const s = seed(['600519', '600519', '000001'])
    expect(s.workspaces).toHaveLength(1)
    expect(s.workspaces[0].name).toBe('默认')
    expect(s.workspaces[0].symbols).toEqual(['600519', '000001'])
    expect(activeWorkspace(s).id).toBe(s.activeId)
  })

  it('新建：空名兜底「工作区」，重名追加序号，新工作区切换为活动', () => {
    let s = seed()
    s = createWorkspace(s, '')
    expect(s.workspaces[1].name).toBe('工作区')
    expect(s.workspaces[1].symbols).toEqual([])
    s = createWorkspace(s, '工作区')
    expect(s.workspaces[2].name).toBe('工作区 2')
    expect(activeWorkspace(s).id).toBe(s.workspaces[2].id)
  })

  it('重命名：空名与重名目标均不生效', () => {
    let s = seed()
    s = createWorkspace(s, 'A')
    const aId = s.workspaces[1].id
    s = renameWorkspace(s, aId, '  ')
    expect(s.workspaces[1].name).toBe('A')
    s = renameWorkspace(s, aId, '默认')
    expect(s.workspaces[1].name).toBe('A')
    s = renameWorkspace(s, aId, '策略池')
    expect(s.workspaces[1].name).toBe('策略池')
  })

  it('删除：最后一个不可删；删活动工作区回退到第一个', () => {
    const only = seed()
    expect(deleteWorkspace(only, only.workspaces[0].id)).toBe(only)
    let s = seed()
    s = createWorkspace(s, 'B')
    const secondId = s.workspaces[1].id
    s = deleteWorkspace(s, secondId)
    expect(s.workspaces).toHaveLength(1)
    expect(s.activeId).toBe(s.workspaces[0].id)
  })

  it('切换：未知 id 不生效', () => {
    const s = seed()
    expect(switchWorkspace(s, 'nope')).toBe(s)
    const created = createWorkspace(s, 'B')
    expect(activeWorkspace(created).name).toBe('B')
    expect(activeWorkspace(switchWorkspace(created, s.workspaces[0].id)).name).toBe('默认')
  })

  it('标的集：非法剔除、去重、上限截断', () => {
    let s = seed()
    const id = s.workspaces[0].id
    s = setWorkspaceSymbols(s, id, ['600519', 'abc', '000001', '600519'])
    expect(s.workspaces[0].symbols).toEqual(['600519', '000001'])
    const overflow = Array.from({ length: MAX_SYMBOLS_PER_WORKSPACE + 5 }, (_, i) =>
      String(600000 + i),
    )
    s = setWorkspaceSymbols(s, id, overflow)
    expect(s.workspaces[0].symbols).toHaveLength(MAX_SYMBOLS_PER_WORKSPACE)
    expect(setWorkspaceSymbols(s, 'missing', ['600519'])).toBe(s)
  })

  it('normalizeSymbol：只收 6 位数字（trim 后）', () => {
    expect(normalizeSymbol(' 600519 ')).toBe('600519')
    expect(normalizeSymbol('60051')).toBeNull()
    expect(normalizeSymbol('6005190')).toBeNull()
    expect(normalizeSymbol('sh600519')).toBeNull()
  })

  it('持久化往返：合法 JSON 还原；结构损坏返 null 走初始态', () => {
    let s = seed(['600519', '000001'])
    s = createWorkspace(s, '策略池')
    s = setWorkspaceSymbols(s, s.workspaces[1].id, ['510300'])
    expect(parseWorkspaceState(serializeWorkspaceState(s))).toEqual(s)

    expect(parseWorkspaceState(null)).toBeNull()
    expect(parseWorkspaceState('{not json')).toBeNull()
    expect(parseWorkspaceState('[]')).toBeNull()
    // 全部工作区非法（无名字/空 symbols）→ null
    expect(
      parseWorkspaceState(JSON.stringify({ workspaces: [{ name: ' ', symbols: 'x' }], activeId: 'x' })),
    ).toBeNull()
    // 非法标的剔除、缺 id 重生成、activeId 失配回第一个
    const parsed = parseWorkspaceState(
      JSON.stringify({
        workspaces: [
          { name: '甲', symbols: ['600519', 'bad', '600519'] },
          { name: '乙', symbols: [] },
        ],
        activeId: 'gone',
      }),
    )!
    expect(parsed.workspaces).toHaveLength(2)
    expect(parsed.workspaces[0].symbols).toEqual(['600519'])
    expect(parsed.activeId).toBe(parsed.workspaces[0].id)
    // 删除活动工作区后 activeId 指向留存项
    const two = createWorkspace(seed(), 'B')
    const kept = deleteWorkspace(two, two.workspaces[1].id)
    expect(parseWorkspaceState(serializeWorkspaceState(kept))).toEqual(kept)
  })
})
