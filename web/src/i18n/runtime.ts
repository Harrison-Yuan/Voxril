import zh from './zh'
import en from './en'

export type MessageKey = keyof typeof zh
export type Messages = Record<MessageKey, string>
export type Lang = 'zh' | 'en'
/** 文案插值参数，替换文本里的 {name} 占位 */
export type MsgParams = Record<string, string | number>

export const messages: Record<Lang, Messages> = { zh, en }

function detectLang(): Lang {
  const saved = localStorage.getItem('voxril-lang') as Lang | null
  if (saved === 'zh' || saved === 'en') return saved
  return navigator.language.startsWith('zh') ? 'zh' : 'en'
}

/** 替换文本里的 `{name}` 占位；用 split/join 以兼容不保证 replaceAll 的目标环境 */
export function interpolate(text: string, params?: MsgParams): string {
  if (!params) return text
  return Object.entries(params).reduce((acc, [k, v]) => acc.split(`{${k}}`).join(String(v)), text)
}

/** 按指定语言取文案，缺键时回落为键名本身（便于在界面上直接看出漏翻） */
export function translate(lang: Lang, key: string, params?: MsgParams): string {
  const raw = (messages[lang] as Record<string, string>)[key]
  if (raw === undefined) return key
  return interpolate(raw, params)
}

/**
 * 当前界面语言。语言只由 localStorage 决定（setLang 会写入），
 * 所以 React 之外读到的结果与 Context 始终一致。
 */
export function getLang(): Lang {
  return detectLang()
}

/**
 * 不依赖 Context 的翻译入口，供 store、服务层等非组件代码使用。
 *
 * 组件里仍应优先用 `useI18n()` 的 t：切换语言会触发重渲染，
 * 而这里返回的是调用当下固定下来的字符串。
 */
export function t(key: MessageKey | string, params?: MsgParams): string {
  return translate(detectLang(), key, params)
}
