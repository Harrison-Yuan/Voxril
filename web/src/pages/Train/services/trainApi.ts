import { invoke } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-dialog'

export interface ApiResponse {
  ok: boolean
  message: string
}

/** 批次摘要（`list_experiments`）。限定名由 `${voice}/${run_id}` 组成，前端不拼目录。 */
export interface ExperimentSummary {
  voice: string
  run_id: string
  /** 采样率；旧布局无法解析时为 null */
  sr: string | null
  /** 旧布局版本目录（`v2_40k`），不属于批次，仅用于兼容展示 */
  legacy: boolean
  /** 训练完成时的最终 epoch；未训练或未完成时为 null */
  epoch: number | null
  /** 数据集指纹；清单缺失时为 null */
  fingerprint: string | null
  has_model: boolean
  has_index: boolean
  /** 批次工作位当前占用字节数 */
  bytes: number
}

/** 批次工作位的分阶段体积（`get_run_usage`）。 */
export interface RunUsage {
  preprocess: number
  f0: number
  feature: number
  train: number
  index: number
  other: number
  /** 当前仍在盘上、可被回收的字节数 */
  reclaimable: number
  /** 该批次已自动回收的累计字节数 */
  reclaimed: number
}

export type ManifestStageStatus = 'pending' | 'running' | 'done' | 'stale' | 'reclaimed' | 'failed'

export interface ManifestStage {
  status: ManifestStageStatus
  manifest_hash?: string
  epoch?: number
  ntotal?: number
  n_ivf?: number
  started_at?: number
  finished_at?: number
}

/** `run.json` 清单：步骤状态的唯一真源。 */
export interface RunManifest {
  run_id: string
  created_at: number
  voice: string
  sr: string
  version: string
  if_f0: boolean
  pipeline_version: string
  dataset: {
    source: string
    fingerprint: string
    splits: number
    total_seconds?: number
  }
  params?: {
    epochs: number
    batch_size: number
    save_epoch: number
    f0_method?: string
    np?: number
  }
  stages: {
    preprocess: ManifestStage
    f0: ManifestStage
    feature: ManifestStage
    train: ManifestStage
    index: ManifestStage
  }
  assets: { kind: string; path: string; bytes: number }[]
  reclaimable: { bytes: number; reclaimed_at?: number }
}

export interface TrainIndexResponse extends ApiResponse {}

export interface PreprocessResponse extends ApiResponse {
  /** 实际使用的工作位限定名 `<voice>/<run_id>`，后续步骤必须原样回传 */
  exp_dir: string
}

/** 批次删除/回收类命令的统一返回：失败时 message 为后端原因。 */
export interface ArtifactOpResponse extends ApiResponse {
  freedBytes: number
}

export interface RemoveIndexResponse extends ArtifactOpResponse {
  removed: number
}

export interface ExtractFeaturesResponse extends ApiResponse {}

export interface TrainModelResponse extends ApiResponse {}

export interface TrainConfig {
  exp_dir: string
  trainset_dir: string
  sr: string
  np_process: number
  f0_method: string
  if_f0: boolean
  save_epoch: number
  total_epoch: number
  batch_size: number
  if_save_latest: boolean
  gpus: string
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

  async listExperiments(): Promise<ExperimentSummary[]> {
    try {
      const result = await invoke<ExperimentSummary[]>('list_experiments')
      return result
    } catch {
      return []
    }
  },

  /** 读取并校准批次清单（磁盘产物 + 指纹），失败时返回 null。 */
  async getRunManifest(expDir: string): Promise<RunManifest | null> {
    try {
      return await invoke<RunManifest>('get_run_manifest', { expDir })
    } catch {
      return null
    }
  },

  /** 批次工作位的分阶段体积。 */
  async getRunUsage(expDir: string): Promise<RunUsage | null> {
    try {
      return await invoke<RunUsage>('get_run_usage', { expDir })
    } catch {
      return null
    }
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
    npProcess: number,
    ifF0: boolean
  ): Promise<PreprocessResponse> {
    try {
      const result = await invoke<PreprocessResponse>('preprocess_dataset', {
        trainsetDir,
        expDir,
        sr,
        npProcess,
        ifF0,
      })
      return result
    } catch (error) {
      return {
        ok: false,
        message: String(error),
        exp_dir: '',
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
    saveEpoch: number
    totalEpoch: number
    batchSize: number
    ifSaveLatest: boolean
    gpus: string
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
        save_epoch: params.saveEpoch,
        total_epoch: params.totalEpoch,
        batch_size: params.batchSize,
        if_save_latest: params.ifSaveLatest,
        gpus: params.gpus,
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

  /** 回收批次一次性产物（R1；deep 时包含预处理中间产物与 G_/D_ 权重）。 */
  async reclaimRun(expDir: string, deep: boolean): Promise<ArtifactOpResponse> {
    try {
      const result = await invoke<{ freed_bytes: number }>('reclaim_run', { expDir, deep })
      return { ok: true, message: '', freedBytes: result.freed_bytes }
    } catch (error) {
      return { ok: false, message: String(error), freedBytes: 0 }
    }
  },

  /** 重置批次工作位：清空全部产物，保留归档权重与批次清单。 */
  async resetExperiment(expDir: string): Promise<ArtifactOpResponse> {
    try {
      const result = await invoke<{ freed_bytes: number }>('reset_experiment', { expDir })
      return { ok: true, message: '', freedBytes: result.freed_bytes }
    } catch (error) {
      return { ok: false, message: String(error), freedBytes: 0 }
    }
  },

  /** 删除历史批次（归档权重 + 工作位 + 批次清单），被成品引用时后端拒绝。 */
  async purgeRun(expDir: string): Promise<ArtifactOpResponse> {
    try {
      const result = await invoke<{ freed_bytes: number }>('purge_run', { expDir })
      return { ok: true, message: '', freedBytes: result.freed_bytes }
    } catch (error) {
      return { ok: false, message: String(error), freedBytes: 0 }
    }
  },

  /** 重建索引前清掉工作位里的旧索引及 sidecar。 */
  async removeIndexArtifacts(expDir: string): Promise<RemoveIndexResponse> {
    try {
      const result = await invoke<{ removed: number; freed_bytes: number }>(
        'remove_index_artifacts',
        { expDir }
      )
      return { ok: true, message: '', removed: result.removed, freedBytes: result.freed_bytes }
    } catch (error) {
      return { ok: false, message: String(error), removed: 0, freedBytes: 0 }
    }
  },
}
