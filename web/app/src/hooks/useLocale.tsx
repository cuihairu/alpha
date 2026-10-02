import { createContext, useCallback, useContext, useState, type ReactNode } from 'react'
import {
  intlTag,
  LOCALE_STORAGE_KEY,
  parseLocale,
  t,
  tf,
  type I18nKey,
  type Locale,
} from '../lib/i18n'

interface LocaleApi {
  locale: Locale
  setLocale: (next: Locale) => void
  /** 查字典（当前语言） */
  tr: (key: I18nKey) => string
  /** 插值查字典（当前语言） */
  trf: (key: I18nKey, vars: Record<string, string | number>) => string
  /** Intl 标签（数字/时间本地化） */
  tag: string
}

const LocaleCtx = createContext<LocaleApi | null>(null)

function initialLocale(): Locale {
  try {
    return parseLocale(
      window.location.search,
      window.localStorage.getItem(LOCALE_STORAGE_KEY),
      window.navigator.language,
    )
  } catch {
    return 'zh'
  }
}

/** 语言 Provider（挂 App 根；切换即全树重渲染，偏好持久化） */
export function LocaleProvider({ children }: { children: ReactNode }) {
  const [locale, setLocaleState] = useState<Locale>(initialLocale)
  const setLocale = useCallback((next: Locale) => {
    setLocaleState(next)
    try {
      window.localStorage.setItem(LOCALE_STORAGE_KEY, next)
    } catch {
      // 隐私模式写失败不阻塞会话（与主题/工作区同纪律）
    }
  }, [])
  const tr = useCallback((key: I18nKey) => t(locale, key), [locale])
  const trf = useCallback(
    (key: I18nKey, vars: Record<string, string | number>) => tf(locale, key, vars),
    [locale],
  )
  return (
    <LocaleCtx.Provider value={{ locale, setLocale, tr, trf, tag: intlTag(locale) }}>
      {children}
    </LocaleCtx.Provider>
  )
}

/** 消费语言（必须在 LocaleProvider 内） */
export function useLocale(): LocaleApi {
  const api = useContext(LocaleCtx)
  if (api === null) throw new Error('useLocale must be used inside <LocaleProvider>')
  return api
}
