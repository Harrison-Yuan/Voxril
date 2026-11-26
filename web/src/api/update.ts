import { invoke } from '@tauri-apps/api/core'

export interface UpdateInfo {
  current: string
  latest: string
  /** 最新 Release 页面地址 */
  url: string
  /** Release 说明（Markdown） */
  notes: string
  has_update: boolean
}

export const updateApi = {
  /** 读取 GitHub 最新 Release 并与当前版本比较；失败时抛错（由调用方静默处理）。 */
  async check(): Promise<UpdateInfo> {
    return invoke<UpdateInfo>('check_update')
  },

  /** 用系统浏览器打开链接（仅允许 https）。 */
  async open(url: string): Promise<void> {
    await invoke('open_url', { url })
  },
}
