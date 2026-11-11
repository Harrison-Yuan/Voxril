import { useEffect, useCallback } from 'react'
import { useTrainStore, type WorkflowStatus } from '@/store/trainStore'
import { workflowApi, type WorkflowStatusDTO } from '@/api/database'

function dtoToWorkflow(dto: WorkflowStatusDTO): WorkflowStatus {
  return {
    preprocess: dto.preprocess as WorkflowStatus['preprocess'],
    f0: (dto as any).f0 as WorkflowStatus['f0'] || 'pending',
    audio: (dto as any).audio as WorkflowStatus['audio'] || 'pending',
    features: dto.features as WorkflowStatus['features'],
    train: dto.train as WorkflowStatus['train'],
    index: dto.index_status as WorkflowStatus['index'],
    trainEpoch: dto.train_epoch,
    totalEpoch: dto.total_epoch,
  }
}

function workflowToDto(expDir: string, workflow: WorkflowStatus): WorkflowStatusDTO {
  return {
    exp_dir: expDir,
    preprocess: workflow.preprocess,
    f0: workflow.f0 as any,
    audio: workflow.audio as any,
    features: workflow.features,
    train: workflow.train,
    index_status: workflow.index,
    train_epoch: workflow.trainEpoch,
    total_epoch: workflow.totalEpoch,
  } as any
}

export const useWorkflowSync = (expDir: string) => {
  const trainStore = useTrainStore()

  useEffect(() => {
    if (!expDir) return

    const loadWorkflow = async () => {
      try {
        const existing = trainStore.getWorkflowStatus(expDir, 0)
        if (existing.trainEpoch > 0 || existing.preprocess !== 'pending') {
          return
        }
        const dto = await workflowApi.get(expDir)
        if (dto) {
          const workflow = dtoToWorkflow(dto)
          trainStore.setWorkflowStatus(expDir, workflow)
        }
      } catch (error) {
        console.error('Failed to load workflow status:', error)
      }
    }

    loadWorkflow()
  }, [expDir, trainStore])

  const syncWorkflow = useCallback(
    async (patch: Partial<WorkflowStatus>) => {
      const current = trainStore.getWorkflowStatus(expDir, 0)
      const updated = { ...current, ...patch }
      
      trainStore.setWorkflowStatus(expDir, updated)
      
      try {
        const dto = workflowToDto(expDir, updated)
        await workflowApi.save(dto)
      } catch (error) {
        console.error('Failed to save workflow status:', error)
      }
    },
    [expDir, trainStore]
  )

  return { syncWorkflow }
}
