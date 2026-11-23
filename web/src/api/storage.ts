import { invoke } from '@tauri-apps/api/core'

export interface StorageZone {
  key: string
  label: string
  bytes: number
  note: string
}

export interface StorageReport {
  workspace: string
  zones: StorageZone[]
  /** 一键“释放空间”可立刻拿回的字节数（R1） */
  reclaimable_now: number
}

export interface ReclaimAllResponse {
  freed_bytes: number
  batches: number
}

export interface DeepCleanResponse {
  freed_gb: number
  message: string
}

export const storageApi = {
  async getReport(): Promise<StorageReport> {
    return invoke('get_storage_report')
  },

  /** 对所有批次执行 R1 回收（1_16k_wavs / eval / tfevents / 孤立 STFT） */
  async reclaimAll(): Promise<ReclaimAllResponse> {
    return invoke('reclaim_all_runs')
  },

  /** 清理构建缓存（src-tauri/target、web/dist 等） */
  async deepClean(): Promise<DeepCleanResponse> {
    return invoke('deep_clean_cache')
  },
}
