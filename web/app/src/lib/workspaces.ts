/**
 * 工作区管理纯函数面（L508 多标签页与工作区管理）：
 * 工作区 = 命名的自选标的集，标签条切换活动工作区；归约器全部不可变、
 * 约束收口（至少保留一个工作区、标的去重/上限），持久化键集中在此，
 * localStorage 包装做隐私模式/异常兜底（沿 theme.ts 模式，浏览器面薄壳
 * 不进单测）。
 */

export interface Workspace {
  id: string
  name: string
  symbols: string[]
}

export interface WorkspaceState {
  workspaces: Workspace[]
  activeId: string
}

export const WORKSPACE_STORAGE_KEY = 'alpha.workspaces'

/** 单工作区标的上限（防 localStorage 无界增长；超过拒绝写入） */
export const MAX_SYMBOLS_PER_WORKSPACE = 30

/** A股代码口径：6 位数字（ sh/sz 前缀形态归后端接入项，此处只收裸代码） */
export function normalizeSymbol(raw: string): string | null {
  const s = raw.trim()
  return /^\d{6}$/.test(s) ? s : null
}

let idCounter = 0
/** 工作区 id：单调计数 + 随机尾巴（跨页签不冲突不是目标——同源存储共享即可） */
export function newWorkspaceId(): string {
  idCounter += 1
  return `ws-${Date.now().toString(36)}-${idCounter}-${Math.floor(Math.random() * 1e6).toString(36)}`
}

export const DEFAULT_WORKSPACE_NAME = '默认'

/** 初始状态：单默认工作区（标的集由调用方注入演示/用户值；默认名可本地化覆盖） */
export function initialWorkspaceState(symbols: string[], defaultName = DEFAULT_WORKSPACE_NAME): WorkspaceState {
  const id = newWorkspaceId()
  return {
    workspaces: [{ id, name: defaultName, symbols: dedupe(symbols) }],
    activeId: id,
  }
}

function dedupe(symbols: string[]): string[] {
  return [...new Set(symbols)]
}

function findByName(workspaces: Workspace[], name: string): Workspace | undefined {
  return workspaces.find((w) => w.name === name)
}

/** 新建工作区：重名追加序号（「默认」→「默认 2」），新工作区空标的集；空名兜底可本地化覆盖 */
export function createWorkspace(state: WorkspaceState, rawName: string, blankName = '工作区'): WorkspaceState {
  const base = rawName.trim() === '' ? blankName : rawName.trim()
  let name = base
  let n = 2
  while (findByName(state.workspaces, name) !== undefined) {
    name = `${base} ${n}`
    n += 1
  }
  const id = newWorkspaceId()
  return {
    workspaces: [...state.workspaces, { id, name, symbols: [] }],
    activeId: id,
  }
}

/** 重命名：空名不生效（返回原状态）；重名目标拒绝 */
export function renameWorkspace(state: WorkspaceState, id: string, rawName: string): WorkspaceState {
  const name = rawName.trim()
  if (name === '') return state
  if (findByName(state.workspaces, name) !== undefined) return state
  return {
    ...state,
    workspaces: state.workspaces.map((w) => (w.id === id ? { ...w, name } : w)),
  }
}

/** 删除：至少保留一个工作区（删最后一个 = no-op）；删活动工作区切到第一个 */
export function deleteWorkspace(state: WorkspaceState, id: string): WorkspaceState {
  if (state.workspaces.length <= 1) return state
  const workspaces = state.workspaces.filter((w) => w.id !== id)
  const activeId = state.activeId === id ? workspaces[0].id : state.activeId
  return { workspaces, activeId }
}

export function switchWorkspace(state: WorkspaceState, id: string): WorkspaceState {
  return state.workspaces.some((w) => w.id === id) ? { ...state, activeId: id } : state
}

/**
 * 设置标的集：归一化非法剔除（非 6 位数字）、去重、上限截断；活动工作区
 * 未找到（理论不可达）返回原状态。
 */
export function setWorkspaceSymbols(state: WorkspaceState, id: string, rawSymbols: string[]): WorkspaceState {
  const symbols = dedupe(
    rawSymbols.map(normalizeSymbol).filter((s): s is string => s !== null),
  ).slice(0, MAX_SYMBOLS_PER_WORKSPACE)
  if (!state.workspaces.some((w) => w.id === id)) return state
  return {
    ...state,
    workspaces: state.workspaces.map((w) => (w.id === id ? { ...w, symbols } : w)),
  }
}

export function activeWorkspace(state: WorkspaceState): Workspace {
  return state.workspaces.find((w) => w.id === state.activeId) ?? state.workspaces[0]
}

/**
 * 持久化合法性收口：任意 JSON → WorkspaceState（结构缺失/标的非法的
 * 工作区剔除、activeId 失配回第一个、id 非字符串重生成）。损坏/缺省
 * 返 null 由调用方走 initialWorkspaceState。
 */
export function parseWorkspaceState(raw: string | null): WorkspaceState | null {
  if (raw === null) return null
  let data: unknown
  try {
    data = JSON.parse(raw)
  } catch {
    return null
  }
  if (typeof data !== 'object' || data === null || !Array.isArray((data as { workspaces?: unknown }).workspaces)) {
    return null
  }
  const workspaces: Workspace[] = []
  for (const entry of (data as { workspaces: unknown[] }).workspaces) {
    if (typeof entry !== 'object' || entry === null) continue
    const e = entry as { id?: unknown; name?: unknown; symbols?: unknown }
    if (typeof e.name !== 'string' || e.name.trim() === '') continue
    if (!Array.isArray(e.symbols)) continue
    const symbols = dedupe(
      e.symbols.map(normalizeSymbol).filter((s): s is string => s !== null),
    ).slice(0, MAX_SYMBOLS_PER_WORKSPACE)
    const id = typeof e.id === 'string' && e.id !== '' ? e.id : newWorkspaceId()
    workspaces.push({ id, name: e.name, symbols })
  }
  if (workspaces.length === 0) return null
  const activeId =
    typeof (data as { activeId?: unknown }).activeId === 'string' &&
    workspaces.some((w) => w.id === (data as { activeId: string }).activeId)
      ? (data as { activeId: string }).activeId
      : workspaces[0].id
  return { workspaces, activeId }
}

export function serializeWorkspaceState(state: WorkspaceState): string {
  return JSON.stringify(state)
}

export function loadWorkspaceState(fallbackSymbols: string[], defaultName = DEFAULT_WORKSPACE_NAME): WorkspaceState {
  try {
    return parseWorkspaceState(localStorage.getItem(WORKSPACE_STORAGE_KEY)) ??
      initialWorkspaceState(fallbackSymbols, defaultName)
  } catch {
    return initialWorkspaceState(fallbackSymbols, defaultName)
  }
}

export function saveWorkspaceState(state: WorkspaceState): void {
  try {
    localStorage.setItem(WORKSPACE_STORAGE_KEY, serializeWorkspaceState(state))
  } catch {
    // 隐私模式/存储不可用：不持久化，会话内工作区仍然可用
  }
}
