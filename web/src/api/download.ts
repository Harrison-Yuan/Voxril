import { invoke } from '@tauri-apps/api/core'

export type DownloadSource = 'auto' | 'official' | 'cn' | 'custom'
export type PipSource = 'auto' | 'official' | 'tuna' | 'aliyun' | 'custom'

export interface DownloadSettings {
  /** 模型下载源：auto | official | cn | custom */
  source: DownloadSource
  /** 自定义 HuggingFace 端点，例如 https://hf-mirror.com */
  hf_endpoint: string
  /** 自定义 GitHub 加速前缀，例如 https://ghfast.top */
  gh_proxy: string
  /** pip 源：auto | official | tuna | aliyun | custom */
  pip_source: PipSource
  /** 自定义 PyPI 索引地址 */
  pip_index: string
}

export const downloadApi = {
  async get(): Promise<DownloadSettings> {
    return invoke('get_download_settings')
  },

  async set(settings: DownloadSettings): Promise<void> {
    await invoke('set_download_settings', { settings })
  },
}
