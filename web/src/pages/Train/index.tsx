import { useState, useEffect, useRef, useMemo } from 'react'
import { useSearchParams } from 'react-router-dom'
import { Space, Typography, Alert, Modal, message, Card, Button, Descriptions } from 'antd'
import { FolderOpenOutlined, DeleteOutlined, PlusOutlined, ReloadOutlined } from '@ant-design/icons'
import { listen } from '@tauri-apps/api/event'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { PreprocessStep } from './components/PreprocessStep'
import { FeatureExtractionSteps } from './components/FeatureExtractionSteps'
import { TrainModelStep } from './components/TrainModelStep'
import { TrainProgress } from './components/TrainProgress'
import { TrainLogViewer } from './components/TrainLogViewer'
import { NextStepsGuide } from './components/NextStepsGuide'
import { BatchTable, formatRunDate } from './components/BatchTable'
import { useExperimentConfig } from './hooks/useExperimentConfig'
import { useWorkflowSync } from './hooks/useWorkflowSync'
import { useTrainStore, type WorkflowStage } from '@/store/trainStore'
import { trainApi } from './services/trainApi'
import type { ExperimentSummary, ManifestStageStatus, RunManifest, RunUsage } from './services/trainApi'
import { formatBytes } from '@/lib/formatBytes'
import { workspaceApi } from '@/api/workspace'
import type { StreamStatus } from './hooks/useTrainProgress'

const { Title, Text } = Typography

