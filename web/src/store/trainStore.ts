import { create } from 'zustand'

export type WorkflowStage = 'pending' | 'running' | 'done' | 'error'
export type StepStatus = 'wait' | 'process' | 'finish' | 'error'

export interface TrainProgressState {
  currentEpoch: number
  currentPercent: number
  latestLoss: string
  estimatedSeconds?: number
  avgEpochTime?: number
}

export interface StepState {
  status: StepStatus
  log: string
  percent?: number
}

export interface WorkflowStatus {
  preprocess: WorkflowStage
  f0: WorkflowStage
  audio: WorkflowStage
  features: WorkflowStage
  train: WorkflowStage
  index: WorkflowStage
  trainEpoch: number
  totalEpoch: number
}

interface TrainState {
  workflowStatus: Record<string, WorkflowStatus>
  trainProgress: Record<string, TrainProgressState>
  steps: Record<string, StepState[]>
  running: Record<string, boolean>
  currentStep: Record<string, number>
  checkpointPath: Record<string, string>
  currentRecordId: Record<string, string | null>
  
  setWorkflowStatus: (expDir: string, status: Partial<WorkflowStatus>) => void
  setTrainProgress: (expDir: string, progress: TrainProgressState) => void
  setSteps: (expDir: string, steps: StepState[]) => void
  updateStep: (expDir: string, idx: number, patch: Partial<StepState>) => void
  setRunning: (expDir: string, running: boolean) => void
  setCurrentStep: (expDir: string, step: number) => void
  setCheckpointPath: (expDir: string, path: string) => void
  setCurrentRecordId: (expDir: string, id: string | null) => void
  
  getWorkflowStatus: (expDir: string, totalEpoch: number) => WorkflowStatus
  getTrainProgress: (expDir: string) => TrainProgressState
  getSteps: (expDir: string) => StepState[]
  getRunning: (expDir: string) => boolean
  getCurrentStep: (expDir: string) => number
  getCheckpointPath: (expDir: string) => string
  getCurrentRecordId: (expDir: string) => string | null
  
  clearExperiment: (expDir: string) => void
}

const emptyWorkflow = (totalEpoch: number): WorkflowStatus => ({
  preprocess: 'pending',
  f0: 'pending',
  audio: 'pending',
  features: 'pending',
  train: 'pending',
  index: 'pending',
  trainEpoch: 0,
  totalEpoch,
})

const emptyProgress = (): TrainProgressState => ({
  currentEpoch: 0,
  currentPercent: 0,
  latestLoss: '',
})

const emptySteps = (): StepState[] => [
  { status: 'wait', log: '' },
  { status: 'wait', log: '' },
  { status: 'wait', log: '' },
]

export const useTrainStore = create<TrainState>()((set, get) => ({
  workflowStatus: {},
  trainProgress: {},
  steps: {},
  running: {},
  currentStep: {},
  checkpointPath: {},
  currentRecordId: {},

  setWorkflowStatus: (expDir, status) =>
    set((state) => ({
      workflowStatus: {
        ...state.workflowStatus,
        [expDir]: { ...state.workflowStatus[expDir], ...status },
      },
    })),

  setTrainProgress: (expDir, progress) =>
    set((state) => ({
      trainProgress: {
        ...state.trainProgress,
        [expDir]: progress,
      },
    })),

  setSteps: (expDir, steps) =>
    set((state) => ({
      steps: {
        ...state.steps,
        [expDir]: steps,
      },
    })),

  updateStep: (expDir, idx, patch) =>
    set((state) => {
      const currentSteps = state.steps[expDir] || emptySteps()
      return {
        steps: {
          ...state.steps,
          [expDir]: currentSteps.map((s, i) => (i === idx ? { ...s, ...patch } : s)),
        },
      }
    }),

  setRunning: (expDir, running) =>
    set((state) => ({
      running: {
        ...state.running,
        [expDir]: running,
      },
    })),

  setCurrentStep: (expDir, step) =>
    set((state) => ({
      currentStep: {
        ...state.currentStep,
        [expDir]: step,
      },
    })),

  setCheckpointPath: (expDir, path) =>
    set((state) => ({
      checkpointPath: {
        ...state.checkpointPath,
        [expDir]: path,
      },
    })),

  setCurrentRecordId: (expDir, id) =>
    set((state) => ({
      currentRecordId: {
        ...state.currentRecordId,
        [expDir]: id,
      },
    })),

  getWorkflowStatus: (expDir, totalEpoch) => {
    const state = get()
    return state.workflowStatus[expDir] || emptyWorkflow(totalEpoch)
  },

  getTrainProgress: (expDir) => {
    const state = get()
    return state.trainProgress[expDir] || emptyProgress()
  },

  getSteps: (expDir) => {
    const state = get()
    return state.steps[expDir] || emptySteps()
  },

  getRunning: (expDir) => {
    const state = get()
    return state.running[expDir] || false
  },

  getCurrentStep: (expDir) => {
    const state = get()
    return state.currentStep[expDir] || 0
  },

  getCheckpointPath: (expDir) => {
    const state = get()
    return state.checkpointPath[expDir] || ''
  },

  getCurrentRecordId: (expDir) => {
    const state = get()
    return state.currentRecordId[expDir] || null
  },

  clearExperiment: (expDir) =>
    set((state) => {
      const newState = { ...state }
      delete newState.workflowStatus[expDir]
      delete newState.trainProgress[expDir]
      delete newState.steps[expDir]
      delete newState.running[expDir]
      delete newState.currentStep[expDir]
      delete newState.checkpointPath[expDir]
      delete newState.currentRecordId[expDir]
      return newState
    }),
}))
