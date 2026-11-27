import { createContext, useCallback, useContext, useState, type ReactNode } from 'react'
import { getLang, translate, type Lang, type MsgParams } from './runtime'

export type { Lang, MessageKey, Messages, MsgParams } from './runtime'

interface I18nContextValue {
  lang: Lang
  setLang: (lang: Lang) => void
  t: (key: string, params?: MsgParams) => string
}

const I18nContext = createContext<I18nContextValue | null>(null)

export function LocaleProvider({ children }: { children: ReactNode }) {
  const [lang, setLangState] = useState<Lang>(getLang)

  const setLang = useCallback((newLang: Lang) => {
    setLangState(newLang)
    localStorage.setItem('voxril-lang', newLang)
  }, [])

  const t = useCallback((key: string, params?: MsgParams): string => translate(lang, key, params), [lang])

  return <I18nContext.Provider value={{ lang, setLang, t }}>{children}</I18nContext.Provider>
}

export function useI18n() {
  const ctx = useContext(I18nContext)
  if (!ctx) {
    throw new Error('useI18n must be used within a LocaleProvider')
  }
  return {
    t: ctx.t,
    lang: ctx.lang,
    setLang: ctx.setLang,
    isZh: ctx.lang === 'zh',
    isEn: ctx.lang === 'en',
  }
}
