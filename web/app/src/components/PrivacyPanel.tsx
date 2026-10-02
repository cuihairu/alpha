import { useMemo, useState } from 'react'
import { useLocale } from '../hooks/useLocale'
import {
  clearUserData,
  exportUserData,
  listUserDataKeys,
  localStorageStore,
  USER_DATA_KEYS,
} from '../lib/privacy'

/**
 * 数据与隐私面板（TODO L488）：GDPR/CCPA 数据主体权利的 UI 面——
 * 数据清单展示、一键导出（可携权，JSON 下载）、一键清除（被遗忘权，
 * 仅 `alpha.` 应用键，非应用存储不动）。清单与按钮语义全部来自
 * lib/privacy 纯函数（本组件只做装配与确认交互）。
 */
export function PrivacyPanel() {
  const { tr } = useLocale()
  const store = useMemo(() => localStorageStore(), [])
  const [presentKeys, setPresentKeys] = useState<string[]>(() => listUserDataKeys(store))
  const [cleared, setCleared] = useState(false)

  const exportJson = () => {
    const payload = exportUserData(store)
    const blob = new Blob([JSON.stringify(payload, null, 2)], { type: 'application/json' })
    const url = URL.createObjectURL(blob)
    const anchor = document.createElement('a')
    anchor.href = url
    anchor.download = `alpha-user-data-${payload.exportedAt.slice(0, 10)}.json`
    anchor.click()
    URL.revokeObjectURL(url)
  }

  const clearAll = () => {
    if (!window.confirm(tr('privacy.confirm'))) return
    const removed = clearUserData(store)
    setPresentKeys(listUserDataKeys(store))
    setCleared(removed.length > 0)
  }

  return (
    <section>
      <h2>{tr('privacy.title')}</h2>
      <p>
        {tr('privacy.descA')}{USER_DATA_KEYS.length}
        {tr('privacy.descB')}
        <code>docs/data-privacy.md</code>{tr('privacy.period')}
      </p>
      <ul>
        {USER_DATA_KEYS.map((key) => (
          <li key={key}>
            <code>{key}</code>
            {presentKeys.includes(key) ? tr('privacy.present') : tr('privacy.absent')}
          </li>
        ))}
      </ul>
      <p>
        <button type="button" onClick={exportJson}>
          {tr('privacy.export')}
        </button>{' '}
        <button type="button" onClick={clearAll}>
          {tr('privacy.clear')}
        </button>
        {cleared && <span role="status">{tr('privacy.cleared')}</span>}
      </p>
    </section>
  )
}
