import { invoke } from '@tauri-apps/api/core'

export interface EnvStatus {
  /** Python 依赖是否安装完成（应用级 venv 是否存在） */
  venv_ready: boolean
}

export const envApi = {
  async getStatus(): Promise<EnvStatus> {
    return invoke<EnvStatus>('get_env_status')
  },
}
