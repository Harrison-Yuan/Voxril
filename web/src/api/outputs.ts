import { invoke } from '@tauri-apps/api/core'

export interface OutputItem {
  id: number
  /** converted | mixed */
  kind: string
  path: string
  song: string
  /** 无法从文件名识别声音时为空串 */
  voice: string
  run_id: string | null
  transpose: number
  created_at: number
  bytes: number
  /** WAV 时长（秒）；不可解析时为 null */
  duration_secs: number | null
  starred: boolean
  /** 已定稿：位于 outputs/final，不参与清理 */
  finalized: boolean
}

export interface FinalizeResponse {
  path: string
}

export const outputsApi = {
  async list(): Promise<OutputItem[]> {
    return invoke('list_outputs')
  },

  /** 定稿：移入 outputs/final，不再自动清理 */
  async finalize(id: number): Promise<FinalizeResponse> {
    return invoke('finalize_output', { id })
  },

  async setStarred(id: number, starred: boolean): Promise<void> {
    await invoke('set_output_starred', { id, starred })
  },

  /** 删除成品（连同 .stft.npy 旁路缓存移入回收站） */
  async remove(path: string): Promise<{ freed_bytes: number }> {
    return invoke('delete_output', { path })
  },
}
