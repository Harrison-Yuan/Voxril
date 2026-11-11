import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type { UnlistenFn } from '@tauri-apps/api/event'

export interface SeparateRequest {
  model_name: string
  input_path: string
  vocal_dir: string
  instrumental_dir: string
  agg: number
  format: string
}

export interface SeparateResponse {
  ok: boolean
  message: string
  vocal_dir: string
  instrumental_dir: string
  vocal_files: string[]
  instrumental_files: string[]
}

export interface SeparateProgress {
  kind: 'progress' | 'log'
  step: 'file' | 'segment' | ''
  current: number
  total: number
  percent: number
  message: string
}

export const uvrApi = {
  async separateVocals(request: SeparateRequest): Promise<SeparateResponse> {
    try {
      const result = await invoke<SeparateResponse>('separate_vocals', { request })
      return result
    } catch (error) {
      return {
        ok: false,
        message: String(error),
        vocal_dir: request.vocal_dir,
        instrumental_dir: request.instrumental_dir,
        vocal_files: [],
        instrumental_files: [],
      }
    }
  },

  async separateVocalsStreaming(
    request: SeparateRequest,
    onProgress: (progress: SeparateProgress) => void
  ): Promise<SeparateResponse> {
    const eventId = `uvr-${Date.now()}`

    let unlisten: UnlistenFn | null = null
    try {
      unlisten = await listen<{ event_id: string; kind: string; step: string; current: number; total: number; percent: number; message: string }>(
        'uvr-separate-progress',
        (event) => {
          const p = event.payload
          if (p.event_id === eventId && (p.kind === 'progress' || p.kind === 'log')) {
            onProgress({
              kind: p.kind as 'progress' | 'log',
              step: p.step as 'file' | 'segment' | '',
              current: p.current,
              total: p.total,
              percent: p.percent,
              message: p.message,
            })
          }
        }
      )

      const result = await invoke<SeparateResponse>('separate_vocals_streaming', { request, eventId })
      return result
    } catch (error) {
      return {
        ok: false,
        message: String(error),
        vocal_dir: request.vocal_dir,
        instrumental_dir: request.instrumental_dir,
        vocal_files: [],
        instrumental_files: [],
      }
    } finally {
      if (unlisten) unlisten()
    }
  },
}
