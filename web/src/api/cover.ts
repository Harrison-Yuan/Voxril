import { invoke } from '@tauri-apps/api/core'

/** 生成配置；不传即由后端使用默认值 */
export interface CoverParams {
  /** 变调（半音） */
  transpose: number
  /** 音高提取算法 */
  f0_method: string
  /** 检索特征比例 */
  index_rate: number
  /** 混音时人声增益（dB） */
  vocal_gain: number
}

export interface CoverVoice {
  voice: string
  /** 该声音下最适合推理的权重标识 */
  model_name: string
  updated_at: number
}

export interface CoverRequest {
  model_name: string
  song_path: string
  params: CoverParams
}

export interface CoverResponse {
  ok: boolean
  message: string
  /** 最终翻唱作品（人声 + 伴奏） */
  mixed_path: string
}

export interface CoverProgress {
  event_id: string
  /** 所属阶段：separate / convert / merge */
  stage: string
  kind: 'progress' | 'log' | string
  percent: number
  message: string
}

export const DEFAULT_COVER_PARAMS: CoverParams = {
  transpose: 0,
  f0_method: 'rmvpe',
  index_rate: 0.75,
  vocal_gain: 6,
}

export const coverApi = {
  /** 按声音聚合后的可选模型列表 */
  async listVoices(): Promise<CoverVoice[]> {
    return invoke<CoverVoice[]>('list_cover_voices')
  },

  /** 读取该声音保存过的生成配置，未保存过返回默认值 */
  async getParams(voice: string): Promise<CoverParams> {
    const res = await invoke<{ params: CoverParams }>('get_cover_params', { voice })
    return res.params
  },

  async saveParams(voice: string, params: CoverParams): Promise<void> {
    await invoke('save_cover_params', { voice, params })
  },

  async cancel(): Promise<void> {
    await invoke('cancel_cover')
  },
}
