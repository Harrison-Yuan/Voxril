import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

export interface EnvStatus {
  /** Python 依赖是否安装完成（应用级 venv 是否存在） */
  venv_ready: boolean
}

export interface DataDirStatus {
  /** 当前数据目录（Python 环境、基础模型、配置） */
  path: string
  /** 是否可写；不可写时需要用户另选目录 */
  writable: boolean
  /** 用户是否已明确确认过位置（选过或迁移过） */
  confirmed: boolean
  /** 目录里是否已有环境与模型（升级用户不再走首启引导） */
  has_data: boolean
  /** 所在卷剩余字节，取不到为 null */
  free_bytes: number | null
  /** 建议预留字节 */
  recommended_bytes: number
}

export interface MigrateProgress {
  percent: number
  message: string
}

export const envApi = {
  async getStatus(): Promise<EnvStatus> {
    return invoke<EnvStatus>('get_env_status')
  },

  async getDataDirStatus(): Promise<DataDirStatus> {
    return invoke<DataDirStatus>('get_data_dir_status')
  },

  /** 保存用户选择的数据目录，重启后生效 */
  async setDataDir(path: string): Promise<void> {
    await invoke('set_data_dir', { path })
  },

  /** 首启确认数据目录；返回 true 表示改选了别处，需要重启才生效 */
  async confirmDataDir(path: string): Promise<boolean> {
    return invoke<boolean>('confirm_data_dir', { path })
  },

  /** 查询某个目录所在卷的剩余空间（首启向导改选目录时用） */
  async getDirFreeSpace(path: string): Promise<number | null> {
    return invoke<number | null>('get_dir_free_space', { path })
  },

  /** 把现有环境与模型复制到新目录，成功后立即清理旧目录并写入新指针，重启生效 */
  async migrateDataDir(target: string, onProgress: (progress: MigrateProgress) => void): Promise<string> {
    const unlisten = await listen<MigrateProgress>('data-migrate-progress', (event) => onProgress(event.payload))
    try {
      return await invoke<string>('migrate_data_dir', { target })
    } finally {
      unlisten()
    }
  },

  async restart(): Promise<void> {
    await invoke('restart_app')
  },
}
