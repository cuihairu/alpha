import { useEffect, useState } from 'react'
import { useLocale } from '../hooks/useLocale'
import { parseThemePref, resolveTheme } from '../lib/theme'
import {
  checkAlerts,
  exportSymbolToFile,
  getAppInfo,
  getOfflineQuotes,
  getRealTimeQuotes,
  initializeApp,
  isTauriRuntime,
  saveDialog,
  setPriceAlert,
  setTrayStatus,
  syncOfflineData,
  type AppInfo,
  type DesktopNotification,
  type InitPayload,
  type OfflineQuotesPayload,
  type SyncReport,
  type TrayState,
} from '../lib/desktop'

/**
 * 桌面能力面板（web-framework-selection.md §6 拍板 A，2026-10-10）：
 * 桌面复用 React 产物后的桌面专属面——配置自举（initialize_app，含损坏回退
 * 提示）、原生「另存为」导出（L113）、告警/托盘闭环（L114）、离线缓存与
 * 增量同步（L116）。语义全在 Rust 框架层（gui.rs 只做映射），本组件只做
 * 装配与结果呈现，与兜底壳 demo 等价迁移。非 Tauri 运行时整面隐藏
 * （浏览器/PWA 零 DOM 影响）。
 *
 * 主题基线：桌面配置 theme 为启动基线（与兜底壳同口径），ThemeToggle 的
 * 用户操作在之后覆盖（用户意图优先）；配置 theme=system 时两边同为跟随
 * 系统，不产生分歧。
 */
