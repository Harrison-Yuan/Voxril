import { useState, useEffect, useRef } from 'react'
import { Form, message } from 'antd'
import { open } from '@tauri-apps/plugin-dialog'
import { inferApi } from '../../api/infer'
import { workspaceApi } from '../../api/workspace'
import { useUvrStore } from '../../store/uvrStore'
import { useInferStore } from '../../store/inferStore'
import { t } from '@/i18n/runtime'

export const DEFAULT_PARAMS = {
  modelName: '', transpose: 0, f0Method: 'rmvpe',
  indexRate: 0.75, filterRadius: 3, resampleSr: 0,
  // protect 必须 < 0.5，否则 pipeline 中的辅音/静音保护分支会被跳过，
  // 导致静音帧也吃满 index_rate 的检索特征，在未发声处合成出人声噪声。
  rmsMixRate: 1.0, protect: 0.33,
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
      const sel = await pickAudio(t('infer.pick_source_vocal'))
      if (sel) {
        setSourcePath(sel)
        setSourceName(sel.split('/').pop() || sel)
        useInferStore.getState().setSourcePath(sel)
      }
    } catch (e) { message.error(t('infer.select_audio_failed', { detail: String(e) })) }
  }

  const selectMerge = async (field: 'vocalPath' | 'instrumentalPath', title: string) => {
    try {
      const sel = await pickAudio(title)
      if (sel) mergeForm.setFieldValue(field, sel)
    } catch (e) { message.error(t('infer.select_audio_failed', { detail: String(e) })) }
  }

  const autoDetectInstrumental = () => {
    const uvrResult = useUvrStore.getState().result
    if (uvrResult?.ok && uvrResult.instrumentFiles.length > 0) {
      mergeForm.setFieldValue('instrumentalPath', uvrResult.instrumentFiles[0])
      return
    }
    if (!workspaceRoot) return
    inferApi.listDatasetAudio('instrumental')
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
    if (!workspaceRoot) { message.warning(t('infer.workspace_required')); return }
    try {
      const res = await inferApi.listDatasetAudio('instrumental')
      if (!res.files?.length) { message.warning(t('infer.no_instrumental_found')); return }
      if (res.files.length === 1) {
        mergeForm.setFieldValue('instrumentalPath', res.files[0].path)
        message.success(t('infer.selected', { name: res.files[0].name }))
      } else {
        setInstrumentalFiles(res.files)
      }
    } catch (e) { message.error(t('infer.load_instrumentals_failed', { detail: String(e) })) }
  }

  const handleConvert = async () => {
    const p = form.getFieldsValue()
    if (!p.modelName) { message.warning(t('infer.model_required')); return }
    if (!sourcePath) { message.warning(t('infer.source_required')); return }
    const result = await storeStartConvert({
      model_name: String(p.modelName || ''), source_path: sourcePath,
      transpose: toNum(p.transpose, 0), f0_method: String(p.f0Method || 'rmvpe'),
      index_rate: toNum(p.indexRate, 0.75), filter_radius: toNum(p.filterRadius, 3),
      resample_sr: toNum(p.resampleSr, 0), rms_mix_rate: toNum(p.rmsMixRate, 1.0),
      protect: toNum(p.protect, 0.33),
    })
    if (result.ok && result.output_path) {
      mergeForm.setFieldValue('vocalPath', result.output_path)
      autoDetectInstrumental()
      message.success(t('infer.done'))
    } else if (!result.ok) {
      message.error(result.message || t('infer.failed'))
    }
  }

  const handleMerge = async () => {
    const v = mergeForm.getFieldsValue()
    if (!v.vocalPath) { message.warning(t('infer.vocal_required')); return }
    if (!v.instrumentalPath) { message.warning(t('infer.accompaniment_required')); return }
    setMergeLoading(true); setMergeOutputPath('')
    try {
      const result = await inferApi.mergeAudio(String(v.vocalPath), String(v.instrumentalPath), v.vocalGain ?? 6)
      if (result.ok && result.output_path) { setMergeOutputPath(result.output_path); message.success(t('infer.merge_success')) }
      else { message.error(result.message || t('infer.merge_failed')) }
    } catch (e) { message.error(t('infer.merge_failed_detail', { detail: String(e) })) }
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
