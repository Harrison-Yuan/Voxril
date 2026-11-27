import { create } from 'zustand'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { invoke } from '@tauri-apps/api/core'
import { coverApi } from '../api/cover'
import type { CoverProgress, CoverRequest, CoverResponse } from '../api/cover'
import { t } from '@/i18n/runtime'

export type CoverStage = 'idle' | 'separate' | 'convert' | 'merge' | 'done'

interface CoverState {
  running: boolean
  stage: CoverStage
  percent: number
  log: string
  result: CoverResponse | null
  error: string
  _unlisten: UnlistenFn | null
  _eventId: string

  start: (request: CoverRequest) => Promise<void>
  cancel: () => Promise<void>
  reset: () => void
}

const MAX_LOG = 30_000

export const useCoverStore = create<CoverState>((set, get) => ({
  running: false,
  stage: 'idle',
  percent: 0,
  log: '',
  result: null,
  error: '',
  _unlisten: null,
  _eventId: '',

  start: async (request: CoverRequest) => {
    const prev = get()
    if (prev._unlisten) prev._unlisten()

    const eventId = `cover-${Date.now()}`
    set({
      running: true,
      stage: 'separate',
      percent: 0,
      log: '',
      result: null,
      error: '',
      _eventId: eventId,
    })

    try {
      const unlisten = await listen<CoverProgress>('cover-progress', (event) => {
        const p = event.payload
        if (p.event_id !== eventId) return
        if (p.kind === 'progress') {
          set({ stage: p.stage as CoverStage, percent: p.percent })
        }
        if (p.message) {
          set(s => {
            const next = s.log + p.message + '\n'
            return { log: next.length > MAX_LOG ? next.slice(next.length - MAX_LOG) : next }
          })
        }
      })
      set({ _unlisten: unlisten })

      const result = await invoke<CoverResponse>('cover_generate', { request, eventId })
      // 期间可能被用户取消/重置，旧任务的返回不再回写状态
      if (get()._eventId !== eventId) return
      if (result.ok && result.mixed_path) {
        set({ result, stage: 'done', percent: 100 })
      } else {
        set({ error: result.message || t('shell.cover_failed') })
      }
    } catch (e) {
      if (get()._eventId === eventId) set({ error: String(e) })
    } finally {
      if (get()._eventId === eventId) set({ running: false })
    }
  },

  cancel: async () => {
    try {
      await coverApi.cancel()
    } catch { /* 取消失败不影响前端状态复位 */ }
    const { _unlisten } = get()
    if (_unlisten) _unlisten()
    set({ running: false, _unlisten: null, _eventId: '', stage: 'idle', percent: 0 })
  },

  reset: () => {
    const { _unlisten } = get()
    if (_unlisten) _unlisten()
    set({
      running: false,
      stage: 'idle',
      percent: 0,
      log: '',
      result: null,
      error: '',
      _unlisten: null,
      _eventId: '',
    })
  },
}))
