import { useState, useCallback, useEffect } from 'react'
import { configApi, type ExperimentConfigDTO } from '@/api/database'

export interface ExperimentConfig {
  expDir: string
  sr: string
  ifF0: boolean
  trainsetDir: string
  spkId: number
  gpus: string
  npProcess: number
  f0Method: string
  saveEpoch: number
  totalEpoch: number
  batchSize: number
  ifSaveLatest: boolean
  ifCacheGpu: boolean
  ifSaveEveryWeights: boolean
}

export const DEFAULT_CONFIG: ExperimentConfig = {
  expDir: 'my_voice',
  sr: '40k',
  ifF0: true,
  trainsetDir: '',
  spkId: 0,
  gpus: '0',
  npProcess: 4,
  f0Method: 'rmvpe',
  saveEpoch: 25,
  totalEpoch: 200,
  batchSize: 1,
  ifSaveLatest: true,
  ifCacheGpu: false,
  ifSaveEveryWeights: false,
}

function dtoToConfig(dto: ExperimentConfigDTO): ExperimentConfig {
  return {
    expDir: dto.exp_dir,
    sr: dto.sr,
    ifF0: dto.if_f0,
    trainsetDir: dto.trainset_dir,
    spkId: dto.spk_id,
    gpus: dto.gpus,
    npProcess: dto.np_process,
    f0Method: dto.f0_method,
    saveEpoch: dto.save_epoch,
    totalEpoch: dto.total_epoch,
    batchSize: dto.batch_size,
    ifSaveLatest: dto.if_save_latest,
    ifCacheGpu: dto.if_cache_gpu,
    ifSaveEveryWeights: dto.if_save_every_weights,
  }
}

function configToDto(config: ExperimentConfig): ExperimentConfigDTO {
  return {
    exp_dir: config.expDir,
    sr: config.sr,
    if_f0: config.ifF0,
    trainset_dir: config.trainsetDir,
    spk_id: config.spkId,
    gpus: config.gpus,
    np_process: config.npProcess,
    f0_method: config.f0Method,
    save_epoch: config.saveEpoch,
    total_epoch: config.totalEpoch,
    batch_size: config.batchSize,
    if_save_latest: config.ifSaveLatest,
    if_cache_gpu: config.ifCacheGpu,
    if_save_every_weights: config.ifSaveEveryWeights,
    version: 'v2',
  }
}

export const useExperimentConfig = () => {
  const [config, setConfig] = useState<ExperimentConfig>(DEFAULT_CONFIG)
  const [isLoaded, setIsLoaded] = useState(false)

  useEffect(() => {
    const loadInitialConfig = async () => {
      try {
        const lastExpDir = await configApi.getLastExperiment()
        
        if (lastExpDir) {
          const lastConfig = await configApi.get(lastExpDir)
          if (lastConfig) {
            setConfig(dtoToConfig(lastConfig))
            setIsLoaded(true)
            return
          }
        }
        
        const configs = await configApi.getAll()
        if (configs.length > 0) {
          const sortedConfigs = configs.sort((a, b) => {
            const aTime = a.updated_at || a.created_at || 0
            const bTime = b.updated_at || b.created_at || 0
            return bTime - aTime
          })
          setConfig(dtoToConfig(sortedConfigs[0]))
        }
        
        setIsLoaded(true)
      } catch (err) {
        console.error('Failed to load configs:', err)
        setIsLoaded(true)
      }
    }
    
    loadInitialConfig()
  }, [])

  const saveConfig = useCallback(
    (newConfig: Partial<ExperimentConfig>) => {
      const updated = { ...config, ...newConfig }
      if (updated.trainsetDir) {
        updated.trainsetDir = updated.trainsetDir.trim()
      }
      setConfig(updated)

      const dto = configToDto(updated)
      configApi.save(dto).catch((err) => console.error('Failed to save config:', err))
    },
    [config]
  )

  const loadConfigForExperiment = useCallback(async (expDir: string) => {
    try {
      const dto = await configApi.get(expDir)
      if (dto) {
        const loaded = dtoToConfig(dto)
        setConfig(loaded)
        await configApi.setLastExperiment(expDir)
        return loaded
      }
      const defaultWithExpDir = { ...DEFAULT_CONFIG, expDir }
      setConfig(defaultWithExpDir)
      await configApi.setLastExperiment(expDir)
      return defaultWithExpDir
    } catch (err) {
      console.error('Failed to load config:', err)
      const defaultWithExpDir = { ...DEFAULT_CONFIG, expDir }
      setConfig(defaultWithExpDir)
      return defaultWithExpDir
    }
  }, [])

  const generateExpName = useCallback(
    (baseName: string, configOverride?: Partial<ExperimentConfig>) => {
      const cfg = { ...config, ...configOverride }
      const parts = [baseName]
      if (cfg.sr !== '40k') parts.push(cfg.sr)
      if (cfg.f0Method !== 'rmvpe') parts.push(cfg.f0Method)
      if (cfg.totalEpoch !== 200) parts.push(`${cfg.totalEpoch}e`)
      return parts.join('_')
    },
    [config]
  )

  return {
    config,
    setConfig,
    saveConfig,
    loadConfigForExperiment,
    generateExpName,
    isLoaded,
  }
}
