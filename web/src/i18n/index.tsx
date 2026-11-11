import { createContext, useContext, useState, useCallback, type ReactNode } from 'react'
import zh from './zh'
import en from './en'

export type MessageKey = keyof typeof zh
export type Messages = Record<MessageKey, string>
export type Lang = 'zh' | 'en'

const messages: Record<Lang, Messages> = { zh, en }

function detectLang(): Lang {
  const saved = localStorage.getItem('voicecraft-lang') as Lang | null
  if (saved === 'zh' || saved === 'en') return saved
  return navigator.language.startsWith('zh') ? 'zh' : 'en'
}

interface I18nContextValue {
  lang: Lang
  setLang: (lang: Lang) => void
  t: (key: string, fallback?: string) => string
}

const I18nContext = createContext<I18nContextValue | null>(null)

export function LocaleProvider({ children }: { children: ReactNode }) {
  const [lang, setLangState] = useState<Lang>(detectLang)

  const setLang = useCallback((newLang: Lang) => {
    setLangState(newLang)
    localStorage.setItem('voicecraft-lang', newLang)
  }, [])

  const t = useCallback((key: string, fallback?: string): string => {
    return (messages[lang] as Record<string, string>)[key] || fallback || key
  }, [lang])

  return (
    <I18nContext.Provider value={{ lang, setLang, t }}>
      {children}
    </I18nContext.Provider>
  )
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
