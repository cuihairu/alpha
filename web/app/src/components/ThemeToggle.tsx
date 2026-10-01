import { useEffect, useState } from 'react'
import {
  THEME_PREFS,
  loadThemePref,
  resolveTheme,
  saveThemePref,
  type ThemePref,
} from '../lib/theme'

/** 应用主题到根元素（data-theme 驱动 styles.css 的令牌切换） */
function applyTheme(resolved: 'light' | 'dark') {
  document.documentElement.dataset.theme = resolved
}

/**
 * 主题切换器（L432 统一主题与个性化配置）：三态偏好持久化 localStorage，
 * system 档跟随 prefers-color-scheme 实时变化；深浅令牌见 styles.css。
 */
export function ThemeToggle() {
  const [pref, setPref] = useState<ThemePref>(() => loadThemePref())

  useEffect(() => {
    const media = window.matchMedia('(prefers-color-scheme: dark)')
    const apply = () => applyTheme(resolveTheme(pref, media.matches))
    apply()
    saveThemePref(pref)
    if (pref !== 'system') return
    media.addEventListener('change', apply)
    return () => media.removeEventListener('change', apply)
  }, [pref])

  return (
    <label className="theme-toggle">
      主题{' '}
      <select value={pref} onChange={(e) => setPref(e.target.value as ThemePref)}>
        {THEME_PREFS.map((t) => (
          <option key={t.value} value={t.value}>
            {t.label}
          </option>
        ))}
      </select>
    </label>
  )
}
