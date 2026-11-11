import { create } from 'zustand'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { invoke } from '@tauri-apps/api/core'
import type { ConvertRequest, ConvertResponse } from '../api/infer'

interface InferState {
  loading: boolean
  log: string
  pct: number
  cur: number
  tot: number
  outputUrl: string
  sourcePath: string
  _unlisten: UnlistenFn | null
  _eventId: string

  startConvert: (request: ConvertRequest) => Promise<ConvertResponse>
  cancel: () => Promise<void>
  setSourcePath: (path: string) => void
  reset: () => void
  restoreFromEvent: (eventId: string) => Promise<void>
}

export const useInferStore = create<InferState>((set, get) => ({
  loading: false,
  log: '',
  pct: 0,
  cur: 0,
  tot: 0,
  outputUrl: '',
  sourcePath: '',
  _unlisten: null,
  _eventId: '',

  startConvert: async (request: ConvertRequest) => {
    const prev = get()
    if (prev._unlisten) {
      prev._unlisten()
    }

    const eventId = `infer-${Date.now()}`
    set({
      loading: true,
      log: '',
      pct: 0,
      cur: 0,
      tot: 0,
      outputUrl: '',
      _eventId: eventId,
    })

    try {
      const unlisten = await listen<{
        event_id: string; kind: string; current: number; total: number; percent: number; message: string
      }>('infer-convert-progress', (event) => {
        const p = event.payload
        if (p.event_id !== eventId) return
        if (p.kind === 'progress') {
          set({ pct: p.percent, cur: p.current, tot: p.total })
        }
        if (p.kind === 'progress' || p.kind === 'log') {
          set(s => ({ log: s.log + p.message + '\n' }))
        }
      })
      set({ _unlisten: unlisten })

      const result = await invoke<ConvertResponse>('convert_voice_streaming', { request, eventId })
      set(s => ({
        log: s.log + '\n' + (result.message || '完成'),
        outputUrl: result.ok && result.output_path ? result.output_path : '',
      }))
      return result
    } catch (error) {
      set(s => ({ log: s.log + `\n转换失败：${String(error)}` }))
      return { ok: false, message: String(error), output_path: '' }
    } finally {
      set({ loading: false })
    }
  },

  setSourcePath: (path: string) => {
    set({ sourcePath: path })
  },

  cancel: async () => {
    try {
      await invoke('cancel_infer')
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
      pct: 0,
      cur: 0,
      tot: 0,
      outputUrl: '',
      _unlisten: null,
      _eventId: '',
    })
  },

  restoreFromEvent: async (eventId: string) => {
    if (!eventId) return
    const prev = get()
    if (prev._unlisten) return

    try {
      const unlisten = await listen<{
        event_id: string; kind: string; current: number; total: number; percent: number; message: string
      }>('infer-convert-progress', (event) => {
        const p = event.payload
        if (p.event_id !== eventId) return
        if (p.kind === 'progress') {
          set({ pct: p.percent, cur: p.current, tot: p.total })
        }
        if (p.kind === 'progress' || p.kind === 'log') {
          set(s => ({ log: s.log + p.message + '\n' }))
        }
      })
      set({ _unlisten: unlisten, _eventId: eventId })
    } catch {
      set({ loading: false, _eventId: '' })
    }
  },
}))