export default function TrainV2Page() {
  const { config, saveConfig, loadConfigForExperiment, generateExpName, isLoaded } = useExperimentConfig()

  const noop = (..._args: unknown[]) => {}
  const addRecord = (..._args: unknown[]) => crypto.randomUUID()
  const updateRecord = noop

  /**
   * 实验标识：配置中的声音名。前端状态（trainStore / workflow 持久化）一律以它为准，
   * 这样预处理建批前后状态键不变，进度不会丢。
   */
  const experimentKey = config.expDir.trim()
  /** 已建批的工作位限定名 `<voice>/<run_id>`；尚未建批时为空。 */
  const [runExpDir, setRunExpDir] = useState('')
  /** 传给后端的工作位：未建批时只有声音名（由预处理建批），建批后为 `<voice>/<run_id>`。 */
  const workExpDir = runExpDir || experimentKey
  /** 工作位绝对路径及其对应的限定名，由后端按工作区布局解析（前端不拼目录名）。 */
  const [resolvedPath, setResolvedPath] = useState<{ dir: string; path: string }>({ dir: '', path: '' })
  const expPath = resolvedPath.dir === workExpDir ? resolvedPath.path : ''

  const { syncWorkflow } = useWorkflowSync(experimentKey)

  const trainStore = useTrainStore()
  const workflowStatus = trainStore.getWorkflowStatus(experimentKey, config.totalEpoch)
  const trainProgress = trainStore.getTrainProgress(experimentKey)
  const steps = trainStore.getSteps(experimentKey)
  const running = trainStore.getRunning(experimentKey)
  const currentStep = trainStore.getCurrentStep(experimentKey)
  const checkpointPath = trainStore.getCheckpointPath(experimentKey)

  const [experiments, setExperiments] = useState<ExperimentSummary[]>([])
  /** 该声音下的批次，最新在前；旧布局目录（`v2_40k`）不属于批次。 */
  const voiceBatches = useMemo(
    () => experiments.filter((item) => item.voice === experimentKey && !item.legacy).reverse(),
    [experiments, experimentKey]
  )
  /** 实验下拉的可选声音名（去重排序）。切到哪个批次由批次列表决定。 */
  const experimentOptions = useMemo(
    () => Array.from(new Set(experiments.map((item) => item.voice))).sort(),
    [experiments]
  )
  /** 当前批次的分阶段体积；未绑定批次时为 null。 */
  const [usage, setUsage] = useState<RunUsage | null>(null)
  /** 自增触发批次状态重读（步骤完成后调用）。 */
  const [refreshToken, setRefreshToken] = useState(0)
  const [logStreamStatus, setLogStreamStatus] = useState<StreamStatus>('idle')
  const [trainProgressStreamStatus, setTrainProgressStreamStatus] = useState<StreamStatus>('idle')
  const [isInitialized, setIsInitialized] = useState(false)
  const [currentExtracting, setCurrentExtracting] = useState<'none' | 'f0' | 'audio'>('none')
  const logUnlistenRef = useRef<UnlistenFn | null>(null)
  const progressUnlistenRef = useRef<UnlistenFn | null>(null)
  /** 已自动绑定过最近批次的声音名，避免刷新列表时把用户手动选的旧批次顶掉。 */
  const autoBoundVoiceRef = useRef('')

  const refreshBatchStatus = () => setRefreshToken((token) => token + 1)

  /**
   * 从翻唱首页声音卡片的「详情」进入时带 `?voice=<声音名>`：
   * 等初始配置加载完成后载入该声音的配置，并把参数从地址栏移除，避免回退时重复套用。
   */
  const [searchParams, setSearchParams] = useSearchParams()
  const voiceParam = searchParams.get('voice')
  const appliedVoiceRef = useRef('')
  useEffect(() => {
    if (!voiceParam || !isLoaded || appliedVoiceRef.current === voiceParam) return
    appliedVoiceRef.current = voiceParam
    void loadConfigForExperiment(voiceParam)
    const next = new URLSearchParams(searchParams)
    next.delete('voice')
    setSearchParams(next, { replace: true })
  }, [voiceParam, isLoaded])

  const loadExperiments = async () => {
    setExperiments(await trainApi.listExperiments())
  }

  /** 清单阶段状态 → 前端流程状态；`failed` 在界面上显示为"失败"。 */
  const stageStatusToWorkflow = (status: ManifestStageStatus): WorkflowStage =>
    status === 'failed' ? 'error' : status

  /** F0 与音频特征合并为"特征提取"总状态。 */
  const combineFeatures = (f0: WorkflowStage, audio: WorkflowStage): WorkflowStage => {
    if (f0 === 'error' || audio === 'error') return 'error'
    if (f0 === 'running' || audio === 'running') return 'running'
    if (f0 === 'stale' || audio === 'stale') return 'stale'
    if (f0 === 'reclaimed' || audio === 'reclaimed') return 'reclaimed'
    if (f0 === 'done' && audio === 'done') return 'done'
    return 'pending'
  }

  /** 以 run.json 为准刷新流程状态。 */
  const applyManifest = (manifest: RunManifest) => {
    const preprocess = stageStatusToWorkflow(manifest.stages.preprocess.status)
    const f0 = stageStatusToWorkflow(manifest.stages.f0.status)
    const audio = stageStatusToWorkflow(manifest.stages.feature.status)
    const train = stageStatusToWorkflow(manifest.stages.train.status)
    const index = stageStatusToWorkflow(manifest.stages.index.status)
    trainStore.setWorkflowStatus(experimentKey, {
      preprocess,
      f0,
      audio,
      features: combineFeatures(f0, audio),
      train,
      index,
      trainEpoch: train === 'done' ? (manifest.stages.train.epoch ?? 0) : 0,
      totalEpoch: manifest.params?.epochs ?? config.totalEpoch,
    })
    trainStore.setRunning(
      experimentKey,
      [preprocess, f0, audio, train, index].includes('running')
    )
  }

  useEffect(() => {
    const initializeApp = async () => {
      try {
        setExperiments(await trainApi.listExperiments())
      } catch (error) {
        console.error('Failed to initialize app:', error)
      } finally {
        setIsInitialized(true)
      }
    }
    
    initializeApp()
  }, [])

  // 工作位目录由后端按布局解析，避免前端硬编码目录结构
  useEffect(() => {
    if (!workExpDir) return
    let cancelled = false
    trainApi.getExperimentPath(workExpDir).then(({ experimentPath }) => {
      if (!cancelled) setResolvedPath({ dir: workExpDir, path: experimentPath })
    })
    return () => { cancelled = true }
  }, [workExpDir])

  // 切换声音即视为换实验：解除上一个批次的绑定
  useEffect(() => {
    setRunExpDir('')
  }, [config.expDir])

  // 页面上还没有批次时，自动接上该声音的最新批次，保证状态、体积与日志连续
  useEffect(() => {
    if (!isInitialized || !experimentKey || runExpDir) return
    if (autoBoundVoiceRef.current === experimentKey) return
    const latest = voiceBatches[0]
    if (!latest) return
    autoBoundVoiceRef.current = experimentKey
    setRunExpDir(`${latest.voice}/${latest.run_id}`)
  }, [isInitialized, experimentKey, runExpDir, voiceBatches])

  // 步骤状态以批次清单（run.json）为唯一真源：磁盘产物 + 指纹校准后的结果
  useEffect(() => {
    if (!isLoaded || !isInitialized || !experimentKey) return
    let cancelled = false

    const loadManifest = async () => {
      const manifest = await trainApi.getRunManifest(workExpDir)
      if (cancelled) return
      if (!manifest) {
        // 尚未建批或仍是旧布局：保留工作流中已有的进度，不做覆盖
        setUsage(null)
        return
      }
      applyManifest(manifest)

      const runUsage = runExpDir ? await trainApi.getRunUsage(workExpDir) : null
      if (cancelled) return
      setUsage(runUsage)

      // 页面重进或应用重启后，仍在运行的阶段需要重新接上日志流与进度流
      if (manifest.stages.preprocess.status === 'running') {
        await checkTrainStatus(true, false)
      } else if (manifest.stages.f0.status === 'running' || manifest.stages.feature.status === 'running') {
        await checkTrainStatus(false, true)
      } else if (manifest.stages.train.status === 'running') {
        await checkTrainStatus()
      }
    }

    loadManifest().catch((error) => {
      console.error('Failed to load run manifest:', error)
      if (!cancelled) trainStore.setRunning(experimentKey, false)
    })

    return () => { cancelled = true }
  }, [experimentKey, workExpDir, runExpDir, expPath, isLoaded, isInitialized, refreshToken])

  const checkTrainStatus = async (reconnectPreprocess = false, reconnectFeatures = false) => {
    if (!experimentKey || !expPath) return
    
    if (reconnectPreprocess) {
      trainStore.setCurrentStep(experimentKey, 0)
      trainStore.setRunning(experimentKey, true)
      await openLogStream(0, `${expPath}/preprocess.log`)
      return
    }

    if (reconnectFeatures) {
      trainStore.setCurrentStep(experimentKey, 0)
      trainStore.setRunning(experimentKey, true)
      await openLogStream(0, `${expPath}/extract_f0_feature.log`)
      return
    }
    
    try {
      const status = await trainApi.getTrainStatus(workExpDir)
      
      if (status.currentEpoch > 0 || status.totalEpoch > 0) {
        trainStore.setTrainProgress(experimentKey, {
          currentEpoch: status.currentEpoch,
          currentPercent: status.percent,
          latestLoss: status.latestLoss,
        })
        
        if (status.errorMessage) {
          persistWorkflow({ train: 'error', trainEpoch: status.currentEpoch, totalEpoch: status.totalEpoch })
          trainStore.updateStep(experimentKey, 1, {
            status: 'error',
            log: `训练异常退出:\n${status.logTail || status.errorMessage}`
          })
          trainStore.setRunning(experimentKey, false)
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
        trainStore.setCurrentStep(experimentKey, 1)
        trainStore.setRunning(experimentKey, true)
        
        const logPath = `${expPath}/train.log`
        const existingLog = status.logTail || await trainApi.readLocalText(logPath)
        if (existingLog) {
          trainStore.updateStep(experimentKey, 1, { log: existingLog.split('\n').slice(-80).join('\n'), status: 'process' })
        }
        await openLogStream(1, logPath, existingLog)
        await openProgressStream(experimentKey, workExpDir, status.totalEpoch)
      }
    } catch (error) {
      console.error('Failed to check train status:', error)
    }
  }

  useEffect(() => {
    if (!experimentKey) return
    
    if (!running) {
      trainStore.setCheckpointPath(experimentKey, '')
    }
  }, [experimentKey])

  useEffect(() => {
    if (workflowStatus.train === 'running' && trainProgressStreamStatus === 'idle' && logStreamStatus === 'idle' && expPath) {
      openLogStream(1, `${expPath}/train.log`)
      openProgressStream(experimentKey, workExpDir, config.totalEpoch)
    }
  }, [workflowStatus.train, expPath, workExpDir, trainProgressStreamStatus, logStreamStatus])

  useEffect(() => () => closeAllStreams(), [])

  const persistWorkflow = async (patch: Partial<typeof workflowStatus>) => {
    const next = { ...workflowStatus, ...patch }
    if (next.trainEpoch > 0) {
      // 训练有进度说明这些步骤确实完成过；但已被回收或指纹失效的状态不能被覆盖
      const settle = (stage: WorkflowStage): WorkflowStage =>
        stage === 'pending' ? 'done' : stage
      next.preprocess = settle(next.preprocess)
      next.f0 = settle(next.f0)
      next.audio = settle(next.audio)
      next.features = settle(next.features)
    }
    await syncWorkflow(next)
  }

  const updateStep = (idx: number, patch: Partial<typeof steps[0]>) => {
    trainStore.updateStep(experimentKey, idx, patch)
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
    const eventId = `log-stream-${experimentKey}-${Date.now()}`
    let logBuffer = initialLog
    
    try {
      const unlisten = await listen(eventId, (event: any) => {
        const payload = event.payload
        
        if (payload.type === 'error') {
          setLogStreamStatus('idle')
          updateStep(stepIdx, { status: 'error', log: `日志读取失败: ${payload.message}` })
          trainStore.setRunning(experimentKey, false)
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
      trainStore.setRunning(experimentKey, false)
    }
  }

  /**
   * 订阅训练进度事件。storeKey 为前端状态键（声音名），workDir 为后端工作位（含批次号）。
   */
  const openProgressStream = async (storeKey: string, workDir: string, totalEpoch: number) => {
    if (progressUnlistenRef.current) {
      progressUnlistenRef.current()
    }
    
    setTrainProgressStreamStatus('connecting')
    const eventId = `progress-stream-${storeKey}-${Date.now()}`
    
    try {
      const unlisten = await listen(eventId, (event: any) => {
        const payload = event.payload
        
        if (payload.type === 'progress') {
          setTrainProgressStreamStatus('connected')
          const epoch = Number(payload.epoch ?? 0)
          const percent = Number(payload.percent ?? 0)
          const loss = String(payload.loss ?? '')
          const status = String(payload.status ?? 'waiting')
          
          trainStore.setTrainProgress(storeKey, {
            currentEpoch: epoch,
            currentPercent: percent,
            latestLoss: loss,
            estimatedSeconds: Number(payload.estimated_seconds ?? 0),
            avgEpochTime: Number(payload.avg_epoch_time ?? 0),
          })
          
          if (status === 'running') {
            persistWorkflow({ train: 'running', trainEpoch: epoch, totalEpoch })
            const recordId = trainStore.getCurrentRecordId(storeKey)
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
          trainStore.setRunning(storeKey, false)
          persistWorkflow({ train: 'error' })
          closeAllStreams()
        }
      })
      
      progressUnlistenRef.current = unlisten
      
      await trainApi.startTrainProgressStream(workDir, totalEpoch, eventId)
      
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
      trainStore.setRunning(experimentKey, false)
      autoBoundVoiceRef.current = ''
      const exists = experiments.some((item) => item.voice === patch.expDir)
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
    if (hasCritical && workflowStatus.trainEpoch > 0 && expPath) {
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
    if (!experimentKey) { message.warning('请先填写声音名称'); return }
    
    trainStore.setRunning(experimentKey, true)
    trainStore.setCurrentStep(experimentKey, 0)
    persistWorkflow({ preprocess: 'running' })
    updateStep(0, { status: 'process', log: '正在启动数据预处理...' })
    
    try {
      // 未建批时传声音名，由后端创建批次；已建批时传批次工作位，复用同一批次
      const preResult = await trainApi.preprocessDataset(config.trainsetDir, workExpDir, config.sr, config.npProcess, config.ifF0)
      
      if (!preResult.ok) {
        trainStore.setRunning(experimentKey, false)
        updateStep(0, { status: 'error', log: `=== 数据处理启动失败 ===\n${preResult.message}` })
        persistWorkflow({ preprocess: 'error' })
        message.error({ content: '数据预处理启动失败，请查看日志', duration: 8 })
        return
      }

      // 批次号在此刻才确定：绑定工作位，并解析它的绝对路径
      const runDir = preResult.exp_dir || workExpDir
      autoBoundVoiceRef.current = experimentKey
      if (runDir !== workExpDir) setRunExpDir(runDir)
      let runPath = runDir === workExpDir ? expPath : ''
      if (!runPath) {
        const { experimentPath } = await trainApi.getExperimentPath(runDir)
        if (experimentPath) {
          setResolvedPath({ dir: runDir, path: experimentPath })
          runPath = experimentPath
        }
      }
      if (!runPath) {
        trainStore.setRunning(experimentKey, false)
        updateStep(0, { status: 'error', log: '无法解析批次目录，请重试' })
        persistWorkflow({ preprocess: 'error' })
        return
      }
      
      // Poll log until completion
      const logPath = `${runPath}/preprocess.log`
      for (let i = 0; i < 300; i++) {
        await new Promise(r => setTimeout(r, 2000))
        try {
          const logText = await trainApi.readLocalText(logPath)
          if (logText.includes('end preprocess')) {
            updateStep(0, { status: 'finish', log: logText })
            persistWorkflow({ preprocess: 'done' })
            message.success('数据预处理完成，请继续执行特征提取')
            trainStore.setRunning(experimentKey, false)
            refreshBatchStatus()
            void loadExperiments()
            return
          }
          if (logText.includes('Fail.') || logText.includes('Traceback') || logText.includes('错误')) {
            updateStep(0, { status: 'error', log: logText })
            persistWorkflow({ preprocess: 'error' })
            message.error({ content: '数据预处理失败，请查看日志', duration: 8 })
            trainStore.setRunning(experimentKey, false)
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
      trainStore.setRunning(experimentKey, false)
    } catch (err) {
      trainStore.setRunning(experimentKey, false)
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
      trainStore.setWorkflowStatus(experimentKey, { f0: 'done' })
      message.info('当前模型不使用 F0，跳过 F0 特征提取')
      return
    }
    if (!runExpDir || !expPath) {
      message.warning('训练批次尚未就绪，请先完成数据预处理')
      return
    }
    setCurrentExtracting('f0')
    try {
      const result = await trainApi.extractF0Features(config.npProcess, config.f0Method, workExpDir)
      if (!result.ok || result.message.includes('Fail') || result.message.includes('错误')) {
        trainStore.setWorkflowStatus(experimentKey, { f0: 'error' })
        message.error({ content: `F0 特征提取启动失败：${result.message.slice(0, 100)}`, duration: 8 })
        setCurrentExtracting('none')
        return
      }
      trainStore.setWorkflowStatus(experimentKey, { f0: 'running' })
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
            trainStore.setWorkflowStatus(experimentKey, { f0: 'done' })
            refreshBatchStatus()
            message.success('F0 特征提取完成，请继续提取音频特征')
            return
          }
          if (logText.includes('Fail.') || logText.includes('f0fail') || logText.includes('Traceback')) {
            trainStore.setWorkflowStatus(experimentKey, { f0: 'error' })
            message.error({ content: `F0 特征提取失败，请查看日志`, duration: 8 })
            return
          }
        } catch { /* log not ready yet */ }
      }
      trainStore.setWorkflowStatus(experimentKey, { f0: 'error' })
      message.error({ content: 'F0 特征提取超时（10 分钟），请检查日志', duration: 8 })
    } catch (err) {
      trainStore.setWorkflowStatus(experimentKey, { f0: 'error' })
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
    if (!runExpDir || !expPath) {
      message.warning('训练批次尚未就绪，请先完成数据预处理')
      return
    }
    setCurrentExtracting('audio')
    try {
      const result = await trainApi.extractAudioFeatures(config.gpus, workExpDir)
      if (!result.ok || result.message.includes('Fail') || result.message.includes('错误')) {
        trainStore.setWorkflowStatus(experimentKey, { audio: 'error', features: 'error' })
        message.error({ content: `音频特征提取启动失败：${result.message.slice(0, 100)}`, duration: 8 })
        setCurrentExtracting('none')
        return
      }
      trainStore.setWorkflowStatus(experimentKey, { audio: 'running', features: 'running' })
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
            trainStore.setWorkflowStatus(experimentKey, { audio: 'done', features: 'done' })
            refreshBatchStatus()
            message.success('音频特征提取完成，可以开始训练模型')
            return
          }
          if (logText.includes('Fail.') || logText.includes('Traceback') || logText.includes('nan')) {
            trainStore.setWorkflowStatus(experimentKey, { audio: 'error', features: 'error' })
            message.error({ content: '音频特征提取失败，请查看日志', duration: 8 })
            return
          }
        } catch { /* log not ready yet */ }
      }
      trainStore.setWorkflowStatus(experimentKey, { audio: 'error', features: 'error' })
      message.error({ content: '音频特征提取超时（10 分钟），请检查日志', duration: 8 })
    } catch (err) {
      trainStore.setWorkflowStatus(experimentKey, { audio: 'error', features: 'error' })
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
    if (!runExpDir || !expPath) {
      message.warning('训练批次尚未就绪，请先完成模型训练')
      return
    }
    trainStore.setRunning(experimentKey, true)
    persistWorkflow({ index: 'running' })
    try {
      const result = await trainApi.trainIndex(workExpDir)
      persistWorkflow({ index: result.ok ? 'done' : 'error' })
      if (result.ok) {
        message.success({ content: '训练流程全部完成！模型和索引已就绪，可以去转换试听了', duration: 8 })
        if ('Notification' in window && Notification.permission === 'granted') {
          new Notification('RVC 训练全部完成', { body: `批次 "${workExpDir}" 已完成，模型和索引已就绪！` })
        }
        const recordId = trainStore.getCurrentRecordId(experimentKey)
        if (recordId) {
          updateRecord(recordId, {
            status: 'completed',
            endTime: Date.now(),
            indexPath: `${workExpDir}/added_*.index`,
          })
        }
        refreshBatchStatus()
        void loadExperiments()
      } else {
        message.error({ content: `特征索引构建失败：${result.message.slice(0, 100)}`, duration: 10 })
      }
    } catch (err) {
      persistWorkflow({ index: 'error' })
      message.error({ content: `特征索引构建失败：${String(err).slice(0, 100)}`, duration: 10 })
    } finally {
      trainStore.setRunning(experimentKey, false)
    }
  }

  const runTrain = async () => {
    const featuresDone = config.ifF0 ? (workflowStatus.f0 === 'done' && workflowStatus.audio === 'done') : workflowStatus.audio === 'done'
    if (!featuresDone) { message.warning('请先完成特征提取'); return }
    if (!runExpDir || !expPath) {
      message.warning('训练批次尚未就绪，请先完成数据预处理与特征提取')
      return
    }

    const hasCheckpoints = await trainApi.getArtifactMatches(expPath, 'checkpoint')
    const isComplete = workflowStatus.trainEpoch >= config.totalEpoch && hasCheckpoints.length > 0
    const isContinue = hasCheckpoints.length > 0 && workflowStatus.trainEpoch > 0 && workflowStatus.trainEpoch < config.totalEpoch

    if (isComplete) {
      // 重新训练只清掉旧索引；预处理产物是本次训练的输入，绝不能删
      const cleaned = await trainApi.removeIndexArtifacts(workExpDir)
      if (!cleaned.ok) {
        message.error({ content: `清理旧索引失败：${cleaned.message}`, duration: 8 })
        return
      }
      trainStore.clearExperiment(experimentKey)
      message.success('已清理旧索引，开始全新训练')
    } else if (!isContinue) {
      // 全新训练：清掉可能残留的旧索引，避免与新索引共存
      const cleaned = await trainApi.removeIndexArtifacts(workExpDir)
      if (!cleaned.ok) {
        message.error({ content: `清理旧索引失败：${cleaned.message}`, duration: 8 })
        return
      }
    }

    trainStore.setRunning(experimentKey, true)
    trainStore.setCurrentStep(experimentKey, 1)
    const startingEpoch = isContinue ? workflowStatus.trainEpoch : 0
    persistWorkflow({ train: 'running', trainEpoch: startingEpoch, totalEpoch: config.totalEpoch })
    trainStore.setTrainProgress(experimentKey, { currentEpoch: startingEpoch, currentPercent: 0, latestLoss: '' })
    const recordId = addRecord({
      expDir: workExpDir, sr: config.sr,
      f0Method: config.f0Method, totalEpoch: config.totalEpoch, completedEpoch: startingEpoch,
      status: 'running',
      config: {
        trainsetDir: config.trainsetDir, npProcess: config.npProcess,
        saveEpoch: config.saveEpoch, batchSize: config.batchSize,
        ifF0: config.ifF0, ifSaveLatest: config.ifSaveLatest,
        ifSaveEveryWeights: config.ifSaveEveryWeights,
      },
    })
    trainStore.setCurrentRecordId(experimentKey, recordId)
    const logPath = `${expPath}/train.log`
    openLogStream(1, logPath)
    openProgressStream(experimentKey, workExpDir, config.totalEpoch)
    updateStep(1, { status: 'process', log: '训练已启动，正在读取主日志...' })
    try {
      const result = await trainApi.trainModel({ ...config, expDir: workExpDir })
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
        const recordId = trainStore.getCurrentRecordId(experimentKey)
        if (recordId) updateRecord(recordId, { status: 'failed', endTime: Date.now() })
        trainStore.setRunning(experimentKey, false)
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
      const recordId = trainStore.getCurrentRecordId(experimentKey)
      if (recordId) updateRecord(recordId, { status: 'failed', endTime: Date.now() })
      trainStore.setRunning(experimentKey, false)
    }
  }

  const stopTrain = async () => {
    if (!runExpDir) {
      message.warning('训练批次尚未就绪')
      return
    }
    try {
      const result = await trainApi.stopTraining(workExpDir)
      if (result.ok) {
        message.success('训练已停止')
        closeAllStreams()
        persistWorkflow({ train: 'error' })
        updateStep(1, { status: 'error', log: '训练已被用户手动停止' })
        trainStore.setRunning(experimentKey, false)
        const recordId = trainStore.getCurrentRecordId(experimentKey)
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

  /** 把历史批次接为当前工作位。 */
  const loadBatch = (batch: ExperimentSummary) => {
    closeAllStreams()
    trainStore.setRunning(experimentKey, false)
    autoBoundVoiceRef.current = batch.voice
    setRunExpDir(`${batch.voice}/${batch.run_id}`)
  }

  /** 新建批次模式：解除当前绑定，下一次数据预处理会创建新批次。 */
  const startNewBatch = () => {
    closeAllStreams()
    trainStore.setRunning(experimentKey, false)
    autoBoundVoiceRef.current = experimentKey
    setRunExpDir('')
    setUsage(null)
    message.info('已切换到新建批次模式，执行「数据预处理」将创建新批次')
  }

  /** 删除批次：工作位 + 归档权重 + 批次清单；被成品引用时后端拒绝。 */
  const deleteBatch = (batch: ExperimentSummary) => {
    const qualified = `${batch.voice}/${batch.run_id}`
    Modal.confirm({
      title: '确认删除批次',
      content: (
        <Space orientation="vertical" size={8}>
          <Text>确定要删除批次 <Text strong>"{formatRunDate(batch.run_id)}"</Text>（{qualified}）吗？</Text>
          <Text type="secondary">将删除工作位、归档权重与批次清单，共 {formatBytes(batch.bytes)}。</Text>
          <Text type="secondary">产物会先移入回收站，7 天后自动清除。已被成品引用的批次无法删除。</Text>
        </Space>
      ),
      okText: '确认删除',
      okType: 'danger',
      cancelText: '取消',
      onOk: async () => {
        const result = await trainApi.purgeRun(qualified)
        if (!result.ok) {
          message.error({ content: result.message, duration: 8 })
          return
        }
        message.success({ content: `已删除批次，${formatBytes(result.freedBytes)} 移入回收站`, duration: 6 })
        if (runExpDir === qualified) {
          autoBoundVoiceRef.current = ''
          setRunExpDir('')
          setUsage(null)
          trainStore.clearExperiment(experimentKey)
        }
        await loadExperiments()
        refreshBatchStatus()
      },
    })
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
        description="四步完成：数据预处理 → 特征提取（先 F0 后音频） → 训练模型 → 构建特征索引。索引完成后模型才能用于转换。"
      />

      <PreprocessStep
        config={config}
        status={workflowStatus.preprocess}
        running={running && currentStep === 0}
        canRun={!running}
        experimentOptions={experimentOptions}
        usage={usage}
        hasStartedWorkflow={running && currentStep === 0}
        onConfigChange={handleConfigChange}
        onExpDirChange={async (expDir: string) => {
          if (experiments.some((item) => item.voice === expDir)) {
            message.loading({ content: `正在加载实验 "${expDir}" 的配置...`, key: 'loadConfig' })
            try {
              await loadConfigForExperiment(expDir)
              // 换声音即解除批次绑定，由批次列表自动接上该声音的最新批次
              autoBoundVoiceRef.current = ''
              setRunExpDir('')
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
        usage={usage}
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
        usage={usage}
        onConfigChange={handleConfigChange}
        onRun={runTrain}
        onStop={stopTrain}
        onBuildIndex={() => runTrainIndex(true)}
        onClearTraining={() => {
          if (!runExpDir) {
            message.warning('训练批次尚未就绪，暂无可清理的训练结果')
            return
          }
          Modal.confirm({
            title: '确认重置批次',
            content: (
              <Space orientation="vertical" size={8}>
                <Text>确定要重置批次 <Text strong>"{workExpDir}"</Text> 吗？</Text>
                <ul style={{ margin: '8px 0', paddingLeft: 20 }}>
                  <li>模型检查点与最终模型 (*.pth)</li>
                  <li>特征索引 (added_*.index)</li>
                  <li>预处理切片、F0、HuBERT 特征与训练日志</li>
                </ul>
                <Text type="success">已归档的历史权重与批次清单会保留。</Text>
                <Text type="secondary">产物会先移入回收站，7 天后自动清除。</Text>
              </Space>
            ),
            okText: '确认重置',
            okType: 'danger',
            cancelText: '取消',
            onOk: async () => {
              try {
                const result = await trainApi.resetExperiment(workExpDir)
                if (result.ok) {
                  message.success({
                    content: `已重置批次，${(result.freedBytes / 1024 / 1024).toFixed(1)} MB 移入回收站，7 天后自动清除`,
                    duration: 8,
                  })
                  trainStore.clearExperiment(experimentKey)
                  setUsage(null)
                  refreshBatchStatus()
                  void loadExperiments()
                } else {
                  message.error({ content: result.message, duration: 8 })
                }
              } catch (error) {
                message.error(`重置失败：${String(error)}`)
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

      <Card
        title={
          <Space>
            <FolderOpenOutlined />
            <span>训练批次</span>
          </Space>
        }
        size="small"
        extra={
          <Space>
            <Button size="small" icon={<PlusOutlined />} onClick={startNewBatch} disabled={running}>
              新建批次
            </Button>
            <Button
              size="small"
              icon={<ReloadOutlined />}
              onClick={() => { void loadExperiments(); refreshBatchStatus() }}
            >
              刷新
            </Button>
          </Space>
        }
      >
        <BatchTable
          batches={voiceBatches}
          currentRunKey={runExpDir}
          running={running}
          onLoad={loadBatch}
          onDelete={deleteBatch}
          emptyText="该声音还没有训练批次，执行数据预处理即可创建"
        />
        {usage && (
          <Text type="secondary" style={{ fontSize: 12 }}>
            当前批次可回收 {formatBytes(usage.reclaimable)}
            {usage.reclaimed > 0 ? `，已自动回收 ${formatBytes(usage.reclaimed)}` : ''}
          </Text>
        )}
      </Card>

      {runExpDir && (
        <Card 
          title={
            <Space>
              <FolderOpenOutlined />
              <span>批次目录结构</span>
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
                  title: '确认重置批次',
                  content: (
                    <Space orientation="vertical" size={8}>
                      <Text>确定要重置批次 <Text strong>"{workExpDir}"</Text> 吗？</Text>
                      <Text type="secondary">将清空工作位中的全部产物：</Text>
                      <ul style={{ margin: '8px 0', paddingLeft: 20 }}>
                        <li>模型检查点与最终模型 (*.pth)</li>
                        <li>特征索引 (added_*.index)</li>
                        <li>预处理切片、F0、HuBERT 特征与训练日志</li>
                      </ul>
                      <Text type="success">已归档的历史权重与批次清单会保留。</Text>
                      <Text type="secondary">产物会先移入回收站，7 天后自动清除。</Text>
                    </Space>
                  ),
                  okText: '确认重置',
                  okType: 'danger',
                  cancelText: '取消',
                  onOk: async () => {
                    try {
                      const result = await trainApi.resetExperiment(workExpDir)
                      if (result.ok) {
                        message.success({
                          content: `已重置批次，${(result.freedBytes / 1024 / 1024).toFixed(1)} MB 移入回收站，7 天后自动清除`,
                          duration: 8,
                        })
                        trainStore.clearExperiment(experimentKey)
                        setUsage(null)
                        refreshBatchStatus()
                        void loadExperiments()
                      } else {
                        message.error({ content: result.message, duration: 8 })
                      }
                    } catch (error) {
                      message.error(`重置失败：${String(error)}`)
                    }
                  },
                })
              }}
            >
              重置批次
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
