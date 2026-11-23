import { invoke } from '@tauri-apps/api/core'

export type DownloadSource = 'auto' | 'official' | 'cn' | 'custom'

export interface DownloadSettings {
  /** auto | official | cn | custom */
  source: DownloadSource
  /** 自定义 HuggingFace 端点，例如 https://hf-mirror.com */
  hf_endpoint: string
  /** 自定义 GitHub 加速前缀，例如 https://ghfast.top */
  gh_proxy: string
}

export const downloadApi = {
  async get(): Promise<DownloadSettings> {
    return invoke('get_download_settings')
  },

  async set(settings: DownloadSettings): Promise<void> {
    await invoke('set_download_settings', { settings })
  },
}
