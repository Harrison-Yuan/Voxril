import { useState, useCallback, useEffect } from 'react'
import { workflowApi, type WorkflowStatusDTO } from '@/api/database'

export type WorkflowStage = 'pending' | 'running' | 'done' | 'error'

export interface WorkflowStatus {
  preprocess: WorkflowStage
  f0: WorkflowStage
  audio: WorkflowStage
  features: WorkflowStage
  train: WorkflowStage
  index: WorkflowStage
  trainEpoch: number
  totalEpoch: number
  nextAction: string
}

const computeNextAction = (status: Omit<WorkflowStatus, 'nextAction'>): string => {
  if (status.preprocess !== 'done') return '先执行：处理数据。'
  if (status.f0 !== 'done') return '下一步：提取 F0 特征。'
  if (status.audio !== 'done') return '下一步：提取音频特征。'
  if (status.trainEpoch >= status.totalEpoch && status.index !== 'done') {
    return '模型已达到目标轮次，下一步直接训练特征索引。'
  }
  if (status.trainEpoch >= Math.max(50, Math.floor(status.totalEpoch * 0.5)) && status.index !== 'done') {
    return `当前已训练到 ${status.trainEpoch}/${status.totalEpoch} 轮，可先训练特征索引并试听。`
  }
  if (status.train === 'running') {
    return `当前已训练到 ${status.trainEpoch}/${status.totalEpoch} 轮，可继续训练或先训练索引试听。`
  }
  if (status.train !== 'done') return '下一步：训练模型。'
  if (status.index !== 'done') return '下一步：训练特征索引。'
  return '训练阶段已闭环，下一步去"分离歌曲"或直接去"转换试听"。'
}

function dtoToStatus(dto: WorkflowStatusDTO): Omit<WorkflowStatus, 'nextAction'> {
  return {
    preprocess: dto.preprocess as WorkflowStage,
    f0: (dto as any).f0 as WorkflowStage || 'pending',
    audio: (dto as any).audio as WorkflowStage || 'pending',
    features: dto.features as WorkflowStage,
    train: dto.train as WorkflowStage,
    index: dto.index_status as WorkflowStage,
    trainEpoch: dto.train_epoch,
    totalEpoch: dto.total_epoch,
  }
}

function statusToDto(expDir: string, status: Omit<WorkflowStatus, 'nextAction'>): WorkflowStatusDTO {
  return {
    exp_dir: expDir,
    preprocess: status.preprocess,
    f0: status.f0 as any,
    audio: status.audio as any,
    features: status.features,
    train: status.train,
    index_status: status.index,
    train_epoch: status.trainEpoch,
    total_epoch: status.totalEpoch,
  } as any
}

export const useTrainWorkflow = (expDir: string) => {
  const [workflowStatus, setWorkflowStatus] = useState<WorkflowStatus>(() => {
    return {
      preprocess: 'pending',
      f0: 'pending',
      audio: 'pending',
      features: 'pending',
      train: 'pending',
      index: 'pending',
      trainEpoch: 0,
      totalEpoch: 100,
      nextAction: '先执行：处理数据。',
    }
  })

  useEffect(() => {
    if (!expDir) return

    workflowApi
      .get(expDir)
      .then((dto) => {
        if (dto) {
          const nextCore = dtoToStatus(dto)
          setWorkflowStatus({ ...nextCore, nextAction: computeNextAction(nextCore) })
        }
      })
      .catch((err) => console.error('Failed to load workflow status:', err))
  }, [expDir])

  const updateWorkflowStatus = useCallback(
    (patch: Partial<Omit<WorkflowStatus, 'nextAction'>>) => {
      setWorkflowStatus((prev) => {
        const nextCore = {
          preprocess: patch.preprocess ?? prev.preprocess,
          f0: patch.f0 ?? prev.f0,
          audio: patch.audio ?? prev.audio,
          features: patch.features ?? prev.features,
          train: patch.train ?? prev.train,
          index: patch.index ?? prev.index,
          trainEpoch: patch.trainEpoch ?? prev.trainEpoch,
          totalEpoch: patch.totalEpoch ?? prev.totalEpoch,
        }

        if (nextCore.trainEpoch > 0) {
          nextCore.preprocess = 'done'
          nextCore.f0 = 'done'
          nextCore.audio = 'done'
          nextCore.features = 'done'
          if (nextCore.train === 'pending') nextCore.train = 'running'
        }

        const next = { ...nextCore, nextAction: computeNextAction(nextCore) }

        workflowApi.save(statusToDto(expDir, nextCore)).catch((err) => console.error('Failed to save workflow status:', err))

        return next
      })
    },
    [expDir]
  )

  const resetWorkflowStatus = useCallback(
    (totalEpoch: number) => {
      const nextCore: Omit<WorkflowStatus, 'nextAction'> = {
        preprocess: 'pending',
        f0: 'pending',
        audio: 'pending',
        features: 'pending',
        train: 'pending',
        index: 'pending',
        trainEpoch: 0,
        totalEpoch,
      }
      const next = { ...nextCore, nextAction: computeNextAction(nextCore) }

      workflowApi.delete(expDir).catch((err) => console.error('Failed to delete workflow status:', err))

      setWorkflowStatus(next)
    },
    [expDir]
  )

  return {
    workflowStatus,
    updateWorkflowStatus,
    resetWorkflowStatus,
  }
}
