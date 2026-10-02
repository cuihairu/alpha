import { useState } from 'react'
import type { useWorkspaces } from '../hooks/useWorkspaces'
import { useLocale } from '../hooks/useLocale'
import { MAX_SYMBOLS_PER_WORKSPACE, normalizeSymbol } from '../lib/workspaces'

type WorkspaceActions = ReturnType<typeof useWorkspaces>

/**
 * 多标签页工作区（L508）：标签条（切换/重命名/删除/新建）+ 活动工作区的
 * 自选标的编辑（chips 移除、输入添加，归一化/去重/上限在 lib 收口）。
 * 活动工作区的标的集驱动实时行情看板（App 接线）。
 */
export function WorkspaceTabs({ ws, record }: { ws: WorkspaceActions; record?: (name: string) => void }) {
  const { tr, trf } = useLocale()
  const [adding, setAdding] = useState(false)
  const [newName, setNewName] = useState('')
  const [editing, setEditing] = useState(false)
  const [editName, setEditName] = useState('')
  const [symbolInput, setSymbolInput] = useState('')
  const [symbolError, setSymbolError] = useState<string | null>(null)

  const active = ws.active

  const submitNew = () => {
    ws.create(newName)
    record?.('workspace.create')
    setNewName('')
    setAdding(false)
  }

  const submitRename = () => {
    ws.rename(active.id, editName)
    setEditName('')
    setEditing(false)
  }

  const addSymbol = () => {
    const symbol = normalizeSymbol(symbolInput)
    if (symbol === null) {
      setSymbolError(tr('ws.symbolInvalid'))
      return
    }
    if (active.symbols.includes(symbol)) {
      setSymbolError(tr('ws.symbolDup'))
      return
    }
    if (active.symbols.length >= MAX_SYMBOLS_PER_WORKSPACE) {
      setSymbolError(trf('ws.symbolCap', { max: MAX_SYMBOLS_PER_WORKSPACE }))
      return
    }
    ws.setSymbols(active.id, [...active.symbols, symbol])
    setSymbolInput('')
    setSymbolError(null)
  }

  return (
    <div className="workspace-bar">
      <div className="workspace-tabs" role="tablist">
        {ws.state.workspaces.map((w) => (
          <span
            key={w.id}
            role="tab"
            aria-selected={w.id === active.id}
            className={`workspace-tab${w.id === active.id ? ' workspace-tab-active' : ''}`}
          >
            <button type="button" className="workspace-tab-name" onClick={() => ws.switchTo(w.id)}>
              {w.name}
            </button>
            {ws.state.workspaces.length > 1 && (
              <button
                type="button"
                className="workspace-tab-close"
                aria-label={trf('ws.deleteTab', { name: w.name })}
                onClick={() => ws.remove(w.id)}
              >
                ×
              </button>
            )}
          </span>
        ))}
        {adding ? (
          <span className="workspace-new">
            <input
              value={newName}
              onChange={(e) => setNewName(e.target.value)}
              placeholder={tr('ws.newPlaceholder')}
              autoFocus
              onKeyDown={(e) => {
                if (e.key === 'Enter') submitNew()
                if (e.key === 'Escape') setAdding(false)
              }}
            />
            <button type="button" onClick={submitNew}>{tr('ws.confirm')}</button>
          </span>
        ) : (
          <button type="button" className="workspace-add" onClick={() => setAdding(true)}>
            {tr('ws.create')}
          </button>
        )}
      </div>
      <div className="workspace-detail">
        <span className="workspace-name">
          {editing ? (
            <input
              value={editName}
              onChange={(e) => setEditName(e.target.value)}
              autoFocus
              onKeyDown={(e) => {
                if (e.key === 'Enter') submitRename()
                if (e.key === 'Escape') setEditing(false)
              }}
            />
          ) : (
            <>
              {active.name}
              <button
                type="button"
                className="workspace-rename"
                onClick={() => {
                  setEditName(active.name)
                  setEditing(true)
                }}
              >
                {tr('ws.rename')}
              </button>
            </>
          )}
        </span>
        <span className="workspace-symbols">
          {active.symbols.map((s) => (
            <span key={s} className="workspace-chip">
              {s}
              <button
                type="button"
                aria-label={trf('ws.removeSymbol', { symbol: s })}
                onClick={() => ws.setSymbols(active.id, active.symbols.filter((x) => x !== s))}
              >
                ×
              </button>
            </span>
          ))}
          <input
            className="workspace-symbol-input"
            value={symbolInput}
            onChange={(e) => {
              setSymbolInput(e.target.value)
              setSymbolError(null)
            }}
            placeholder={tr('ws.addSymbolPlaceholder')}
            onKeyDown={(e) => {
              if (e.key === 'Enter') addSymbol()
            }}
          />
          {symbolError && <span className="workspace-error">{symbolError}</span>}
        </span>
      </div>
    </div>
  )
}
