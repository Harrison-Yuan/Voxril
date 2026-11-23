import { invoke } from '@tauri-apps/api/core'

/** 迁移项：from/to 为相对工作区根的可读路径 */
export interface MigrationItem {
  from: string
  to: string
  bytes: number
}

export interface LayoutStatus {
  /** 1 = 旧布局，2 = 新布局 */
  version: number
  is_v2: boolean
  workspace: string
  migrated_at: number | null
  mapping_count: number
}

export interface MigrationPreview {
  version: number
  items: MigrationItem[]
  total_bytes: number
  /** 非空表示当前不可迁移 */
  blockers: string[]
}

export interface MigrationResult {
  moved: number
  total_bytes: number
}

export const layoutApi = {
  async getStatus(): Promise<LayoutStatus> {
    return invoke('get_layout_status')
  },

  async preview(): Promise<MigrationPreview> {
    return invoke('preview_layout_migration')
  },

  async migrate(): Promise<MigrationResult> {
    return invoke('migrate_layout')
  },
}
