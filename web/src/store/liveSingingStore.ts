import { create } from 'zustand'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

interface Metrics {
  state: string
  rms: number
  f0: number
  f0_continuity: number
  hnr: number
  engine_load_ms: number
  detected_key?: string
}

interface LiveSingingState {
  connected: boolean
  loading: boolean
  metrics: Metrics
  log: string
  startError: string | null
  progressPercent: number
  progressMessage: string
  accompanimentKey: string
  f0Profile: F0ProfileData | null
  extractingF0: boolean
  extractingLyrics: boolean

  start: (request: {
    input_device: string
    output_device: string
    model_path: string
    f0_up_key: number
    accompaniment_path: string
    cover_path?: string    // 翻唱人声（RVC 转换结果）
    duck_gain: number
    index_path?: string   // 可选：前端主动传入 index 路径，避免后端扫描选错
    mode?: string         // "correct" | "vc" (默认 vc)
  }) => Promise<{ ok: boolean; error?: string }>
  stop: () => Promise<boolean>
  checkStatus: () => Promise<void>
  reset: () => void
  extractF0: (audioPath: string) => Promise<F0ProfileData | null>
  extractLyrics: (audioPath: string, force?: boolean) => Promise<{ lrcPath: string; lineCount: number } | null>
  searchLyrics: (query: string) => Promise<Array<{ id: number; name: string; artist: string }>>
  downloadLyrics: (songId: number, audioPath: string) => Promise<{ lrcPath: string; lineCount: number }>
  setAccompanimentPause: (paused: boolean) => Promise<void>
  getAccompanimentStatus: () => Promise<boolean>
}

interface F0ProfileData {
  f0_curve: number[]
  sample_rate: number
  hop_length: number
  duration_secs: number
  frame_count: number
  json_path: string
}

let metricUnlisten: UnlistenFn | null = null
let logUnlisten: UnlistenFn | null = null
let stoppedUnlisten: UnlistenFn | null = null
let progressUnlisten: UnlistenFn | null = null
let accKeyUnlisten: UnlistenFn | null = null
let listenersReady = false

async function setupLiveListeners(store: any) {
  if (listenersReady) return
  listenersReady = true

  // 先清除旧 listener
  metricUnlisten?.()
  logUnlisten?.()
  stoppedUnlisten?.()
  progressUnlisten?.()
  accKeyUnlisten?.()

  metricUnlisten = await listen<any>('live-metrics', (event) => {
    try {
      const data = typeof event.payload === 'string'
        ? JSON.parse(event.payload)
        : event.payload
      store.setState((s: LiveSingingState) => ({ metrics: { ...s.metrics, ...data } }))
    } catch { /* ignore */ }
  })

  logUnlisten = await listen<string>('live-singing-log', (event) => {
    const msg = typeof event.payload === 'string' ? event.payload : String(event.payload)
    store.setState((s: LiveSingingState) => {
      const next = s.log + msg + '\n'
      // 日志上限 50KB，超出时截断前半
      if (next.length > 50_000) {
        return { log: next.slice(next.length - 30_000) }
      }
      return { log: next }
    })
  })

  stoppedUnlisten = await listen('live-singing-stopped', () => {
    store.setState({ connected: false, loading: false })
  })

  progressUnlisten = await listen<any>('live-singing-progress', (event) => {
    try {
      const data = typeof event.payload === 'string'
        ? JSON.parse(event.payload)
        : event.payload
      store.setState({ progressPercent: data.percent, progressMessage: data.message })
    } catch { /* ignore */ }
  })

  accKeyUnlisten = await listen<any>('accompaniment-key', (event) => {
    try {
      const data = typeof event.payload === 'string'
        ? JSON.parse(event.payload)
        : event.payload
      store.setState({ accompanimentKey: data.key || '' })
    } catch { /* ignore */ }
  })
}

function teardownLiveListeners() {
  listenersReady = false
  metricUnlisten?.()
  metricUnlisten = null
  logUnlisten?.()
  logUnlisten = null
  stoppedUnlisten?.()
  stoppedUnlisten = null
  progressUnlisten?.()
  progressUnlisten = null
  accKeyUnlisten?.()
  accKeyUnlisten = null
}

