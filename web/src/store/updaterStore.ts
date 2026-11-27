import { create } from 'zustand'
import { check as checkUpdater, type Update } from '@tauri-apps/plugin-updater'
import { getVersion } from '@tauri-apps/api/app'
import { envApi } from '@/api/env'

export type UpdaterPhase = 'idle' | 'checking' | 'available' | 'downloading' | 'ready' | 'uptodate' | 'error'

/** 用户「忽略此版本」的记录，避免每次启动都提示同一版本 */
const IGNORED_KEY = 'voxril-ignored-version'
/** 检查超时：GitHub 直连在国内经常挂起，不设超时会一直停在「检查中」 */
const CHECK_TIMEOUT_MS = 15_000

/** 当前待安装的更新；Update 持有 Rust 侧资源，只放模块级变量，不进 store state */
let pending: Update | null = null
/** StrictMode 下 effect 会执行两次，自动检查只跑一次 */
let bootstrapped = false

/**
 * 首启向导是否还在等用户确认数据目录。
 * 这段时间不自动弹更新面板：两个模态叠在一起会盖掉首启引导，
 * 而且此时重启安装会打断环境初始化。红点提示仍然保留。
 */
async function inFirstRunSetup(): Promise<boolean> {
  try {
    const status = await envApi.getDataDirStatus()
    return status.writable && !status.confirmed && !status.has_data
  } catch {
    return false
  }
}

interface UpdaterState {
  phase: UpdaterPhase
  /** 当前运行的版本号（「已是最新版本」文案要用） */
  currentVersion: string
  /** 新版本号（available 及之后有值） */
  version: string
  /** 更新说明（Release 正文） */
  notes: string
  error: string
  /** 已下载 / 总字节 */
  downloaded: number
  total: number
  /** 面板是否展开（由标题栏图标控制） */
  panelOpen: boolean
  /** 该版本已被用户「忽略」：不再主动打扰（不弹面板、不显红点） */
  dismissed: boolean
  bootstrap: () => void
  check: (manual?: boolean) => Promise<void>
  download: () => Promise<void>
  install: () => Promise<void>
  openPanel: () => void
  closePanel: () => void
  ignore: () => void
}

/**
 * 应用内更新：检查 GitHub Release 的 latest.json，下载签名更新包后再安装。
 *
 * 与「打开浏览器手动下载」的区别：这里是真正的静默下载 + 安装，
 * 签名由 tauri-plugin-updater 用内置公钥校验，被篡改的包会被拒绝。
 */
export const useUpdaterStore = create<UpdaterState>((set, get) => ({
  phase: 'idle',
  currentVersion: '',
  version: '',
  notes: '',
  error: '',
  downloaded: 0,
  total: 0,
  panelOpen: false,
  dismissed: false,

  /** 启动后延迟检查一次；失败静默（离线、接口不可达时不打扰用户） */
  bootstrap: () => {
    if (bootstrapped) return
    bootstrapped = true
    setTimeout(() => {
      void get().check(false)
    }, 3000)
  },

  check: async (manual = true) => {
    // 正在检查 / 已下载待安装时不要重复发起，否则并发请求会覆盖 pending
    const phase = get().phase
    if (phase === 'checking' || phase === 'downloading' || phase === 'ready') return
    set({ phase: 'checking', error: '' })
    if (!get().currentVersion) {
      const currentVersion = await getVersion().catch(() => '')
      if (currentVersion) set({ currentVersion })
    }
    try {
      const update = await checkUpdater({ timeout: CHECK_TIMEOUT_MS })
      pending?.close().catch(() => {})
      pending = update
      if (!update) {
        set({
          phase: 'uptodate',
          version: '',
          notes: '',
          dismissed: false,
          panelOpen: manual ? true : get().panelOpen,
        })
        return
      }
      const dismissed = localStorage.getItem(IGNORED_KEY) === update.version
      let autoPanel = manual || !dismissed
      if (!manual && autoPanel) autoPanel = !(await inFirstRunSetup())
      set({
        phase: 'available',
        version: update.version,
        notes: update.body ?? '',
        downloaded: 0,
        total: 0,
        dismissed,
        // 自动检查时，若该版本已被忽略或首启向导还在路上，就只在图标上留提示
        panelOpen: autoPanel,
      })
    } catch (e) {
      // 自动检查失败完全静默：国内网络访问 GitHub 经常超时，不该报错打扰
      if (!manual) {
        set({ phase: 'idle' })
        return
      }
      set({ phase: 'error', error: e instanceof Error ? e.message : String(e) })
    }
  },

  download: async () => {
    if (!pending || get().phase === 'downloading') return
    if (!get().panelOpen) set({ panelOpen: true })
    set({ phase: 'downloading', downloaded: 0, total: 0, error: '', dismissed: false })
    try {
      await pending.download((event) => {
        if (event.event === 'Started') {
          set({ total: event.data.contentLength ?? 0 })
        } else if (event.event === 'Progress') {
          set((s) => ({ downloaded: s.downloaded + event.data.chunkLength }))
        }
      })
      set({ phase: 'ready' })
    } catch (e) {
      set({ phase: 'error', error: e instanceof Error ? e.message : String(e) })
    }
  },

  install: async () => {
    if (!pending) return
    try {
      // 不让安装器自行重启（Windows 默认会），统一走 restart_app，
      // 避免两条重启路径互相竞争导致拉起两个实例
      await pending.install({ restartAfterInstall: false })
      await envApi.restart()
    } catch (e) {
      set({ phase: 'error', error: e instanceof Error ? e.message : String(e) })
    }
  },

  openPanel: () => set({ panelOpen: true }),
  closePanel: () => set({ panelOpen: false }),

  ignore: () => {
    const version = get().version
    if (version) localStorage.setItem(IGNORED_KEY, version)
    set({ dismissed: true, panelOpen: false })
  },
}))
