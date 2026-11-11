import { create } from 'zustand'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { invoke } from '@tauri-apps/api/core'
import type { SeparateRequest, SeparateResponse } from '../api/uvr'

export interface UvrResult {
  ok: boolean
  message: string
  vocalDir: string
  instrumentDir: string
  vocalFiles: string[]
  instrumentFiles: string[]
}

interface UvrState {
  loading: boolean
  log: string
  progressPercent: number
  progressStep: string
  progressCurrent: number
  progressTotal: number
  result: UvrResult | null
  _unlisten: UnlistenFn | null
  _eventId: string

  startSeparate: (request: SeparateRequest) => Promise<void>
  cancel: () => Promise<void>
  reset: () => void
}

export const useUvrStore = create<UvrState>((set, get) => ({
  loading: false,
  log: '',
  progressPercent: 0,
  progressStep: '',
  progressCurrent: 0,
  progressTotal: 0,
  result: null,
  _unlisten: null,
  _eventId: '',

  startSeparate: async (request: SeparateRequest) => {
    const prev = get()
    if (prev._unlisten) {
      prev._unlisten()
    }

    const eventId = `uvr-${Date.now()}`
    set({
      loading: true,
      log: '',
      progressPercent: 0,
      progressStep: '',
      progressCurrent: 0,
      progressTotal: 0,
      result: null,
      _eventId: eventId,
    })

    try {
      const unlisten = await listen<{
        event_id: string; kind: string; step: string
        current: number; total: number; percent: number; message: string
      }>('uvr-separate-progress', (event) => {
        const p = event.payload
        if (p.event_id !== eventId) return
        if (p.kind === 'progress') {
          set({
            progressPercent: p.percent,
            progressStep: p.step,
            progressCurrent: p.current,
            progressTotal: p.total,
          })
        }
        if (p.kind === 'progress' || p.kind === 'log') {
          set(s => {
            const next = s.log + p.message + '\n'
            if (next.length > 50_000) {
              return { log: next.slice(next.length - 30_000) }
            }
            return { log: next }
          })
        }
      })
      set({ _unlisten: unlisten })

      const result = await invoke<SeparateResponse>('separate_vocals_streaming', { request, eventId })
      set(s => ({
        log: s.log + '\n' + (result.message || '完成'),
        result: {
          ok: result.ok,
          message: result.message,
          vocalDir: result.vocal_dir,
          instrumentDir: result.instrumental_dir,
          vocalFiles: result.vocal_files,
          instrumentFiles: result.instrumental_files,
        },
      }))
    } catch (error) {
      set(s => ({ log: s.log + `\n失败：${String(error)}` }))
    } finally {
      set({ loading: false })
    }
  },

  cancel: async () => {
    try {
      await invoke('cancel_uvr')
    } catch { /* ignore */ }
    const { _unlisten } = get()
    if (_unlisten) _unlisten()
    set({
      loading: false,
      _unlisten: null,
      _eventId: '',
    })
  },

  reset: () => {
    const { _unlisten } = get()
    if (_unlisten) _unlisten()
    set({
      loading: false,
      log: '',
      progressPercent: 0,
      progressStep: '',
      progressCurrent: 0,
      progressTotal: 0,
      result: null,
      _unlisten: null,
      _eventId: '',
    })
  },
}))
