import { useState, useEffect, useRef } from 'react'
import { Form, message } from 'antd'
import { open } from '@tauri-apps/plugin-dialog'
import { invoke } from '@tauri-apps/api/core'
import { inferApi } from '../../api/infer'
import { workspaceApi } from '../../api/workspace'
import { useUvrStore } from '../../store/uvrStore'
import { useInferStore } from '../../store/inferStore'

export const DEFAULT_PARAMS = {
  modelName: '', transpose: 0, f0Method: 'rmvpe',
  indexRate: 0.75, filterRadius: 3, resampleSr: 0,
  rmsMixRate: 1.0, protect: 0.5,
}

export const toNum = (v: unknown, fb: number) => {
  const n = Number(v); return Number.isFinite(n) ? n : fb
}

export function useInferState() {
  const initialized = useRef(false)
  const [form] = Form.useForm()
  const [mergeForm] = Form.useForm()
  const [models, setModels] = useState<string[]>([])
  const [sourcePath, setSourcePath] = useState('')
  const [sourceName, setSourceName] = useState('')
  const [mergeOutputPath, setMergeOutputPath] = useState('')
  const [mergeLoading, setMergeLoading] = useState(false)
  const [workspaceRoot, setWorkspaceRoot] = useState('')
  const [fv, setFv] = useState(DEFAULT_PARAMS)
  const [instrumentalFiles, setInstrumentalFiles] = useState<{ name: string; path: string }[]>([])

  // Conversion state from store (survives page navigation)
  const storeLog = useInferStore(s => s.log)
  const storeLoading = useInferStore(s => s.loading)
  const storePct = useInferStore(s => s.pct)
  const storeCur = useInferStore(s => s.cur)
  const storeTot = useInferStore(s => s.tot)
  const storeOutputUrl = useInferStore(s => s.outputUrl)
  const storeStartConvert = useInferStore(s => s.startConvert)
  const storeEventId = useInferStore(s => s._eventId)
  const storeRestore = useInferStore(s => s.restoreFromEvent)

  useEffect(() => {
    if (initialized.current) return
    initialized.current = true
    inferApi.listModels().then(setModels).catch(() => setModels([]))
    workspaceApi.getWorkspace().then(r => {
      const root = (r.workspace || '').replace(/\/$/, '')
      setWorkspaceRoot(root)
      if (root) mergeForm.setFieldValue('outputDir', `${root}/mixed`)
    }).catch(() => {})
    setTimeout(autoDetectInstrumental, 500)

    // Reconnect to running conversion events if coming back to this page
    if (storeEventId) {
      storeRestore(storeEventId)
    }
  }, [])

  // Restore sourcePath from store if conversion was running
  useEffect(() => {
    if (storeLoading && !sourcePath) {
      const store = useInferStore.getState()
      if (store.sourcePath) {
        setSourcePath(store.sourcePath)
        setSourceName(store.sourcePath.split('/').pop() || store.sourcePath)
      }
    }
  }, [storeLoading])

  const pickAudio = async (title: string) => {
    const sel = await open({ directory: false, multiple: false, title, filters: [{ name: 'Audio', extensions: ['mp3', 'wav', 'flac', 'ogg', 'm4a', 'aac'] }] })
    return sel as string | null
  }

  const selectSource = async () => {
    try {
      const sel = await pickAudio('选择要转换的人声音频')
      if (sel) {
        setSourcePath(sel)
        setSourceName(sel.split('/').pop() || sel)
        useInferStore.getState().setSourcePath(sel)
      }
    } catch (e) { message.error(`选择音频失败：${String(e)}`) }
  }

  const selectMerge = async (field: 'vocalPath' | 'instrumentalPath', title: string) => {
    try {
      const sel = await pickAudio(title)
      if (sel) mergeForm.setFieldValue(field, sel)
    } catch (e) { message.error(`选择音频失败：${String(e)}`) }
  }

  const autoDetectInstrumental = () => {
    const uvrResult = useUvrStore.getState().result
    if (uvrResult?.ok && uvrResult.instrumentFiles.length > 0) {
      mergeForm.setFieldValue('instrumentalPath', uvrResult.instrumentFiles[0])
      return
    }
    if (!workspaceRoot) return
    invoke<{ files: { name: string; path: string }[] }>('list_audio_files', { dir: `${workspaceRoot}/separated/instrumental` })
      .then(res => {
        if (res.files?.length === 1) {
          mergeForm.setFieldValue('instrumentalPath', res.files[0].path)
        } else if (res.files?.length > 1) {
          setInstrumentalFiles(res.files)
        }
      })
      .catch(() => {})
  }

  const quickInstrumental = async () => {
    if (!workspaceRoot) { message.warning('请先配置工作目录'); return }
    try {
      const res = await invoke<{ files: { name: string; path: string }[] }>('list_audio_files', { dir: `${workspaceRoot}/separated/instrumental` })
      if (!res.files?.length) { message.warning('工作目录中未找到伴奏文件'); return }
      if (res.files.length === 1) {
        mergeForm.setFieldValue('instrumentalPath', res.files[0].path)
        message.success(`已选择：${res.files[0].name}`)
      } else {
        setInstrumentalFiles(res.files)
      }
    } catch (e) { message.error(`加载伴奏文件列表失败：${String(e)}`) }
  }

  const handleConvert = async () => {
    const p = form.getFieldsValue()
    if (!p.modelName) { message.warning('请先选择模型'); return }
    if (!sourcePath) { message.warning('请先上传音频文件'); return }
    const result = await storeStartConvert({
      model_name: String(p.modelName || ''), source_path: sourcePath,
      transpose: toNum(p.transpose, 0), f0_method: String(p.f0Method || 'rmvpe'),
      index_rate: toNum(p.indexRate, 0.75), filter_radius: toNum(p.filterRadius, 3),
      resample_sr: toNum(p.resampleSr, 0), rms_mix_rate: toNum(p.rmsMixRate, 1.0),
      protect: toNum(p.protect, 0.5),
    })
    if (result.ok && result.output_path) {
      mergeForm.setFieldValue('vocalPath', result.output_path)
      autoDetectInstrumental()
      message.success('转换完成')
    } else if (!result.ok) {
      message.error(result.message || '转换失败')
    }
  }

  const handleMerge = async () => {
    const v = mergeForm.getFieldsValue()
    if (!v.vocalPath) { message.warning('请先选择新的人声音频'); return }
    if (!v.instrumentalPath) { message.warning('请选择伴奏文件'); return }
    setMergeLoading(true); setMergeOutputPath('')
    try {
      const result = await inferApi.mergeAudio(String(v.vocalPath), String(v.instrumentalPath), String(v.outputDir || ''), v.vocalGain ?? 6)
      if (result.ok && result.output_path) { setMergeOutputPath(result.output_path); message.success('伴奏和人声已合成完成') }
      else { message.error(result.message || '合成失败') }
    } catch (e) { message.error(`合成失败：${String(e)}`) }
    finally { setMergeLoading(false) }
  }

  return {
    form, mergeForm, models, sourcePath, sourceName, mergeOutputPath, mergeLoading,
    workspaceRoot,
    log: storeLog, loading: storeLoading, pct: storePct,
    cur: storeCur, tot: storeTot, outputUrl: storeOutputUrl,
    fv, setFv,
    instrumentalFiles, setInstrumentalFiles,
    selectSource, selectMerge, quickInstrumental, handleConvert, handleMerge,
  }
}
