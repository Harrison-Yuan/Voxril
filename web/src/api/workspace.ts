import { invoke } from '@tauri-apps/api/core'

export interface WorkspaceResponse {
  workspace: string
}

export const workspaceApi = {
  async getWorkspace(): Promise<WorkspaceResponse> {
    return invoke('get_workspace')
  },

  /** 数据集根目录（分离后的人声/伴奏所在目录），由后端按布局解析 */
  async getDatasetsRoot(): Promise<string> {
    return invoke('get_datasets_root')
  },

  async setWorkspace(workspace: string): Promise<WorkspaceResponse> {
    return invoke('set_workspace', { workspace })
  },

  async selectWorkspace(): Promise<string | null> {
    return invoke('select_workspace')
  },

  async openInFinder(path: string): Promise<void> {
    await invoke('open_in_finder', { path })
  },
}
