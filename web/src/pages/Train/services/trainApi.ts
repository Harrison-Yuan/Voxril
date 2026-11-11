import { invoke } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-dialog'

export interface ApiResponse {
  ok: boolean
  message: string
}

export interface TrainIndexResponse extends ApiResponse {}

export interface PreprocessResponse extends ApiResponse {}

export interface ExtractFeaturesResponse extends ApiResponse {}

export interface TrainModelResponse extends ApiResponse {}

export interface TrainConfig {
  exp_dir: string
  trainset_dir: string
  sr: string
  np_process: number
  f0_method: string
  if_f0: boolean
  spk_id: number
  save_epoch: number
  total_epoch: number
  batch_size: number
  if_save_latest: boolean
  gpus: string
  if_cache_gpu: boolean
  if_save_every_weights: boolean
}

export const trainApi = {
  async existsByBackend(path: string): Promise<boolean> {
    try {
      const result = await invoke<{ exists: boolean }>('file_exists', { path })
      return result.exists
    } catch {
      return false
    }
  },

  async getArtifactMatches(dir: string, kind: 'index' | 'checkpoint'): Promise<string[]> {
    try {
      const result = await invoke<{ files: string[] }>('list_artifacts', { dir, kind })
      return result.files
    } catch {
      return []
    }
  },

  async getCheckpointIteration(path: string): Promise<number> {
    try {
      const result = await invoke<{ iteration: number }>('get_checkpoint_iteration', { path })
      return result.iteration
    } catch {
      return 0
    }
  },

  async readLocalText(path: string): Promise<string> {
    try {
      const result = await invoke<{ content: string }>('read_text_file', { path })
      return result.content
    } catch {
      return ''
    }
  },

  async listExperiments(): Promise<string[]> {
    try {
      const result = await invoke<string[]>('list_experiments')
      return result
    } catch {
      return []
    }
  },

  async deleteFile(path: string): Promise<void> {
    await invoke('delete_file', { path })
  },

  async trainIndex(expDir: string): Promise<TrainIndexResponse> {
    try {
      const result = await invoke<ApiResponse>('train_index', {
        expDir,
      })
      return result
    } catch (error) {
      return {
        ok: false,
        message: String(error),
      }
    }
  },

  async preprocessDataset(
    trainsetDir: string,
    expDir: string,
    sr: string,
    npProcess: number
  ): Promise<PreprocessResponse> {
    try {
      const result = await invoke<ApiResponse>('preprocess_dataset', {
        trainsetDir,
        expDir,
        sr,
        npProcess,
      })
      return result
    } catch (error) {
      return {
        ok: false,
        message: String(error),
      }
    }
  },

  async extractF0Features(
    npProcess: number,
    f0Method: string,
    expDir: string
  ): Promise<ExtractFeaturesResponse> {
    try {
      const result = await invoke<ApiResponse>('extract_f0_features', {
        npProcess,
        f0Method,
        expDir,
      })
      return result
    } catch (error) {
      return {
        ok: false,
        message: String(error),
      }
    }
  },

  async extractAudioFeatures(
    gpus: string,
    expDir: string
  ): Promise<ExtractFeaturesResponse> {
    try {
      const result = await invoke<ApiResponse>('extract_audio_features', {
        gpus,
        expDir,
      })
      return result
    } catch (error) {
      return {
        ok: false,
        message: String(error),
      }
    }
  },

  async trainModel(params: {
    expDir: string
    trainsetDir: string
    sr: string
    npProcess: number
    f0Method: string
    ifF0: boolean
    spkId: number
    saveEpoch: number
    totalEpoch: number
    batchSize: number
    ifSaveLatest: boolean
    gpus: string
    ifCacheGpu: boolean
    ifSaveEveryWeights: boolean
  }): Promise<TrainModelResponse> {
    try {
      const config: TrainConfig = {
        exp_dir: params.expDir,
        trainset_dir: params.trainsetDir,
        np_process: params.npProcess,
        f0_method: params.f0Method,
        sr: params.sr,
        if_f0: params.ifF0,
        spk_id: params.spkId,
        save_epoch: params.saveEpoch,
        total_epoch: params.totalEpoch,
        batch_size: params.batchSize,
        if_save_latest: params.ifSaveLatest,
        gpus: params.gpus,
        if_cache_gpu: params.ifCacheGpu,
        if_save_every_weights: params.ifSaveEveryWeights,
      }
      const result = await invoke<ApiResponse>('train_model', { config })
      return result
    } catch (error) {
      return {
        ok: false,
        message: String(error),
      }
    }
  },

  async stopTraining(expDir: string): Promise<ApiResponse> {
    try {
      const result = await invoke<ApiResponse>('stop_training', { expDir })
      return result
    } catch (error) {
      return {
        ok: false,
        message: String(error),
      }
    }
  },

  async selectDirectory(title: string): Promise<string> {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title,
      })
      if (!selected) return ''
      if (Array.isArray(selected)) return selected[0] || ''
      return selected
    } catch (error) {
      console.error('选择目录失败:', error)
      return ''
    }
  },

  async getTrainStatus(expDir: string): Promise<{
    isRunning: boolean
    pid: string | null
    currentEpoch: number
    totalEpoch: number
    latestLoss: string
    percent: number
    logTail: string
    errorMessage: string
  }> {
    try {
      const result = await invoke<{
        is_running: boolean
        pid: string | null
        current_epoch: number
        total_epoch: number
        latest_loss: string
        percent: number
        log_tail: string
        error_message: string
      }>('get_train_status', { expDir })
      return {
        isRunning: result.is_running,
        pid: result.pid,
        currentEpoch: result.current_epoch,
        totalEpoch: result.total_epoch,
        latestLoss: result.latest_loss,
        percent: result.percent,
        logTail: result.log_tail,
        errorMessage: result.error_message,
      }
    } catch {
      return {
        isRunning: false,
        pid: null,
        currentEpoch: 0,
        totalEpoch: 0,
        latestLoss: '',
        percent: 0,
        logTail: '',
        errorMessage: '',
      }
    }
  },

  async getConfig(expDir: string): Promise<TrainConfig | null> {
    try {
      const result = await invoke<{ config: TrainConfig | null }>('get_config', { expDir })
      return result.config
    } catch {
      return null
    }
  },

  async saveConfig(config: TrainConfig): Promise<ApiResponse> {
    try {
      const result = await invoke<ApiResponse>('save_config', { config })
      return result
    } catch (error) {
      return {
        ok: false,
        message: String(error),
      }
    }
  },

  async checkPreprocessStatus(expDir: string): Promise<{
    preprocess_done: boolean
    features_done: boolean
    preprocess_running: boolean
    features_running: boolean
    error: string | null
  }> {
    try {
      const result = await invoke<{
        preprocess_done: boolean
        features_done: boolean
        preprocess_running: boolean
        features_running: boolean
        error: string | null
      }>('check_preprocess_status', { expDir })
      return result
    } catch {
      return {
        preprocess_done: false,
        features_done: false,
        preprocess_running: false,
        features_running: false,
        error: null,
      }
    }
  },

  async getExperimentPath(expDir: string): Promise<{ experimentPath: string }> {
    try {
      const result = await invoke<{ experimentPath: string }>('get_experiment_path', { expDir })
      return result
    } catch {
      return { experimentPath: '' }
    }
  },

  async startLogStream(logPath: string, eventId: string): Promise<void> {
    await invoke('start_log_stream', { logPath, eventId })
  },

  async startTrainProgressStream(
    expDir: string,
    totalEpoch: number,
    eventId: string
  ): Promise<void> {
    await invoke('start_train_progress_stream', { expDir, totalEpoch, eventId })
  },

  async fileExists(path: string): Promise<boolean> {
    try {
      const result = await invoke<{ exists: boolean }>('file_exists', { path })
      return result.exists
    } catch {
      return false
    }
  },

  async deleteTrainingResult(expDir: string): Promise<ApiResponse> {
    try {
      const message = await invoke<string>('delete_training_result', { expDir })
      return { ok: true, message }
    } catch (error) {
      return { ok: false, message: String(error) }
    }
  },

  async clearTrainingArtifacts(expDir: string): Promise<ApiResponse> {
    try {
      const message = await invoke<string>('clear_training_artifacts', { expDir })
      return { ok: true, message }
    } catch (error) {
      return { ok: false, message: String(error) }
    }
  },
}
