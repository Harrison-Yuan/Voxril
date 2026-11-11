import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type { UnlistenFn } from '@tauri-apps/api/event'

export interface ConvertRequest {
  model_name: string
  source_path: string
  transpose: number
  f0_method: string
  index_rate: number
  filter_radius: number
  resample_sr: number
  rms_mix_rate: number
  protect: number
}

export interface ConvertResponse {
  ok: boolean
  message: string
  output_path: string
}

export interface MergeResponse {
  ok: boolean
  message: string
  output_path: string
}

export interface ConvertProgress {
  kind: 'progress' | 'log'
  current: number
  total: number
  percent: number
  message: string
}

export interface BaseModelStatus {
  id: string
  name: string
  exists: boolean
  size_mb: number
}

export interface SeparatedSong {
  name: string
  instrumental_path: string
  vocal_path: string
  f0_path: string
  f0_exists: boolean
  lrc_exists: boolean
  lrc_path: string
  converted_path: string
  stft_exists: boolean
}

export const inferApi = {
  async convertVoiceStreaming(
    request: ConvertRequest,
    onProgress: (progress: ConvertProgress) => void
  ): Promise<ConvertResponse> {
    const eventId = `infer-${Date.now()}`

    let unlisten: UnlistenFn | null = null
    try {
      unlisten = await listen<{ event_id: string; kind: string; current: number; total: number; percent: number; message: string }>(
        'infer-convert-progress',
        (event) => {
          const p = event.payload
          if (p.event_id === eventId && (p.kind === 'progress' || p.kind === 'log')) {
            onProgress({
              kind: p.kind as 'progress' | 'log',
              current: p.current,
              total: p.total,
              percent: p.percent,
              message: p.message,
            })
          }
        }
      )

      const result = await invoke<ConvertResponse>('convert_voice_streaming', { request, eventId })
      return result
    } catch (error) {
      return {
        ok: false,
        message: String(error),
        output_path: '',
      }
    } finally {
      if (unlisten) unlisten()
    }
  },

  async mergeAudio(
    vocalPath: string,
    instrumentalPath: string,
    outputDir: string,
    vocalGain: number = 1.0
  ): Promise<MergeResponse> {
    return invoke('merge_audio', {
      vocalPath,
      instrumentalPath,
      outputDir,
      vocalGain,
    })
  },

  async listModels(): Promise<string[]> {
    return invoke('list_models')
  },

  async deleteModel(modelName: string): Promise<boolean> {
    return invoke('delete_model', { modelName })
  },

  async deleteBaseModel(modelId: string): Promise<boolean> {
    return invoke('delete_base_model', { modelId })
  },

  async getModelStoragePaths(): Promise<string[]> {
    return invoke('get_model_storage_paths')
  },

  async importModel(
    source: string,
    modelName: string,
    onProgress: (progress: ConvertProgress) => void
  ): Promise<ConvertResponse> {
    const eventId = `import-${Date.now()}`

    let unlisten: UnlistenFn | null = null
    try {
      unlisten = await listen<{ event_id: string; kind: string; current: number; total: number; percent: number; message: string }>(
        'model-import-progress',
        (event) => {
          const p = event.payload
          if (p.event_id === eventId) {
            onProgress({
              kind: p.kind as 'progress' | 'log',
              current: p.current,
              total: p.total,
              percent: p.percent,
              message: p.message,
            })
          }
        }
      )

      return await invoke<ConvertResponse>('import_model', {
        request: { source, model_name: modelName },
        eventId,
      })
    } catch (error) {
      return { ok: false, message: String(error), output_path: '' }
    } finally {
      if (unlisten) unlisten()
    }
  },

  async getBaseModelsStatus(): Promise<BaseModelStatus[]> {
    return invoke('get_base_models_status')
  },

  async downloadBaseModel(
    modelId: string,
    onProgress: (progress: ConvertProgress) => void
  ): Promise<boolean> {
    const eventId = `download-${Date.now()}`
    let unlisten: UnlistenFn | null = null
    try {
      unlisten = await listen<{ event_id: string; kind: string; current: number; total: number; percent: number; message: string }>(
        'base-model-download-progress',
        (event) => {
          const p = event.payload
          if (p.event_id === eventId) {
            onProgress({
              kind: p.kind as 'progress' | 'log',
              current: p.current,
              total: p.total,
              percent: p.percent,
              message: p.message,
            })
          }
        }
      )
      return await invoke<boolean>('download_base_model', {
        modelId,
        eventId,
      })
    } catch (error) {
      // 抛出原始错误，让调用方 catch 并展示
      throw new Error(String(error))
    } finally {
      if (unlisten) unlisten()
    }
  },

  async listLiveModels(): Promise<{ name: string; path: string; kind: string; format: string; exists: boolean }[]> {
    return invoke('list_live_models')
  },

  async exportPthToOnnx(sourcePath: string, outputPath: string): Promise<void> {
    return invoke('export_pth_to_onnx', { sourcePath, outputPath })
  },

  async listSeparatedSongs(): Promise<SeparatedSong[]> {
    return invoke('list_separated_songs')
  },
}
