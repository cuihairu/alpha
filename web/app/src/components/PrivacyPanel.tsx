import { useMemo, useState } from 'react'
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
    if (!window.confirm('清除全部应用本地数据（自选工作区与主题偏好）？此操作不可撤销。')) return
    const removed = clearUserData(store)
    setPresentKeys(listUserDataKeys(store))
    setCleared(removed.length > 0)
  }

  return (
    <section>
      <h2>数据与隐私（L488：本地数据导出 / 清除，无账号体系）</h2>
      <p>
        应用用户数据仅存本机（键清单 {USER_DATA_KEYS.length}
        项：自选工作区、主题偏好），服务端不留存个人维度数据。义务映射见
        <code> docs/data-privacy.md</code>。
      </p>
      <ul>
        {USER_DATA_KEYS.map((key) => (
          <li key={key}>
            <code>{key}</code>
            {presentKeys.includes(key) ? '（本机存在）' : '（未使用）'}
          </li>
        ))}
      </ul>
      <p>
        <button type="button" onClick={exportJson}>
          导出我的数据（JSON）
        </button>{' '}
        <button type="button" onClick={clearAll}>
          清除我的数据
        </button>
        {cleared && <span role="status"> 已清除本机应用数据。</span>}
      </p>
    </section>
  )
}
