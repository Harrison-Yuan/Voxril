import { invoke } from '@tauri-apps/api/core'

export interface ExperimentConfigDTO {
  exp_dir: string
  sr: string
  if_f0: boolean
  trainset_dir: string
  spk_id: number
  gpus: string
  np_process: number
  f0_method: string
  save_epoch: number
  total_epoch: number
  batch_size: number
  if_save_latest: boolean
  if_cache_gpu: boolean
  if_save_every_weights: boolean
  version: string
  created_at?: number
  updated_at?: number
}

export interface TrainingHistoryDTO {
  id?: number
  exp_dir: string
  version: string
  sr: string
  f0_method: string
  total_epoch: number
  completed_epoch: number
  status: string
  start_time: number
  end_time?: number
  checkpoint_path?: string
  index_path?: string
  config?: string
  notes?: string
}

export interface WorkflowStatusDTO {
  exp_dir: string
  preprocess: string
  features: string
  train: string
  index_status: string
  train_epoch: number
  total_epoch: number
  updated_at?: number
}

export const configApi = {
  async get(expDir: string): Promise<ExperimentConfigDTO | null> {
    try {
      const result = await invoke<{ config: ExperimentConfigDTO | null }>('get_config', {
        expDir,
      })
      return result.config
    } catch {
      return null
    }
  },

  async getAll(): Promise<ExperimentConfigDTO[]> {
    try {
      const result = await invoke<{ configs: ExperimentConfigDTO[] }>('get_all_configs')
      return result.configs
    } catch {
      return []
    }
  },

  async save(config: ExperimentConfigDTO): Promise<void> {
    await invoke('save_config', { config })
  },

  async delete(expDir: string): Promise<void> {
    await invoke('delete_config', { expDir })
  },

  async getLastExperiment(): Promise<string | null> {
    try {
      const result = await invoke<{ exp_dir: string | null }>('get_last_experiment')
      return result.exp_dir
    } catch {
      return null
    }
  },

  async setLastExperiment(expDir: string): Promise<void> {
    await invoke('set_last_experiment', { expDir })
  },
}

export const historyApi = {
  async getAll(): Promise<TrainingHistoryDTO[]> {
    try {
      const result = await invoke<{ history: TrainingHistoryDTO[] }>('get_all_history')
      return result.history
    } catch {
      return []
    }
  },

  async add(record: TrainingHistoryDTO): Promise<number> {
    const result = await invoke<{ id: number }>('add_history', { record })
    return result.id
  },

  async update(id: number, updates: Partial<TrainingHistoryDTO>): Promise<void> {
    await invoke('update_history', { id, updates })
  },

  async delete(id: number): Promise<void> {
    await invoke('delete_history', { id })
  },

  async deleteAll(): Promise<void> {
    await invoke('delete_all_history')
  },
}

export const workflowApi = {
  async get(expDir: string): Promise<WorkflowStatusDTO | null> {
    try {
      const result = await invoke<{ status: WorkflowStatusDTO | null }>('get_workflow', {
        expDir,
      })
      return result.status
    } catch {
      return null
    }
  },

  async save(status: WorkflowStatusDTO): Promise<void> {
    await invoke('save_workflow', { status })
  },

  async delete(expDir: string): Promise<void> {
    await invoke('delete_workflow', { expDir })
  },
}

export const workspaceApi = {
  async get(): Promise<string> {
    const result = await invoke<{ workspace: string }>('get_workspace')
    return result.workspace
  },

  async select(): Promise<string | null> {
    const result = await invoke<string | null>('select_workspace')
    return result
  },

  async set(workspace: string): Promise<{ workspace: string }> {
    const result = await invoke<{ workspace: string }>('set_workspace', {
      workspace,
    })
    return result
  },
}
