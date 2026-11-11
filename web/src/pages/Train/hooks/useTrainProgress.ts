import { useState, useRef, useCallback } from 'react'
import { listen } from '@tauri-apps/api/event'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { invoke } from '@tauri-apps/api/core'

export interface TrainProgressState {
  currentEpoch: number
  currentPercent: number
  latestLoss: string
  completedEpoch?: number
  estimatedSeconds?: number
  avgEpochTime?: number
}

export type StreamStatus = 'idle' | 'connecting' | 'connected' | 'reconnecting'

export const useTrainProgress = () => {
  const [trainProgress, setTrainProgress] = useState<TrainProgressState>({
    currentEpoch: 0,
    currentPercent: 0,
    latestLoss: '',
  })

  const [logStreamStatus, setLogStreamStatus] = useState<StreamStatus>('idle')
  const [trainProgressStreamStatus, setTrainProgressStreamStatus] = useState<StreamStatus>('idle')

  const logUnlistenRef = useRef<UnlistenFn | null>(null)
  const trainProgressUnlistenRef = useRef<UnlistenFn | null>(null)
  const logEventIdRef = useRef<string>('')
  const trainProgressEventIdRef = useRef<string>('')

  const startLogStream = useCallback(async (logPath: string, onLog: (text: string) => void) => {
    if (logUnlistenRef.current) {
      logUnlistenRef.current()
      logUnlistenRef.current = null
    }

    setLogStreamStatus('connecting')
    const eventId = `log-stream-${Date.now()}-${Math.random()}`
    logEventIdRef.current = eventId

    try {
      const unlisten = await listen<{ type: string; text?: string; message?: string }>(
        eventId,
        (event) => {
          if (event.payload.type === 'log' && event.payload.text) {
            setLogStreamStatus('connected')
            onLog(event.payload.text)
          } else if (event.payload.type === 'error') {
            setLogStreamStatus('reconnecting')
            console.error('Log stream error:', event.payload.message)
          }
        }
      )

      logUnlistenRef.current = unlisten

      await invoke('start_log_stream', { logPath, eventId })
    } catch (error) {
      setLogStreamStatus('reconnecting')
      console.error('Failed to start log stream:', error)
    }
  }, [])

  const startTrainProgressStream = useCallback(
    async (
      expDir: string,
      totalEpoch: number,
      onProgress: (epoch: number, percent: number, loss: string, status: string) => void,
      onComplete: () => void
    ) => {
      if (trainProgressUnlistenRef.current) {
        trainProgressUnlistenRef.current()
        trainProgressUnlistenRef.current = null
      }

      setTrainProgressStreamStatus('connecting')
      const eventId = `train-progress-${Date.now()}-${Math.random()}`
      trainProgressEventIdRef.current = eventId

      let hasTriggeredComplete = false

      try {
        const unlisten = await listen<{
          type: string
          epoch?: number
          percent?: number
          loss?: string
          status?: string
          message?: string
        }>(eventId, (event) => {
          if (event.payload.type === 'progress') {
            setTrainProgressStreamStatus('connected')

            const epoch = Number(event.payload.epoch ?? 0)
            const percent = Number(event.payload.percent ?? 0)
            const loss = String(event.payload.loss ?? '')
            const status = String(event.payload.status ?? 'waiting')

            setTrainProgress({ currentEpoch: epoch, currentPercent: percent, latestLoss: loss })
            onProgress(epoch, percent, loss, status)

            if (status === 'done' && !hasTriggeredComplete) {
              hasTriggeredComplete = true
              onComplete()
            }
          } else if (event.payload.type === 'error') {
            setTrainProgressStreamStatus('reconnecting')
            console.error('Train progress stream error:', event.payload.message)
          }
        })

        trainProgressUnlistenRef.current = unlisten

        await invoke('start_train_progress_stream', { expDir, totalEpoch, eventId })
      } catch (error) {
        setTrainProgressStreamStatus('reconnecting')
        console.error('Failed to start train progress stream:', error)
      }
    },
    []
  )

  const stopAllStreams = useCallback(() => {
    if (logUnlistenRef.current) {
      logUnlistenRef.current()
      logUnlistenRef.current = null
    }
    if (trainProgressUnlistenRef.current) {
      trainProgressUnlistenRef.current()
      trainProgressUnlistenRef.current = null
    }
    setLogStreamStatus('idle')
    setTrainProgressStreamStatus('idle')
  }, [])

  return {
    trainProgress,
    setTrainProgress,
    logStreamStatus,
    trainProgressStreamStatus,
    startLogStream,
    startTrainProgressStream,
    stopAllStreams,
  }
}
