import { invoke } from '@tauri-apps/api/core'

export interface TrainConfig {
  exp_dir: string
  sr: string
  trainset_dir: string
  np_process: number
  f0_method: string
  if_f0: boolean
  save_epoch: number
  total_epoch: number
  batch_size: number
  if_save_latest: boolean
  if_save_every_weights: boolean
  gpus: string
}

export interface TrainResponse {
  ok: boolean
  message: string
}

export interface TrainStatusResponse {
  is_running: boolean
  pid?: string
  current_epoch: number
  total_epoch: number
  latest_loss: string
  percent: number
}

export const trainApi = {
  async preprocessDataset(
    trainsetDir: string,
    expDir: string,
    sr: string,
    npProcess: number
  ): Promise<TrainResponse> {
    return invoke('preprocess_dataset', {
      trainsetDir,
      expDir,
      sr,
      npProcess,
    })
  },

  async extractF0Features(
    npProcess: number,
    f0Method: string,
    expDir: string
  ): Promise<TrainResponse> {
    return invoke('extract_f0_features', {
      npProcess,
      f0Method,
      expDir,
    })
  },

  async extractAudioFeatures(
    gpus: string,
    expDir: string
  ): Promise<TrainResponse> {
    return invoke('extract_audio_features', {
      gpus,
      expDir,
    })
  },

  async trainModel(config: TrainConfig): Promise<TrainResponse> {
    return invoke('train_model', { config })
  },

  async stopTraining(expDir: string): Promise<TrainResponse> {
    return invoke('stop_training', { expDir })
  },

  async trainIndex(expDir: string): Promise<TrainResponse> {
    return invoke('train_index', { expDir })
  },

  async getTrainStatus(expDir: string): Promise<TrainStatusResponse> {
    return invoke('get_train_status', { expDir })
  },

  async listExperiments(): Promise<string[]> {
    return invoke('list_experiments')
  },

  async selectDirectory(title: string): Promise<string> {
    const { open } = await import('@tauri-apps/plugin-dialog')
    const selected = await open({
      directory: true,
      multiple: false,
      title,
    })
    return selected || ''
  },

  async readLocalText(path: string): Promise<string> {
    return invoke('read_text', { path })
  },
}
