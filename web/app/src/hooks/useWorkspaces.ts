import { useCallback, useMemo, useState } from 'react'
import {
  activeWorkspace,
  createWorkspace,
  deleteWorkspace,
  loadWorkspaceState,
  renameWorkspace,
  saveWorkspaceState,
  setWorkspaceSymbols,
  switchWorkspace,
  type WorkspaceState,
} from '../lib/workspaces'

/**
 * 工作区状态挂钩（L508）：归约器纯函数（lib/workspaces.ts）+ localStorage
 * 持久化薄壳（首载读、每次变更写，隐私模式失败不阻塞会话；归约器返回
 * 原引用即 no-op，跳过写盘）。
 */
export function useWorkspaces(fallbackSymbols: string[]) {
  const [state, setState] = useState<WorkspaceState>(() => loadWorkspaceState(fallbackSymbols))

  /** 变更统一出口：no-op（同引用）不写盘 */
  const apply = useCallback((reduce: (s: WorkspaceState) => WorkspaceState) => {
    setState((s) => {
      const next = reduce(s)
      if (next !== s) saveWorkspaceState(next)
      return next
    })
  }, [])

  const actions = useMemo(
    () => ({
      create: (name: string) => apply((s) => createWorkspace(s, name)),
      switchTo: (id: string) => apply((s) => switchWorkspace(s, id)),
      rename: (id: string, name: string) => apply((s) => renameWorkspace(s, id, name)),
      remove: (id: string) => apply((s) => deleteWorkspace(s, id)),
      setSymbols: (id: string, symbols: string[]) => apply((s) => setWorkspaceSymbols(s, id, symbols)),
    }),
    [apply],
  )

  return { state, active: activeWorkspace(state), ...actions }
}
