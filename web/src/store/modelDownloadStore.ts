import { create } from 'zustand'

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

  startDownload: (id: string, label?: string) => set(s => {
    if (s.downloads.some(d => d.id === id)) return s
    return { downloads: [...s.downloads, { id, label: label || id, log: '准备下载...', percent: -1, downloadedMb: 0 }] }
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
