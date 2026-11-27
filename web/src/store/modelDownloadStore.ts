import { create } from 'zustand'
import { listen } from '@tauri-apps/api/event'
import { t } from '@/i18n/runtime'

export interface DownloadItem {
  id: string
  label: string
  log: string
  /** 0-100, -1 = unknown */
  percent: number
  /** MB downloaded so far, 0 = unknown */
  downloadedMb: number
}

interface ModelDownloadState {
  downloads: DownloadItem[]
  /** 最近一次模型下载完成的时间戳，供页面刷新模型状态 */
  completedAt: number

  startDownload: (id: string, label?: string) => void
  updateLog: (id: string, log: string) => void
  updateProgress: (id: string, percent: number, downloadedMb: number) => void
  finishDownload: (id: string) => void
  isDownloading: (id: string) => boolean
  getDownloadItem: (id: string) => DownloadItem | undefined
  getDownloadingCount: () => number
}

export const useModelDownloadStore = create<ModelDownloadState>((set, get) => ({
  downloads: [],
  completedAt: 0,

  startDownload: (id: string, label?: string) => set(s => {
    if (s.downloads.some(d => d.id === id)) return s
    return { downloads: [...s.downloads, { id, label: label || id, log: t('shell.download_preparing'), percent: -1, downloadedMb: 0 }] }
  }),

  updateLog: (id: string, log: string) => set(s => ({
    downloads: s.downloads.map(d => d.id === id ? { ...d, log } : d)
  })),

  updateProgress: (id: string, percent: number, downloadedMb: number) => set(s => ({
    downloads: s.downloads.map(d => d.id === id ? { ...d, percent, downloadedMb } : d)
  })),

  finishDownload: (id: string) => set(s => ({
    downloads: s.downloads.filter(d => d.id !== id)
  })),

  isDownloading: (id: string) => get().downloads.some(d => d.id === id),

  getDownloadItem: (id: string) => get().downloads.find(d => d.id === id),

  getDownloadingCount: () => get().downloads.length,
}))

let syncStarted = false

/**
 * 全局订阅模型下载日志（启动自检与手动下载共用同一个事件）。
 *
 * 日志带 `model_id`，因此无论下载是谁发起的，只要用户进入模型页（或恰好
 * 停留在页面上），对应模型条目都会自动出现并实时更新进度。
 */
export function initModelDownloadSync(): Promise<void> {
  if (syncStarted) return Promise.resolve()
  syncStarted = true
  return listen<{ event_id: string | null; model_id?: string | null; message: string }>(
    'model-download-log',
    (event) => {
      const { model_id: modelId, message } = event.payload
      if (!modelId) return

      const store = useModelDownloadStore.getState()
      if (!store.getDownloadItem(modelId)) store.startDownload(modelId)
      store.updateLog(modelId, message)

      const pctMatch = message.match(/(\d+)%/)
      const mbMatch = message.match(/(\d+(?:\.\d+)?)MB\)/)
      const percent = pctMatch ? parseInt(pctMatch[1], 10) : -1
      const mb = mbMatch ? parseFloat(mbMatch[1]) : 0

      // '下载完成' 是后端（tools/check_download_models.py 打印的日志）协议字面量，
      // 只用于匹配原始输出，不参与 i18n，改成 t() 会让进度判定失效
      if (message.includes('下载完成')) {
        store.updateProgress(modelId, 100, mb)
        store.finishDownload(modelId)
        useModelDownloadStore.setState({ completedAt: Date.now() })
      } else if (percent >= 0 || mb > 0) {
        store.updateProgress(modelId, percent, mb)
      }
    }
  ).then(() => undefined)
}
