/**
 * 主题系统纯函数面（L432 统一主题与个性化配置）：
 * 三态偏好（浅色/深色/跟随系统）→ 实际生效主题；偏好持久化键集中在此，
 * localStorage 包装做隐私模式/异常兜底（浏览器面薄壳，不进单测）。
 */

export type ThemePref = 'light' | 'dark' | 'system'
export type ResolvedTheme = 'light' | 'dark'

export const THEME_STORAGE_KEY = 'alpha.theme'

export const THEME_PREFS: Array<{ value: ThemePref; label: string }> = [
  { value: 'system', label: '跟随系统' },
  { value: 'light', label: '浅色' },
  { value: 'dark', label: '深色' },
]

/** 偏好 → 生效主题：system 跟随系统深色检测，其余直取 */
export function resolveTheme(pref: ThemePref, systemDark: boolean): ResolvedTheme {
  if (pref === 'system') return systemDark ? 'dark' : 'light'
  return pref
}

/** 合法性收口：localStorage 读回的任意字符串 → ThemePref（缺省跟随系统） */
export function parseThemePref(raw: string | null): ThemePref {
  return raw === 'light' || raw === 'dark' || raw === 'system' ? raw : 'system'
}

export function loadThemePref(): ThemePref {
  try {
    return parseThemePref(localStorage.getItem(THEME_STORAGE_KEY))
  } catch {
    return 'system'
  }
}

export function saveThemePref(pref: ThemePref): void {
  try {
    localStorage.setItem(THEME_STORAGE_KEY, pref)
  } catch {
    // 隐私模式/存储不可用：偏好不持久化，主题仍然生效
  }
}