export const useLiveSingingStore = create<LiveSingingState>((set) => ({
  connected: false,
  loading: false,
  metrics: { state: 'mute', rms: 0, f0: 0, f0_continuity: 0, hnr: 0, engine_load_ms: 0, detected_key: '' },
  log: '',
  startError: null,
  progressPercent: 0,
  progressMessage: '',
  accompanimentKey: '',
  f0Profile: null,
  extractingF0: false,
  extractingLyrics: false,

  start: async (request: LiveSingingState['start'] extends (req: infer R) => any ? R : never): Promise<{ok: boolean; error?: string}> => {
    set({
      loading: true,
      startError: null,
      progressPercent: 1,
      progressMessage: '正在准备启动直播引擎...',
      log: '',
    })

    const store = useLiveSingingStore
    await setupLiveListeners(store)

    let readyUnlisten: UnlistenFn | null = null
    let errorUnlisten: UnlistenFn | null = null
    let timeoutId: ReturnType<typeof setTimeout> | null = null
    let settled = false

    const cleanupStartWaiters = () => {
      if (timeoutId) {
        clearTimeout(timeoutId)
        timeoutId = null
      }
      readyUnlisten?.()
      readyUnlisten = null
      errorUnlisten?.()
      errorUnlisten = null
    }

    let finishStartWait: ((result: 'ready' | 'error', errorMessage?: string) => void) | null = null
    const waitForStartResult = new Promise<'ready' | 'error'>((resolve) => {
      finishStartWait = (result, errorMessage) => {
        if (settled) return
        settled = true
        if (errorMessage) {
          store.setState({ startError: errorMessage })
        }
        cleanupStartWaiters()
        resolve(result)
      }
    })

    const finish = (result: 'ready' | 'error', errorMessage?: string) => {
      finishStartWait?.(result, errorMessage)
    }

    timeoutId = setTimeout(() => {
      finish('error', '启动超时：后端没有返回就绪状态。请检查模型路径、音频设备权限或查看引擎日志。')
    }, 120_000)

    try {
      readyUnlisten = await listen('live-singing-ready', () => finish('ready'))
      errorUnlisten = await listen<string>('live-singing-error', (event) => {
        const msg = typeof event.payload === 'string' ? event.payload : String(event.payload)
        finish('error', msg)
      })

      await invoke('start_live_singing', { request })

      const result = await waitForStartResult

      if (result === 'ready') {
        set({ loading: false, connected: true, progressPercent: 100, progressMessage: '引擎已就绪' })
        return { ok: true }
      }

      await invoke('stop_live_singing').catch(() => { })
      teardownLiveListeners()
      set({
        loading: false,
        connected: false,
        startError: useLiveSingingStore.getState().startError,
      })
      const error: string = useLiveSingingStore.getState().startError || '启动失败，但后端未返回错误详情'
      return { ok: false, error }
    } catch (e) {
      cleanupStartWaiters()
      teardownLiveListeners()
      await invoke('stop_live_singing').catch(() => { })
      const error = String(e)
      set({ loading: false, connected: false, startError: error, progressMessage: '' })
      return { ok: false, error }
    }
  },

  stop: async () => {
    set({ loading: true })
    try {
      await invoke('stop_live_singing')
      teardownLiveListeners()
      set({ connected: false, loading: false, metrics: { state: 'mute', rms: 0, f0: 0, f0_continuity: 0, hnr: 0, engine_load_ms: 0 }, progressPercent: 0, progressMessage: '' })
      return true
    } catch (e) {
      // 即使 stop 调用失败也强制复位前端状态
      teardownLiveListeners()
      set({ connected: false, loading: false, progressPercent: 0, progressMessage: '' })
      return false
    }
  },

  checkStatus: async () => {
    const currentState = useLiveSingingStore.getState()
    try {
      const active = await invoke<boolean>('get_live_singing_status')
      if (active) {
        // 引擎已就绪
        set({ connected: true, loading: false })
        const store = useLiveSingingStore
        setupLiveListeners(store)
      } else if (!currentState.loading) {
        // 只有不在加载中时，才把状态复位为离线
        set({ connected: false })
      }
      // loading=true 时保持原状态，切页回来继续显示进度
    } catch {
      if (!currentState.loading) {
        set({ connected: false })
      }
    }
  },

  reset: () => {
    teardownLiveListeners()
    set({
      connected: false,
      loading: false,
      metrics: { state: 'mute', rms: 0, f0: 0, f0_continuity: 0, hnr: 0, engine_load_ms: 0 },
      log: '',
      startError: null,
      progressPercent: 0,
      progressMessage: '',
      accompanimentKey: '',
    })
  },

  extractF0: async (audioPath: string) => {
    set({ extractingF0: true })
    try {
      const profile = await invoke<F0ProfileData>('extract_f0_profile', { audioPath })
      set({ f0Profile: profile, extractingF0: false })
      return profile
    } catch (e) {
      set({ extractingF0: false })
      throw e
    }
  },

  extractLyrics: async (audioPath: string, force?: boolean) => {
    set({ extractingLyrics: true })
    try {
      const result = await invoke<{ lrc_path: string; line_count: number }>('extract_lyrics', { audioPath, force: force ?? false })
      set({ extractingLyrics: false })
      return { lrcPath: result.lrc_path, lineCount: result.line_count }
    } catch (e) {
      set({ extractingLyrics: false })
      throw e
    }
  },

  searchLyrics: async (query: string) => {
    const results = await invoke<Array<{ id: number; name: string; artist: string }>>('search_lyrics', { query })
    return results
  },

  downloadLyrics: async (songId: number, audioPath: string) => {
    const result = await invoke<{ lrc_path: string; line_count: number }>('download_lyrics', { songId, audioPath })
    return { lrcPath: result.lrc_path, lineCount: result.line_count }
  },

  setAccompanimentPause: async (paused: boolean) => {
    await invoke('set_accompaniment_pause', { paused })
  },

  getAccompanimentStatus: async () => {
    return await invoke<boolean>('get_accompaniment_status')
  },
}))