export function DesktopPanel() {
  const { tr, trf } = useLocale()
  // 运行时判定收敛在挂载态：SSR/测试环境下首次渲染与浏览器一致（null）
  const [runtime] = useState(isTauriRuntime)
  const [boot, setBoot] = useState<InitPayload | null>(null)
  const [info, setInfo] = useState<AppInfo | null>(null)
  const [exportMsg, setExportMsg] = useState<string | null>(null)
  const [fired, setFired] = useState<DesktopNotification[] | null>(null)
  const [tray, setTray] = useState<TrayState | null>(null)
  const [alertBusy, setAlertBusy] = useState(false)
  const [offline, setOffline] = useState<OfflineQuotesPayload | null>(null)
  const [sync, setSync] = useState<SyncReport | null>(null)

  useEffect(() => {
    if (!isTauriRuntime()) return
    let cancelled = false
    initializeApp()
      .then((payload) => {
        if (cancelled) return
        setBoot(payload)
        const dark = window.matchMedia('(prefers-color-scheme: dark)').matches
        document.documentElement.dataset.theme = resolveTheme(
          parseThemePref(payload.config.theme),
          dark,
        )
        return getAppInfo().then((i) => {
          if (!cancelled) setInfo(i)
        })
      })
      .catch(() => {
        // 自举失败保持「未就绪」态：桌面 Rust 侧异常不拖垮整个 React 应用
      })
    return () => {
      cancelled = true
    }
  }, [])

  if (!runtime) return null

  const firstSymbol = boot?.config.symbols[0]

  const runExport = () => {
    if (!firstSymbol) {
      setExportMsg(tr('desktop.noSymbols'))
      return
    }
    setExportMsg(null)
    saveDialog({
      defaultPath: `${firstSymbol}.csv`,
      filters: [
        { name: 'CSV', extensions: ['csv'] },
        { name: 'JSON', extensions: ['json'] },
      ],
    })
      .then((path) => {
        if (!path) {
          setExportMsg(tr('desktop.exportCancelled'))
          return null
        }
        const fmt = /\.json$/i.test(path) ? 'json' : 'csv'
        return exportSymbolToFile(firstSymbol, fmt, path).then((outcome) => {
          // 目标已存在且用户在原生覆盖确认里选了「否」：同取消口径
          if (!outcome) {
            setExportMsg(tr('desktop.exportCancelled'))
            return
          }
          setExportMsg(trf('desktop.exportDone', { rows: outcome.rows, name: outcome.filename }))
        })
      })
      .catch((e: unknown) => {
        setExportMsg(trf('desktop.exportFail', { msg: String(e) }))
      })
  }

  // 演示口径与兜底壳一致：目标价取现价 −1%（确定性行情即刻满足上穿，
  // 触发即停用；同文重复触发由通知队列去重）
  const armAlert = () => {
    if (!firstSymbol) {
      setExportMsg(tr('desktop.noSymbols'))
      return
    }
    setAlertBusy(true)
    getRealTimeQuotes([firstSymbol])
      .then((quotes) => {
        const q = quotes[0]
        if (!q) throw new Error(tr('desktop.noSymbols'))
        return setPriceAlert(firstSymbol, q.price * 0.99, 'above')
      })
      .then(() => checkAlerts())
      .then((notifications) => setFired(notifications))
      .then(() => setTrayStatus())
      .then((status) => setTray(status))
      .catch((e: unknown) => {
        setExportMsg(trf('desktop.cmdFail', { msg: String(e) }))
      })
      .finally(() => setAlertBusy(false))
  }

  const runOffline = () => {
    if (!boot?.config.symbols.length) {
      setExportMsg(tr('desktop.noSymbols'))
      return
    }
    getOfflineQuotes(boot.config.symbols)
      .then((payload) => setOffline(payload))
      .catch((e: unknown) => {
        setExportMsg(trf('desktop.cmdFail', { msg: String(e) }))
      })
  }

  const runSync = () => {
    if (!boot?.config.symbols.length) {
      setExportMsg(tr('desktop.noSymbols'))
      return
    }
    syncOfflineData(boot.config.symbols)
      .then((report) => setSync(report))
      .catch((e: unknown) => {
        setExportMsg(trf('desktop.cmdFail', { msg: String(e) }))
      })
  }

  return (
    <section>
      <h2>{tr('desktop.title')}</h2>
      <p>{tr('desktop.intro')}</p>
      {!boot ? (
        <p>{tr('desktop.notReady')}</p>
      ) : (
        <>
          <ul>
            <li>
              {tr('desktop.source')}: {boot.source}
            </li>
            <li>
              {tr('desktop.apiUrl')}: {boot.config.api_url}
            </li>
            <li>
              {tr('desktop.symbols')}: {boot.config.symbols.join(', ')}
            </li>
            {info && (
              <li>
                {tr('desktop.product')}: {info.name} {info.version}（{info.os} / {info.arch}）
              </li>
            )}
          </ul>
          {boot.validation.length > 0 && (
            <p>
              {tr('desktop.validation')}: {boot.validation.join('；')}
            </p>
          )}
          <p>
            <button type="button" onClick={runExport}>
              {tr('desktop.exportBtn')}
            </button>{' '}
            <button type="button" onClick={armAlert} disabled={alertBusy}>
              {tr('desktop.alertBtn')}
            </button>{' '}
            <button type="button" onClick={runOffline}>
              {tr('desktop.offlineRead')}
            </button>{' '}
            <button type="button" onClick={runSync}>
              {tr('desktop.offlineSync')}
            </button>
          </p>
          {exportMsg && <p>{exportMsg}</p>}
          {fired && (
            <ul>
              <li>
                {tr('desktop.fired')}:{' '}
                {fired.length === 0
                  ? tr('desktop.firedNone')
                  : fired.map((n) => `${n.title} — ${n.body}`).join('；')}
              </li>
              {tray && (
                <>
                  <li>
                    {tr('desktop.tray')}: {tray.status_text}
                  </li>
                  <li>
                    {tr('desktop.activeAlerts')}: {tray.active_alerts}
                  </li>
                  <li>
                    {tr('desktop.lastTrigger')}: {tray.last_trigger ?? '—'}
                  </li>
                </>
              )}
            </ul>
          )}
          {offline && (
            <ul>
              <li>{offline.online ? tr('desktop.online') : tr('desktop.offline')}</li>
              {offline.quotes.map((q) => (
                <li key={q.symbol}>
                  {q.symbol}: {q.price} [{q.source}]
                </li>
              ))}
              {offline.missing.length > 0 && (
                <li>
                  {tr('desktop.missing')}: {offline.missing.join('，')}
                </li>
              )}
            </ul>
          )}
          {sync && (
            <ul>
              <li>{sync.online ? tr('desktop.online') : tr('desktop.offline')}</li>
              <li>
                {tr('desktop.applied')}: {sync.applied.join('，') || '—'}
              </li>
              <li>
                {tr('desktop.unchanged')}: {sync.unchanged}
              </li>
              <li>
                {tr('desktop.failed')}: {sync.failed.join('，') || '—'}
              </li>
            </ul>
          )}
        </>
      )}
    </section>
  )
}
