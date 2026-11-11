import { useState, useEffect, useRef } from 'react'
import { Space, Typography, Alert, Modal, message, Card, Button, Descriptions } from 'antd'
import { FolderOpenOutlined, DeleteOutlined } from '@ant-design/icons'
import { listen } from '@tauri-apps/api/event'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { PreprocessStep } from './components/PreprocessStep'
import { FeatureExtractionSteps } from './components/FeatureExtractionSteps'
import { TrainModelStep } from './components/TrainModelStep'
import { TrainProgress } from './components/TrainProgress'
import { TrainLogViewer } from './components/TrainLogViewer'
import { NextStepsGuide } from './components/NextStepsGuide'
import { useExperimentConfig } from './hooks/useExperimentConfig'
import { useWorkflowSync } from './hooks/useWorkflowSync'
import { useTrainStore } from '@/store/trainStore'
import { trainApi } from './services/trainApi'
import { workspaceApi } from '@/api/workspace'
import { workflowApi } from '@/api/database'
import type { StreamStatus } from './hooks/useTrainProgress'

const { Title, Text } = Typography

export default function TrainV2Page() {
  const { config, saveConfig, loadConfigForExperiment, generateExpName, isLoaded } = useExperimentConfig()

  const noop = (..._args: unknown[]) => {}
  const addRecord = (..._args: unknown[]) => crypto.randomUUID()
  const updateRecord = noop

  const [workspaceRoot, setWorkspaceRoot] = useState<string>('')
  const qualifiedExpDir = config.expDir ? `${config.expDir}/v2_${config.sr}` : ''
  const expPath = workspaceRoot && config.expDir ? `${workspaceRoot}/experiments/${qualifiedExpDir}` : ''

  const { syncWorkflow } = useWorkflowSync(qualifiedExpDir)

  const trainStore = useTrainStore()
  const workflowStatus = trainStore.getWorkflowStatus(qualifiedExpDir, config.totalEpoch)
  const trainProgress = trainStore.getTrainProgress(qualifiedExpDir)
  const steps = trainStore.getSteps(qualifiedExpDir)
  const running = trainStore.getRunning(qualifiedExpDir)
  const currentStep = trainStore.getCurrentStep(qualifiedExpDir)
  const checkpointPath = trainStore.getCheckpointPath(qualifiedExpDir)

  const [experimentOptions, setExperimentOptions] = useState<string[]>([])
  const [logStreamStatus, setLogStreamStatus] = useState<StreamStatus>('idle')
  const [trainProgressStreamStatus, setTrainProgressStreamStatus] = useState<StreamStatus>('idle')
  const [isInitialized, setIsInitialized] = useState(false)
  const [currentExtracting, setCurrentExtracting] = useState<'none' | 'f0' | 'audio'>('none')
  const logUnlistenRef = useRef<UnlistenFn | null>(null)
  const progressUnlistenRef = useRef<UnlistenFn | null>(null)

  useEffect(() => {
    const initializeApp = async () => {
      try {
        const [experiments, workspace] = await Promise.all([
          trainApi.listExperiments(),
          workspaceApi.getWorkspace()
        ])
        setExperimentOptions(experiments)
        setWorkspaceRoot(workspace.workspace)
        
        setIsInitialized(true)
      } catch (error) {
        console.error('Failed to initialize app:', error)
        setIsInitialized(true)
      }
    }
    
    initializeApp()
  }, [])

  useEffect(() => {
    if (!isLoaded || !isInitialized || !qualifiedExpDir || !workspaceRoot) return
    
    const checkFileSystemStatus = async () => {
      try {
        const [
          preprocessLogExists,
          extractLogExists,
        ] = await Promise.all([
          trainApi.fileExists(`${expPath}/preprocess.log`),
          trainApi.fileExists(`${expPath}/extract_f0_feature.log`),
        ])
        
        let preprocessStatus: typeof workflowStatus.preprocess = 'pending'
        let featuresStatus: typeof workflowStatus.features = 'pending'
        let f0Status: typeof workflowStatus.f0 = 'pending'
        let audioStatus: typeof workflowStatus.audio = 'pending'
        
        if (preprocessLogExists) {
          const preprocessLog = await trainApi.readLocalText(`${expPath}/preprocess.log`)
          if (preprocessLog.includes('end preprocess')) {
            preprocessStatus = 'done'
          } else if (preprocessLog.includes('Fail.') || preprocessLog.includes('错误') || preprocessLog.includes('Traceback')) {
            preprocessStatus = 'error'
          } else if (preprocessLog.includes('start preprocess') || preprocessLog.includes('Found ') || preprocessLog.includes('Progress:')) {
            preprocessStatus = 'running'
          } else {
            preprocessStatus = 'running'
          }
        }
        
        if (extractLogExists && preprocessStatus === 'done') {
          const extractLog = await trainApi.readLocalText(`${expPath}/extract_f0_feature.log`)
          if (extractLog.includes('all-feature-done')) {
            f0Status = 'done'
            audioStatus = 'done'
            featuresStatus = 'done'
          } else if (extractLog.includes('Fail.') || extractLog.includes('错误') || extractLog.includes('Traceback')) {
            f0Status = 'error'
            featuresStatus = 'error'
          } else if (extractLog.includes('todo-f0') || extractLog.includes('all-feature-')) {
            f0Status = 'running'
            featuresStatus = 'running'
          }
          
          // Check audio feature extraction separately
          const audioLogPath = `${expPath}/extract_audio_feature.log`
          const audioLogExists = await trainApi.fileExists(audioLogPath)
          if (audioLogExists && f0Status === 'done') {
            const audioLog = await trainApi.readLocalText(audioLogPath)
            if (audioLog.includes('all-feature-done')) {
              audioStatus = 'done'
            } else if (audioLog.includes('Fail.') || audioLog.includes('错误') || audioLog.includes('Traceback')) {
              audioStatus = 'error'
            } else if (audioLog.includes('all-feature-')) {
              audioStatus = 'running'
            }
          }
        }
        
        trainStore.setWorkflowStatus(qualifiedExpDir, {
          preprocess: preprocessStatus,
          f0: f0Status,
          audio: audioStatus,
          features: featuresStatus,
        })
        
        const isActuallyRunning = preprocessStatus === 'running' || f0Status === 'running' || audioStatus === 'running'
        
        if (isActuallyRunning) {
          const actualStatus = await trainApi.getTrainStatus(qualifiedExpDir)
          if (!actualStatus.isRunning) {
            if (preprocessStatus === 'running') {
              preprocessStatus = 'error'
            }
            if (f0Status === 'running') {
              f0Status = 'error'
            }
            if (audioStatus === 'running') {
              audioStatus = 'error'
            }
            trainStore.setWorkflowStatus(qualifiedExpDir, {
              preprocess: preprocessStatus,
              f0: f0Status,
              audio: audioStatus,
              features: f0Status === 'error' || audioStatus === 'error' ? 'error' : featuresStatus,
            })
            trainStore.setRunning(qualifiedExpDir, false)
          } else {
            trainStore.setRunning(qualifiedExpDir, true)
            trainStore.setCurrentStep(qualifiedExpDir, 0)
          }
        } else {
          trainStore.setRunning(qualifiedExpDir, false)
        }
        
        const featuresRunning = f0Status === 'running' || audioStatus === 'running'
        const modelExists = await trainApi.fileExists(`${expPath}/train.log`) || await trainApi.fileExists(`${expPath}/config.json`)
        if (modelExists || preprocessStatus === 'running' || featuresRunning) {
          await checkTrainStatus(preprocessStatus === 'running', featuresRunning)
        }

        const indexFiles = await trainApi.getArtifactMatches(expPath, 'index')
        const hasModelPth = await trainApi.fileExists(`${expPath}/model.pth`)
        const checkpointMatches = await trainApi.getArtifactMatches(expPath, 'checkpoint')
        const hasCheckpoints = checkpointMatches.length > 0
        if (indexFiles.length > 0 || hasModelPth || hasCheckpoints) {
          const status = await trainApi.getTrainStatus(qualifiedExpDir)
          const actualEpoch = status.currentEpoch > 0 ? status.currentEpoch : workflowStatus.trainEpoch
          if (actualEpoch >= config.totalEpoch) {
            trainStore.setWorkflowStatus(qualifiedExpDir, {
              preprocess: 'done',
              f0: 'done',
              audio: 'done',
              features: 'done',
              train: 'done',
              index: 'done',
              trainEpoch: config.totalEpoch,
              totalEpoch: config.totalEpoch,
            })
          } else {
            trainStore.setWorkflowStatus(qualifiedExpDir, {
              preprocess: 'done',
              f0: 'done',
              audio: 'done',
              features: 'done',
              trainEpoch: actualEpoch,
              totalEpoch: config.totalEpoch,
            })
            if (actualEpoch > 0 && !status.isRunning && !status.errorMessage) {
              persistWorkflow({ train: 'pending', trainEpoch: actualEpoch, totalEpoch: config.totalEpoch })
            }
          }
        }
      } catch (error) {
        console.error('Failed to check file system status:', error)
        trainStore.setRunning(qualifiedExpDir, false)
      }
    }
    
    checkFileSystemStatus()
  }, [qualifiedExpDir, isLoaded, isInitialized, workspaceRoot])

  const checkTrainStatus = async (reconnectPreprocess = false, reconnectFeatures = false) => {
    if (!qualifiedExpDir || !workspaceRoot) return
    
    if (reconnectPreprocess) {
      trainStore.setCurrentStep(qualifiedExpDir, 0)
      trainStore.setRunning(qualifiedExpDir, true)
      await openLogStream(0, `${expPath}/preprocess.log`)
      return
    }

    if (reconnectFeatures) {
      trainStore.setCurrentStep(qualifiedExpDir, 0)
      trainStore.setRunning(qualifiedExpDir, true)
      await openLogStream(0, `${expPath}/extract_f0_feature.log`)
      return
    }
    
    try {
      const status = await trainApi.getTrainStatus(qualifiedExpDir)
      
      if (status.currentEpoch > 0 || status.totalEpoch > 0) {
        trainStore.setTrainProgress(qualifiedExpDir, {
          currentEpoch: status.currentEpoch,
          currentPercent: status.percent,
          latestLoss: status.latestLoss,
        })
        
        if (status.errorMessage) {
          persistWorkflow({ train: 'error', trainEpoch: status.currentEpoch, totalEpoch: status.totalEpoch })
          trainStore.updateStep(qualifiedExpDir, 1, {
            status: 'error',
            log: `训练异常退出:\n${status.logTail || status.errorMessage}`
          })
          trainStore.setRunning(qualifiedExpDir, false)
          message.error(`训练异常: ${status.errorMessage}`)
          return
        }
        
        persistWorkflow({
          train: status.isRunning ? 'running' : (status.currentEpoch >= status.totalEpoch ? 'done' : 'pending'),
          trainEpoch: status.currentEpoch,
          totalEpoch: status.totalEpoch,
        })
      }
      
      if (status.isRunning) {
        trainStore.setCurrentStep(qualifiedExpDir, 1)
        trainStore.setRunning(qualifiedExpDir, true)
        
        const logPath = `${expPath}/train.log`
        const existingLog = status.logTail || await trainApi.readLocalText(logPath)
        if (existingLog) {
          trainStore.updateStep(qualifiedExpDir, 1, { log: existingLog.split('\n').slice(-80).join('\n'), status: 'process' })
        }
        await openLogStream(1, logPath, existingLog)
        await openProgressStream(qualifiedExpDir, status.totalEpoch)
      }
    } catch (error) {
      console.error('Failed to check train status:', error)
    }
  }

  useEffect(() => {
    if (!qualifiedExpDir) return
    
    if (!running) {
      trainStore.setCheckpointPath(qualifiedExpDir, '')
    }
  }, [qualifiedExpDir])

  useEffect(() => {
    if (workflowStatus.train === 'running' && trainProgressStreamStatus === 'idle' && logStreamStatus === 'idle' && workspaceRoot) {
      openLogStream(1, `${expPath}/train.log`)
      openProgressStream(qualifiedExpDir, config.totalEpoch)
    }
  }, [workflowStatus.train, workspaceRoot, trainProgressStreamStatus, logStreamStatus])

  useEffect(() => () => closeAllStreams(), [])

  const persistWorkflow = async (patch: Partial<typeof workflowStatus>) => {
    const next = { ...workflowStatus, ...patch }
    if (next.trainEpoch > 0) {
      next.preprocess = 'done'
      next.f0 = 'done'
      next.audio = 'done'
      next.features = 'done'
    }
    await syncWorkflow(next)
  }

  const updateStep = (idx: number, patch: Partial<typeof steps[0]>) => {
    trainStore.updateStep(qualifiedExpDir, idx, patch)
  }

  const closeAllStreams = () => {
    if (logUnlistenRef.current) {
      logUnlistenRef.current()
      logUnlistenRef.current = null
    }
    if (progressUnlistenRef.current) {
      progressUnlistenRef.current()
      progressUnlistenRef.current = null
    }
    setLogStreamStatus('idle')
    setTrainProgressStreamStatus('idle')
  }

  const openLogStream = async (stepIdx: number, logPath: string, initialLog = '') => {
    if (logUnlistenRef.current) {
      logUnlistenRef.current()
    }
    
    setLogStreamStatus('connecting')
    const eventId = `log-stream-${qualifiedExpDir}-${Date.now()}`
    let logBuffer = initialLog
    
    try {
      const unlisten = await listen(eventId, (event: any) => {
        const payload = event.payload
        
        if (payload.type === 'error') {
          setLogStreamStatus('idle')
          updateStep(stepIdx, { status: 'error', log: `日志读取失败: ${payload.message}` })
          trainStore.setRunning(qualifiedExpDir, false)
          return
        }
        
        if (payload.type === 'log') {
          setLogStreamStatus('connected')
          logBuffer += payload.text + '\n'
          const lines = logBuffer.split('\n')
          const displayLines = lines.slice(-80).join('\n')

          // 解析 [PROGRESS:f0:XX%] 或 [PROGRESS:feature:XX%] 格式的进度
          const progressMatch = payload.text.match(/\[PROGRESS:(f0|feature):(\d+)%\]/)
          if (progressMatch) {
            const percent = parseInt(progressMatch[2], 10)
            updateStep(stepIdx, { log: displayLines, percent })
          } else {
            updateStep(stepIdx, { log: displayLines })
          }

          if (logBuffer.length > 50000) {
            logBuffer = lines.slice(-200).join('\n')
          }
        }
      })
      
      logUnlistenRef.current = unlisten
      
      await trainApi.startLogStream(logPath, eventId)
      
    } catch (error) {
      console.error('Failed to start log stream:', error)
      setLogStreamStatus('idle')
      updateStep(stepIdx, { status: 'error', log: `启动日志流失败: ${String(error)}` })
      trainStore.setRunning(qualifiedExpDir, false)
    }
  }

  const openProgressStream = async (expDir: string, totalEpoch: number) => {
    if (progressUnlistenRef.current) {
      progressUnlistenRef.current()
    }
    
    setTrainProgressStreamStatus('connecting')
    const eventId = `progress-stream-${expDir}-${Date.now()}`
    
    try {
      const unlisten = await listen(eventId, (event: any) => {
        const payload = event.payload
        
        if (payload.type === 'progress') {
          setTrainProgressStreamStatus('connected')
          const epoch = Number(payload.epoch ?? 0)
          const percent = Number(payload.percent ?? 0)
          const loss = String(payload.loss ?? '')
          const status = String(payload.status ?? 'waiting')
          
          trainStore.setTrainProgress(expDir, {
            currentEpoch: epoch,
            currentPercent: percent,
            latestLoss: loss,
            estimatedSeconds: Number(payload.estimated_seconds ?? 0),
            avgEpochTime: Number(payload.avg_epoch_time ?? 0),
          })
          
          if (status === 'running') {
            persistWorkflow({ train: 'running', trainEpoch: epoch, totalEpoch })
            const recordId = trainStore.getCurrentRecordId(expDir)
            if (recordId) updateRecord(recordId, { completedEpoch: epoch })
          }
          
          if (status === 'done') {
            closeAllStreams()
            onTrainDone(epoch, totalEpoch)
          }
        }

        if (payload.type === 'error') {
          message.error(`训练异常: ${payload.message}`)
          updateStep(1, { status: 'error', log: `训练异常退出:\n${payload.message}` })
          trainStore.setRunning(qualifiedExpDir, false)
          persistWorkflow({ train: 'error' })
          closeAllStreams()
        }
      })
      
      progressUnlistenRef.current = unlisten
      
      await trainApi.startTrainProgressStream(expDir, totalEpoch, eventId)
      
    } catch (error) {
      console.error('Failed to start progress stream:', error)
      setTrainProgressStreamStatus('idle')
    }
  }

  const onTrainDone = async (epoch: number, totalEpoch: number) => {
    persistWorkflow({ train: 'done', trainEpoch: Math.max(epoch, totalEpoch), totalEpoch })
    message.success({ content: '✅ 模型训练完成！正在自动构建特征索引...', duration: 5 })
    if ('Notification' in window && Notification.permission === 'granted') {
      new Notification('RVC 训练完成', { body: '模型训练已完成，正在自动构建特征索引...' })
    }
    await new Promise((r) => setTimeout(r, 1000))
    await runTrainIndex(true)
  }

  const handleConfigChange = async (patch: Partial<typeof config>) => {
    if (patch.expDir && patch.expDir !== config.expDir) {
      closeAllStreams()
      trainStore.setRunning(qualifiedExpDir, false)
      const exists = experimentOptions.includes(patch.expDir)
      message.loading({ content: exists ? `正在加载实验 "${patch.expDir}" 的配置...` : `正在创建新实验 "${patch.expDir}"...`, key: 'loadConfig' })
      try {
        await loadConfigForExperiment(patch.expDir)
        message.success({ content: exists ? `已切换至实验 "${patch.expDir}"` : `已创建新实验 "${patch.expDir}"`, key: 'loadConfig', duration: 2 })
      } catch {
        saveConfig(patch)
        message.success({ content: `已创建新实验 "${patch.expDir}"`, key: 'loadConfig', duration: 2 })
      }
      return
    }
    
    const criticalKeys = ['sr']
    const hasCritical = Object.keys(patch).some((k) => criticalKeys.includes(k))
    if (hasCritical && workflowStatus.trainEpoch > 0 && workspaceRoot) {
      const newCfg = { ...config, ...patch }
      try {
        const text = await trainApi.readLocalText(`${expPath}/config.json`)
        if (text) {
          const old = JSON.parse(text)
          const oldSr = old.data?.sampling_rate === 32000 ? '32k' : old.data?.sampling_rate === 48000 ? '48k' : '40k'
          if (oldSr !== newCfg.sr) {
            const suggested = generateExpName(config.expDir.split('_')[0], newCfg)
            message.warning({ content: `检测到关键配置变更（${oldSr} → ${newCfg.sr}），建议创建新实验：${suggested}`, duration: 8 })
            Modal.confirm({
              title: '配置已变更，无法继续当前实验',
              content: `是否创建新实验 "${suggested}"？点击取消将恢复原配置。`,
              okText: '创建新实验', cancelText: '恢复原配置',
              onOk: () => saveConfig({ ...patch, expDir: suggested }),
            })
            return
          }
        }
      } catch { /* ignore */ }
    }
    saveConfig(patch)
  }

  const runPreprocess = async () => {
    if ('Notification' in window && Notification.permission === 'default') {
      Notification.requestPermission().catch(() => undefined)
    }
    if (!config.trainsetDir) { message.warning('请先配置训练集路径'); return }
    if (!workspaceRoot) { message.warning('工作区未初始化'); return }
    
    trainStore.setRunning(qualifiedExpDir, true)
    trainStore.setCurrentStep(qualifiedExpDir, 0)
    persistWorkflow({ preprocess: 'running' })
    updateStep(0, { status: 'process', log: '正在启动数据预处理...' })
    
    try {
      const preResult = await trainApi.preprocessDataset(config.trainsetDir, qualifiedExpDir, config.sr, config.npProcess)
      
      if (!preResult.ok) {
        trainStore.setRunning(qualifiedExpDir, false)
        updateStep(0, { status: 'error', log: `=== 数据处理启动失败 ===\n${preResult.message}` })
        persistWorkflow({ preprocess: 'error' })
        message.error({ content: '数据预处理启动失败，请查看日志', duration: 8 })
        return
      }
      
      // Poll log until completion
      const logPath = `${expPath}/preprocess.log`
      for (let i = 0; i < 300; i++) {
        await new Promise(r => setTimeout(r, 2000))
        try {
          const logText = await trainApi.readLocalText(logPath)
          if (logText.includes('end preprocess')) {
            updateStep(0, { status: 'finish', log: logText })
            persistWorkflow({ preprocess: 'done' })
            message.success('数据预处理完成，请继续执行特征提取')
            trainStore.setRunning(qualifiedExpDir, false)
            return
          }
          if (logText.includes('Fail.') || logText.includes('Traceback') || logText.includes('错误')) {
            updateStep(0, { status: 'error', log: logText })
            persistWorkflow({ preprocess: 'error' })
            message.error({ content: '数据预处理失败，请查看日志', duration: 8 })
            trainStore.setRunning(qualifiedExpDir, false)
            return
          }
          // Update log display so user sees progress
          if (i % 3 === 0) {
            updateStep(0, { status: 'process', log: logText })
          }
        } catch { /* log not ready yet */ }
      }
      updateStep(0, { status: 'error', log: '预处理超时（10 分钟）' })
      persistWorkflow({ preprocess: 'error' })
      message.error({ content: '数据预处理超时（10 分钟），请检查日志', duration: 8 })
      trainStore.setRunning(qualifiedExpDir, false)
    } catch (err) {
      trainStore.setRunning(qualifiedExpDir, false)
      updateStep(0, { status: 'error', log: String(err) })
      persistWorkflow({ preprocess: 'error' })
      message.error('数据预处理启动失败')
    }
  }

  const runExtractF0 = async () => {
    if (workflowStatus.preprocess !== 'done') {
      message.warning('请先完成数据处理')
      return
    }
    if (!config.ifF0) {
      trainStore.setWorkflowStatus(qualifiedExpDir, { f0: 'done' })
      message.info('当前模型不使用 F0，跳过 F0 特征提取')
      return
    }
    setCurrentExtracting('f0')
    try {
      const result = await trainApi.extractF0Features(config.npProcess, config.f0Method, qualifiedExpDir)
      if (!result.ok || result.message.includes('Fail') || result.message.includes('错误')) {
        trainStore.setWorkflowStatus(qualifiedExpDir, { f0: 'error' })
        message.error({ content: `F0 特征提取启动失败：${result.message.slice(0, 100)}`, duration: 8 })
        setCurrentExtracting('none')
        return
      }
      // Poll log until completion
      const logPath = `${expPath}/extract_f0_feature.log`
      for (let i = 0; i < 600; i++) {
        await new Promise(r => setTimeout(r, 2000))
        try {
          const logText = await trainApi.readLocalText(logPath)
          // 实时更新日志和进度
          const displayLines = logText.split('\n').slice(-80).join('\n')
          const progressMatch = logText.match(/\[PROGRESS:f0:(\d+)%\]/g)
          const lastProgress = progressMatch ? progressMatch[progressMatch.length - 1].match(/(\d+)%/) : null
          const percent = lastProgress ? parseInt(lastProgress[1], 10) : undefined
          updateStep(0, { log: displayLines, ...(percent !== undefined ? { percent } : {}) })

          if (logText.includes('all-feature-done')) {
            trainStore.setWorkflowStatus(qualifiedExpDir, { f0: 'done' })
            message.success('F0 特征提取完成，请继续提取音频特征')
            return
          }
          if (logText.includes('Fail.') || logText.includes('f0fail') || logText.includes('Traceback')) {
            trainStore.setWorkflowStatus(qualifiedExpDir, { f0: 'error' })
            message.error({ content: `F0 特征提取失败，请查看日志`, duration: 8 })
            return
          }
        } catch { /* log not ready yet */ }
      }
      trainStore.setWorkflowStatus(qualifiedExpDir, { f0: 'error' })
      message.error({ content: 'F0 特征提取超时（10 分钟），请检查日志', duration: 8 })
    } catch (err) {
      trainStore.setWorkflowStatus(qualifiedExpDir, { f0: 'error' })
      message.error({ content: `F0 特征提取失败：${String(err).slice(0, 100)}`, duration: 8 })
    } finally {
      setCurrentExtracting('none')
    }
  }

  const runExtractAudio = async () => {
    if (workflowStatus.preprocess !== 'done') {
      message.warning('请先完成数据处理')
      return
    }
    const f0Required = config.ifF0 && workflowStatus.f0 !== 'done'
    if (f0Required) {
      message.warning('请先完成 F0 特征提取')
      return
    }
    setCurrentExtracting('audio')
    try {
      const result = await trainApi.extractAudioFeatures(config.gpus, qualifiedExpDir)
      if (!result.ok || result.message.includes('Fail') || result.message.includes('错误')) {
        trainStore.setWorkflowStatus(qualifiedExpDir, { audio: 'error', features: 'error' })
        message.error({ content: `音频特征提取启动失败：${result.message.slice(0, 100)}`, duration: 8 })
        setCurrentExtracting('none')
        return
      }
      // Poll log until completion
      const logPath = `${expPath}/extract_audio_feature.log`
      for (let i = 0; i < 600; i++) {
        await new Promise(r => setTimeout(r, 2000))
        try {
          const logText = await trainApi.readLocalText(logPath)
          // 实时更新日志和进度
          const displayLines = logText.split('\n').slice(-80).join('\n')
          const progressMatch = logText.match(/\[PROGRESS:feature:(\d+)%\]/g)
          const lastProgress = progressMatch ? progressMatch[progressMatch.length - 1].match(/(\d+)%/) : null
          const percent = lastProgress ? parseInt(lastProgress[1], 10) : undefined
          updateStep(0, { log: displayLines, ...(percent !== undefined ? { percent } : {}) })

          if (logText.includes('all-feature-done')) {
            trainStore.setWorkflowStatus(qualifiedExpDir, { audio: 'done', features: 'done' })
            message.success('音频特征提取完成，可以开始训练模型')
            return
          }
          if (logText.includes('Fail.') || logText.includes('Traceback') || logText.includes('nan')) {
            trainStore.setWorkflowStatus(qualifiedExpDir, { audio: 'error', features: 'error' })
            message.error({ content: '音频特征提取失败，请查看日志', duration: 8 })
            return
          }
        } catch { /* log not ready yet */ }
      }
      trainStore.setWorkflowStatus(qualifiedExpDir, { audio: 'error', features: 'error' })
      message.error({ content: '音频特征提取超时（10 分钟），请检查日志', duration: 8 })
    } catch (err) {
      trainStore.setWorkflowStatus(qualifiedExpDir, { audio: 'error', features: 'error' })
      message.error({ content: `音频特征提取失败：${String(err).slice(0, 100)}`, duration: 8 })
    } finally {
      setCurrentExtracting('none')
    }
  }

  const runTrainIndex = async (skipGuard = false) => {
    if (!skipGuard && workflowStatus.index === 'done') {
      message.info('特征索引已构建完成')
      return
    }
    trainStore.setRunning(qualifiedExpDir, true)
    persistWorkflow({ index: 'running' })
    try {
      const result = await trainApi.trainIndex(qualifiedExpDir)
      persistWorkflow({ index: result.ok ? 'done' : 'error' })
      if (result.ok) {
        await trainApi.deleteFile(`${expPath}/total_fea.npy`).catch(() => {})
        message.success({ content: '训练流程全部完成！模型和索引已就绪，可以去转换试听了', duration: 8 })
        if ('Notification' in window && Notification.permission === 'granted') {
          new Notification('RVC 训练全部完成', { body: `实验 "${qualifiedExpDir}" 已完成，模型和索引已就绪！` })
        }
        const recordId = trainStore.getCurrentRecordId(qualifiedExpDir)
        if (recordId) {
          updateRecord(recordId, {
            status: 'completed',
            endTime: Date.now(),
            indexPath: `logs/${qualifiedExpDir}/added_*.index`,
          })
        }
      } else {
        message.error({ content: `特征索引构建失败：${result.message.slice(0, 100)}`, duration: 10 })
      }
    } catch (err) {
      persistWorkflow({ index: 'error' })
      message.error({ content: `特征索引构建失败：${String(err).slice(0, 100)}`, duration: 10 })
    } finally {
      trainStore.setRunning(qualifiedExpDir, false)
    }
  }

  const runTrain = async () => {
    const featuresDone = config.ifF0 ? (workflowStatus.f0 === 'done' && workflowStatus.audio === 'done') : workflowStatus.audio === 'done'
    if (!featuresDone) { message.warning('请先完成特征提取'); return }

    const hasCheckpoints = await trainApi.getArtifactMatches(expPath, 'checkpoint')
    const isComplete = workflowStatus.trainEpoch >= config.totalEpoch && hasCheckpoints.length > 0
    const isContinue = hasCheckpoints.length > 0 && workflowStatus.trainEpoch > 0 && workflowStatus.trainEpoch < config.totalEpoch

    if (isComplete) {
      // Full retrain: clean everything, start from scratch
      await trainApi.clearTrainingArtifacts(qualifiedExpDir)
      trainStore.clearExperiment(qualifiedExpDir)
      message.success('已清理旧训练结果，开始全新训练')
    } else if (!isContinue) {
      // Fresh training (no prior checkpoints): clean stale index files only
      try {
        const staleIndexFiles = await trainApi.getArtifactMatches(expPath, 'index')
        for (const f of staleIndexFiles) {
          await trainApi.deleteFile(`${expPath}/${f}`)
        }
      } catch { /* ignore */ }
    }

    trainStore.setRunning(qualifiedExpDir, true)
    trainStore.setCurrentStep(qualifiedExpDir, 1)
    const startingEpoch = isContinue ? workflowStatus.trainEpoch : 0
    persistWorkflow({ train: 'running', trainEpoch: startingEpoch, totalEpoch: config.totalEpoch })
    trainStore.setTrainProgress(qualifiedExpDir, { currentEpoch: startingEpoch, currentPercent: 0, latestLoss: '' })
    const recordId = addRecord({
      expDir: qualifiedExpDir, sr: config.sr,
      f0Method: config.f0Method, totalEpoch: config.totalEpoch, completedEpoch: startingEpoch,
      status: 'running',
      config: {
        trainsetDir: config.trainsetDir, npProcess: config.npProcess,
        saveEpoch: config.saveEpoch, batchSize: config.batchSize,
        ifF0: config.ifF0, ifSaveLatest: config.ifSaveLatest,
        ifSaveEveryWeights: config.ifSaveEveryWeights,
      },
    })
    trainStore.setCurrentRecordId(qualifiedExpDir, recordId)
    const logPath = `${expPath}/train.log`
    openLogStream(1, logPath)
    openProgressStream(qualifiedExpDir, config.totalEpoch)
    updateStep(1, { status: 'process', log: '训练已启动，正在读取主日志...' })
    try {
      const result = await trainApi.trainModel({ ...config, expDir: qualifiedExpDir })
      const modelMissing = result.message.includes('底模缺失')
      const failed = !result.ok || result.message.includes('Fail') || result.message.includes('错误')
      if (failed) {
        closeAllStreams()
        updateStep(1, { status: 'error', log: result.message })
        persistWorkflow({ train: 'error' })
        if (modelMissing) {
          message.error({ content: <span>底模缺失，请前往<a href="/models" onClick={(e) => { e.preventDefault(); window.location.hash = '/models' }}>「模型管理」</a>页面下载预训练底模后再训练</span>, duration: 10 })
        } else {
          message.error('训练启动失败，请查看日志')
        }
        const recordId = trainStore.getCurrentRecordId(qualifiedExpDir)
        if (recordId) updateRecord(recordId, { status: 'failed', endTime: Date.now() })
        trainStore.setRunning(qualifiedExpDir, false)
      } else {
        updateStep(1, { status: 'process', log: `训练已启动\n${result.message}` })
        persistWorkflow({ train: 'running' })
        message.success('训练已启动，完成后将自动训练特征索引')
      }
    } catch (err) {
      closeAllStreams()
      updateStep(1, { status: 'error', log: String(err) })
      persistWorkflow({ train: 'error' })
      const errStr = String(err)
      if (errStr.includes('底模缺失')) {
        message.error({ content: <span>底模缺失，请前往<a href="/models" onClick={(e) => { e.preventDefault(); window.location.hash = '/models' }}>「模型管理」</a>页面下载预训练底模后再训练</span>, duration: 10 })
      } else {
        message.error('训练请求失败')
      }
      const recordId = trainStore.getCurrentRecordId(qualifiedExpDir)
      if (recordId) updateRecord(recordId, { status: 'failed', endTime: Date.now() })
      trainStore.setRunning(qualifiedExpDir, false)
    }
  }

  const stopTrain = async () => {
    try {
      const result = await trainApi.stopTraining(qualifiedExpDir)
      if (result.ok) {
        message.success('训练已停止')
        closeAllStreams()
        persistWorkflow({ train: 'error' })
        updateStep(1, { status: 'error', log: '训练已被用户手动停止' })
        trainStore.setRunning(qualifiedExpDir, false)
        const recordId = trainStore.getCurrentRecordId(qualifiedExpDir)
        if (recordId) {
          updateRecord(recordId, {
            status: 'stopped', endTime: Date.now(), completedEpoch: trainProgress.currentEpoch,
          })
        }
      } else {
        message.error('停止训练失败')
      }
    } catch (err) {
      message.error(`停止训练失败：${String(err)}`)
    }
  }

  const showProgress = currentStep === 1 || trainProgress.currentEpoch > 0 || workflowStatus.trainEpoch > 0

  return (
    <Space orientation="vertical" size="large" style={{ width: '100%' }}>
      <div>
        <Title level={3} style={{ margin: 0, fontWeight: 600 }}>训练模型</Title>
        <Text type="secondary" style={{ marginTop: 4, display: 'block', fontSize: 14 }}>
          将自己的音色训练成 RVC 声音模型，训练完成后可反复使用
        </Text>
      </div>

      <Alert
        type="info"
        showIcon
        message="训练流程"
        description="三步完成：数据预处理 → 特征提取（先F0后音频） → 开始训练。训练完成记得构建特征索引。"
      />

      <PreprocessStep
        config={config}
        status={workflowStatus.preprocess}
        running={running && currentStep === 0}
        canRun={!running}
        experimentOptions={experimentOptions}
        hasStartedWorkflow={running && currentStep === 0}
        onConfigChange={handleConfigChange}
        onExpDirChange={async (expDir: string) => {
          if (experimentOptions.includes(expDir)) {
            message.loading({ content: `正在加载实验 "${expDir}" 的配置...`, key: 'loadConfig' })
            try {
              const loadedConfig = await loadConfigForExperiment(expDir)
              const newQualified = `${loadedConfig.expDir}/v2_${loadedConfig.sr}`
              const dto = await workflowApi.get(newQualified)
              if (dto) {
                trainStore.setWorkflowStatus(newQualified, {
                  preprocess: dto.preprocess as typeof workflowStatus.preprocess,
                  features: dto.features as typeof workflowStatus.features,
                  train: dto.train as typeof workflowStatus.train,
                  index: dto.index_status as typeof workflowStatus.index,
                  trainEpoch: dto.train_epoch,
                  totalEpoch: dto.total_epoch,
                })
                if (dto.train_epoch > 0) {
                  trainStore.setRunning(newQualified, false)
                }
              }
              message.success({ content: `已加载实验 "${expDir}" 的配置`, key: 'loadConfig', duration: 2 })
            } catch (error) {
              console.error('Failed to load experiment config:', error)
              message.error({ content: `加载配置失败：${String(error)}`, key: 'loadConfig', duration: 3 })
            }
          } else {
            handleConfigChange({ expDir })
          }
        }}
        onSelectDirectory={async () => {
          try {
            console.log('开始选择文件夹...')
            const path = await trainApi.selectDirectory('选择训练集文件夹')
            console.log('选择的路径:', path)
            if (path) {
              saveConfig({ trainsetDir: path.trim() })
              message.success(`已选择训练集路径：${path}`)
            } else {
              message.info('未选择文件夹')
            }
          } catch (error) {
            console.error('选择文件夹失败:', error)
            message.error(`选择文件夹失败：${String(error)}`)
          }
        }}
        onRun={runPreprocess}
      />

      <FeatureExtractionSteps
        preprocessStatus={workflowStatus.preprocess}
        f0Status={workflowStatus.f0}
        audioStatus={workflowStatus.audio}
        ifF0={config.ifF0}
        running={running}
        currentExtracting={currentExtracting}
        f0Percent={steps[0]?.percent}
        audioPercent={steps[0]?.percent}
        onExtractF0={runExtractF0}
        onExtractAudio={runExtractAudio}
      />

      <TrainModelStep
        config={config}
        trainStatus={workflowStatus.train}
        featuresStatus={workflowStatus.audio}
        indexStatus={workflowStatus.index}
        trainEpoch={workflowStatus.trainEpoch}
        totalEpoch={config.totalEpoch}
        running={running && currentStep === 1}
        canRun={!running && workflowStatus.audio === 'done'}
        onConfigChange={handleConfigChange}
        onRun={runTrain}
        onStop={stopTrain}
        onBuildIndex={() => runTrainIndex(true)}
        onClearTraining={() => {
          Modal.confirm({
            title: '确认清理训练结果',
            content: (
              <Space orientation="vertical" size={8}>
                <Text>确定要清理实验 <Text strong>"{qualifiedExpDir}"</Text> 的所有训练结果吗？</Text>
                <ul style={{ margin: '8px 0', paddingLeft: 20 }}>
                  <li>模型文件 (*.pth)</li>
                  <li>特征索引 (added_*.index)</li>
                  <li>训练日志和评估数据</li>
                </ul>
                <Text type="success">预处理数据（音频切片、F0、HuBERT 特征）将被保留，可直接重新训练。</Text>
                <Text type="danger">此操作不可撤销。</Text>
              </Space>
            ),
            okText: '确认清理',
            okType: 'danger',
            cancelText: '取消',
            onOk: async () => {
              try {
                const result = await trainApi.clearTrainingArtifacts(qualifiedExpDir)
                if (result.ok) {
                  message.success({ content: result.message, duration: 8 })
                  trainStore.clearExperiment(qualifiedExpDir)
                } else {
                  message.error({ content: result.message, duration: 8 })
                }
              } catch (error) {
                message.error(`清理失败：${String(error)}`)
              }
            },
          })
        }}
      />

      {showProgress && (
        <TrainProgress
          trainProgress={trainProgress}
          trainStatus={workflowStatus.train}
          indexStatus={workflowStatus.index}
          trainEpoch={workflowStatus.trainEpoch}
          totalEpoch={config.totalEpoch}
          checkpointPath={checkpointPath}
          trainProgressStreamStatus={trainProgressStreamStatus}
        />
      )}

      {steps[currentStep].log && (
        <TrainLogViewer
          log={steps[currentStep].log}
          status={steps[currentStep].status}
          logStreamStatus={logStreamStatus}
          running={running && currentStep === 1}
        />
      )}

      <NextStepsGuide />

      {experimentOptions.includes(qualifiedExpDir) && (
        <Card 
          title={
            <Space>
              <FolderOpenOutlined />
              <span>实验目录结构</span>
            </Space>
          }
          size="small"
          extra={
            <Button
              size="small"
              danger
              icon={<DeleteOutlined />}
              onClick={() => {
                Modal.confirm({
                  title: '确认删除训练结果',
                  content: (
                    <Space orientation="vertical" size={8}>
                      <Text>确定要删除实验 <Text strong>"{qualifiedExpDir}"</Text> 的所有训练结果吗？</Text>
                      <Text type="secondary">将删除以下内容：</Text>
                      <ul style={{ margin: '8px 0', paddingLeft: 20 }}>
                        <li>模型文件 (*.pth)</li>
                        <li>特征索引 (added_*.index)</li>
                        <li>训练日志和评估数据</li>
                      </ul>
                      <Text type="success">预处理数据（音频切片、F0、HuBERT 特征）将被保留，可直接重新训练。</Text>
                      <Text type="danger">此操作不可撤销。</Text>
                    </Space>
                  ),
                  okText: '确认删除',
                  okType: 'danger',
                  cancelText: '取消',
                  onOk: async () => {
                    try {
                      const result = await trainApi.clearTrainingArtifacts(qualifiedExpDir)
                      if (result.ok) {
                        message.success({ content: result.message, duration: 8 })
                        trainStore.clearExperiment(qualifiedExpDir)
                      } else {
                        message.error({ content: result.message, duration: 8 })
                      }
                    } catch (error) {
                      message.error(`删除失败：${String(error)}`)
                    }
                  },
                })
              }}
            >
              删除训练结果
            </Button>
          }
        >
          <Descriptions column={1} size="small" bordered>
            <Descriptions.Item label="预处理数据">
              <Space orientation="vertical" size={2} style={{ width: '100%' }}>
                <Text code>{expPath ? `${expPath}/` : '加载中...'}</Text>
                <Text type="secondary" style={{ fontSize: 12 }}>
                  包含音频切片(0_gt_wavs)、重采样音频(1_16k_wavs)、F0特征(2a_f0)、HuBERT特征(3_feature256/768)
                </Text>
              </Space>
            </Descriptions.Item>
            <Descriptions.Item label="模型产物">
              <Space orientation="vertical" size={2} style={{ width: '100%' }}>
                <Text code>{expPath ? `${expPath}/` : '加载中...'}</Text>
                <Text type="secondary" style={{ fontSize: 12 }}>
                  包含模型检查点(G_*.pth)、索引文件(added_*.index)、训练日志和配置
                </Text>
              </Space>
            </Descriptions.Item>
            <Descriptions.Item label="模型文件">
              <Space orientation="vertical" size={2} style={{ width: '100%' }}>
                <Text code>{expPath ? `${expPath}/G_*.pth` : '加载中...'}</Text>
                <Text type="secondary" style={{ fontSize: 12 }}>
                  训练的模型检查点，用于声音转换
                </Text>
              </Space>
            </Descriptions.Item>
            {workflowStatus.index === 'done' && (
              <Descriptions.Item label="索引文件">
                <Space orientation="vertical" size={2} style={{ width: '100%' }}>
                  <Text code>{expPath ? `${expPath}/added_*.index` : '加载中...'}</Text>
                  <Text type="secondary" style={{ fontSize: 12 }}>
                    特征索引文件，提升转换质量
                  </Text>
                </Space>
              </Descriptions.Item>
            )}
            <Descriptions.Item label="训练日志">
              <Space orientation="vertical" size={2} style={{ width: '100%' }}>
                <Text code>{expPath ? `${expPath}/train.log` : '加载中...'}</Text>
                <Text type="secondary" style={{ fontSize: 12 }}>
                  完整的训练过程日志
                </Text>
              </Space>
            </Descriptions.Item>
            <Descriptions.Item label="快速操作">
              <Space>
                <Button
                  size="small"
                  icon={<FolderOpenOutlined />}
                  onClick={async () => {
                    try {
                      await workspaceApi.openInFinder(expPath)
                      message.success(`已在 Finder 中打开 ${expPath}`)
                    } catch (error) {
                      message.error(`打开失败：${String(error)}`)
                    }
                  }}
                >
                  在 Finder 中打开
                </Button>
                
              </Space>
            </Descriptions.Item>
          </Descriptions>
        </Card>
      )}

    </Space>
  )
}
