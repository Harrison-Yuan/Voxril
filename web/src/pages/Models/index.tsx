import { useState, useEffect, useMemo, useCallback } from 'react'
import { Typography, Space, Button, Table, Modal, Input, message, Progress, Tag, Alert, Tabs } from 'antd'
import { listen } from '@tauri-apps/api/event'
import { AppstoreOutlined, DownloadOutlined, DeleteOutlined, CloudDownloadOutlined, CheckCircleOutlined, CloseCircleOutlined } from '@ant-design/icons'
import { inferApi } from '@/api/infer'
import type { BaseModelStatus as BaseModelStatusType } from '@/api/infer'
import { workspaceApi } from '@/api/workspace'
import { useModelDownloadStore } from '@/store/modelDownloadStore'
import { trainApi } from '@/pages/Train/services/trainApi'
import type { ExperimentSummary } from '@/pages/Train/services/trainApi'
import { BatchTable, formatRunDate } from '@/pages/Train/components/BatchTable'
import { formatBytes } from '@/lib/formatBytes'
import { useI18n } from '@/i18n'

const { Title, Text } = Typography

interface ModelItem {
  key: string
  type: 'base' | 'voice'
  category: string
  name: string
  path: string
  exists: boolean
  size_mb: number
  icon: string
  statusText: string
  statusColor: string
  /** 仅基础依赖：说明文案的 i18n 键 */
  descKey?: string
  /** 仅声音模型：批次摘要副标题 */
  subtitle?: string
}

/** 归档权重名：`<voice>-<run_id>-e<epoch>.pth` */
const ARCHIVE_RE = /(\d{6}-\d{4}(?:-\d+)?)-e(\d+)\.pth$/

interface VoiceModelInfo {
  voice: string
  /** 去重后的批次号 */
  batches: string[]
  /** 归档权重中的最大 epoch */
  latestEpoch: number | null
  /** 是否存在旧布局（未归档）模型 */
  hasLegacy: boolean
}

/** 把 `list_models` 的路径列表汇总为"按声音分组的批次摘要"。 */
function summarizeVoices(list: string[]): VoiceModelInfo[] {
  const map = new Map<string, VoiceModelInfo>()
  for (const model of list) {
    const parts = model.split('/').filter(Boolean)
    if (parts.length === 0) continue
    const voice = parts[0]
    const info = map.get(voice) ?? { voice, batches: [], latestEpoch: null, hasLegacy: false }
    if (model.includes('/weights/')) {
      const fname = parts[parts.length - 1]
      const hit = fname.match(ARCHIVE_RE)
      if (hit) {
        if (!info.batches.includes(hit[1])) info.batches.push(hit[1])
        const epoch = parseInt(hit[2], 10)
        if (Number.isFinite(epoch) && (info.latestEpoch === null || epoch > info.latestEpoch)) {
          info.latestEpoch = epoch
        }
      } else {
        info.hasLegacy = true
      }
    } else {
      info.hasLegacy = true
    }
    map.set(voice, info)
  }
  return Array.from(map.values()).sort((a, b) => a.voice.localeCompare(b.voice))
}

/** 基础依赖模型的静态元信息，name/desc 存 i18n 键，渲染处再 t()。 */
const BASE_MODEL_META: Record<string, { nameKey: string; icon: string; descKey: string }> = {
  hubert: { nameKey: 'dep.hubert.name', icon: '🧠', descKey: 'dep.hubert.desc' },
  rmvpe: { nameKey: 'dep.rmvpe.name', icon: '🎵', descKey: 'dep.rmvpe.desc' },
  bs_roformer: { nameKey: 'dep.bs_roformer.name', icon: '🎤', descKey: 'dep.bs_roformer.desc' },
  inst_main: { nameKey: 'dep.inst_main.name', icon: '🎙️', descKey: 'dep.inst_main.desc' },
  karaoke: { nameKey: 'dep.karaoke.name', icon: '🎶', descKey: 'dep.karaoke.desc' },
  voc_ft: { nameKey: 'dep.voc_ft.name', icon: '🎧', descKey: 'dep.voc_ft.desc' },
  kim_vocal: { nameKey: 'dep.kim_vocal.name', icon: '🎼', descKey: 'dep.kim_vocal.desc' },
  f0G40k: { nameKey: 'dep.f0G40k.name', icon: '🧬', descKey: 'dep.f0G40k.desc' },
  f0D40k: { nameKey: 'dep.f0D40k.name', icon: '🛡️', descKey: 'dep.f0D40k.desc' },
}

export default function ModelsPage() {
  const { t } = useI18n()
  const [voiceInfos, setVoiceInfos] = useState<VoiceModelInfo[]>([])
  /** 全部批次摘要，用于按声音展开批次明细。 */
  const [experiments, setExperiments] = useState<ExperimentSummary[]>([])
  const [loading, setLoading] = useState(false)
  const [baseModels, setBaseModels] = useState<BaseModelStatusType[]>([])
  const [baseModelsLoading, setBaseModelsLoading] = useState(false)
  const [storagePaths, setStoragePaths] = useState<string[]>([])
  const [activeTab, setActiveTab] = useState('all')

  // Import modal
  const [isImportVisible, setIsImportVisible] = useState(false)
  const [importSource, setImportSource] = useState('')
  const [importName, setImportName] = useState('')
  const [importing, setImporting] = useState(false)
  const [importLog, setImportLog] = useState('')
  const [importPercent, setImportPercent] = useState(0)

  const isDownloading = useModelDownloadStore((s) => s.isDownloading)
  const startDownload = useModelDownloadStore((s) => s.startDownload)
  const finishDownload = useModelDownloadStore((s) => s.finishDownload)
  const getDownloadItem = useModelDownloadStore((s) => s.getDownloadItem)
  const completedAt = useModelDownloadStore((s) => s.completedAt)
  // Subscribe to all downloads so the table re-renders on progress changes
  void useModelDownloadStore((s) => s.downloads)

  /** 基础依赖模型的展示名（用当前语言即时取词）。 */
  const baseModelName = useCallback(
    (id: string) => {
      const meta = BASE_MODEL_META[id]
      return meta ? t(meta.nameKey) : id
    },
    [t],
  )

  useEffect(() => {
    loadAll()
  }, [])

  // 启动自检整批下载结束后刷新模型状态（进度本身由全局订阅实时同步）
  useEffect(() => {
    let unlisten: (() => void) | undefined
    listen('model-download-done', () => {
      void loadAll()
    }).then((fn) => {
      unlisten = fn
    })
    return () => unlisten?.()
  }, [])

  // 单个模型下载完成后静默刷新一次状态，避免表格闪烁
  useEffect(() => {
    if (!completedAt) return
    const timer = setTimeout(() => {
      void loadAll(true)
    }, 600)
    return () => clearTimeout(timer)
  }, [completedAt])

  const loadAll = async (silent = false) => {
    if (!silent) {
      setLoading(true)
      setBaseModelsLoading(true)
    }
    try {
      const [modelsList, , baseStatus, paths, runs] = await Promise.all([
        inferApi.listModels(),
        workspaceApi.getWorkspace(),
        inferApi.getBaseModelsStatus(),
        inferApi.getModelStoragePaths(),
        trainApi.listExperiments(),
      ])
      setVoiceInfos(summarizeVoices(modelsList))
      setExperiments(runs)
      setBaseModels(baseStatus)
      setStoragePaths(paths)

      // 进入页面时若已有下载在跑（如启动自检），直接恢复其进度显示
      const active = await inferApi.getActiveModelDownload().catch(() => null)
      if (active) startDownload(active, baseModelName(active))
    } catch {
      if (!silent) message.error(t('models.load_list_failed'))
    } finally {
      if (!silent) {
        setLoading(false)
        setBaseModelsLoading(false)
      }
    }
  }

  /** 声音 → 去重后的批次（最新在前；旧布局版本目录不算批次）。 */
  const batchesByVoice = useMemo(() => {
    const map = new Map<string, ExperimentSummary[]>()
    for (const run of experiments) {
      if (run.legacy) continue
      const arr = map.get(run.voice) ?? []
      arr.push(run)
      map.set(run.voice, arr)
    }
    for (const arr of map.values()) arr.sort((a, b) => b.run_id.localeCompare(a.run_id))
    return map
  }, [experiments])

  /** 该声音的"当前模型"指针：默认取最新批次；归档权重永不覆盖，故重训不会让上一版消失。 */
  const currentRunKey = (voice: string) => {
    const latest = batchesByVoice.get(voice)?.[0]
    return latest ? `${voice}/${latest.run_id}` : undefined
  }

  // Build unified model list
  const allModels: ModelItem[] = useMemo(() => {
    const items: ModelItem[] = []

    // Base dependency models
    for (const bm of baseModels) {
      // 按 id 去重，防止后端返回重复项
      if (items.some((i) => i.key === `base-${bm.id}`)) continue
      const meta = BASE_MODEL_META[bm.id]
      items.push({
        key: `base-${bm.id}`,
        type: 'base',
        category: 'models.base',
        name: baseModelName(bm.id),
        path: bm.exists ? `${bm.id}` : '',
        exists: bm.exists,
        size_mb: bm.size_mb,
        icon: meta?.icon || '📦',
        descKey: meta?.descKey,
        statusText: bm.exists ? t('models.size_hint', { mb: bm.size_mb.toFixed(0) }) : t('models.status_missing'),
        statusColor: bm.exists ? 'success' : 'error',
      })
    }

    // User voice models：归档权重所在声音 + 仅有批次、尚未归档的声音
    const voiceNames = new Set(voiceInfos.map((info) => info.voice))
    for (const voice of batchesByVoice.keys()) voiceNames.add(voice)

    for (const voice of Array.from(voiceNames).sort((a, b) => a.localeCompare(b))) {
      const info = voiceInfos.find((i) => i.voice === voice)
      const batchCount = batchesByVoice.get(voice)?.length ?? info?.batches.length ?? 0
      const details: string[] = []
      details.push(batchCount > 0 ? t('models.batch_count', { n: batchCount }) : t('models.no_archived_batch'))
      if (info?.latestEpoch != null) details.push(t('models.latest_epoch', { n: info.latestEpoch }))
      if (info?.hasLegacy) details.push(t('models.has_legacy'))
      items.push({
        key: `voice-${voice}`,
        type: 'voice',
        category: 'models.voice',
        name: voice,
        path: voice,
        exists: true,
        size_mb: 0,
        icon: '🗣️',
        statusText: t('models.installed'),
        statusColor: 'success',
        subtitle: details.join(' · '),
      })
    }

    return items
  }, [baseModels, voiceInfos, batchesByVoice, t, baseModelName])

  const filteredModels = useMemo(() => {
    if (activeTab === 'all') return allModels
    if (activeTab === 'base') return allModels.filter((m) => m.type === 'base')
    return allModels.filter((m) => m.type === 'voice')
  }, [allModels, activeTab])

  const handleDeleteVoice = (modelName: string) => {
    Modal.confirm({
      title: t('models.delete_voice_confirm'),
      content: t('models.delete_voice_desc', { name: modelName }),
      okText: t('models.confirm_delete'),
      okType: 'danger',
      cancelText: t('common.cancel'),
      onOk: async () => {
        try {
          await inferApi.deleteModel(modelName)
          message.success(t('models.voice_deleted'))
          loadAll()
        } catch (e) {
          message.error(t('models.delete_failed', { msg: String(e) }))
        }
      },
    })
  }

  /** 删除批次：工作位 + 归档权重 + 批次清单；被成品引用时后端拒绝。 */
  const handleDeleteBatch = (batch: ExperimentSummary) => {
    const qualified = `${batch.voice}/${batch.run_id}`
    Modal.confirm({
      title: t('models.delete_batch_confirm'),
      content: (
        <Space orientation="vertical" size={8}>
          <Text>
            {t('models.delete_batch_prefix')} <Text strong>"{formatRunDate(batch.run_id)}"</Text>
            {t('models.delete_batch_suffix', { qualified })}
          </Text>
          <Text type="secondary">{t('models.delete_batch_size', { size: formatBytes(batch.bytes) })}</Text>
          <Text type="secondary">{t('models.delete_batch_trash')}</Text>
        </Space>
      ),
      okText: t('models.confirm_delete'),
      okType: 'danger',
      cancelText: t('common.cancel'),
      onOk: async () => {
        const result = await trainApi.purgeRun(qualified)
        if (!result.ok) {
          message.error({ content: result.message, duration: 8 })
          return
        }
        message.success({ content: t('models.batch_deleted', { size: formatBytes(result.freedBytes) }), duration: 6 })
        await loadAll()
      },
    })
  }

  const handleDeleteBase = (modelId: string) => {
    Modal.confirm({
      title: t('models.download_confirm'),
      content: t('models.delete_base_q', { name: baseModelName(modelId), tail: t('models.download_confirm_desc') }),
      okText: t('models.confirm_delete'),
      okType: 'danger',
      cancelText: t('common.cancel'),
      onOk: async () => {
        try {
          await inferApi.deleteBaseModel(modelId)
          message.success(t('models.base_deleted'))
          loadAll()
        } catch (e) {
          message.error(t('models.delete_failed', { msg: String(e) }))
        }
      },
    })
  }

  const handleImport = async () => {
    if (!importSource.trim()) {
      message.warning(t('models.source_required'))
      return
    }
    if (!importName.trim()) {
      message.warning(t('models.name_required'))
      return
    }

    setImporting(true)
    setImportLog(t('models.preparing_import'))
    setImportPercent(0)

    const res = await inferApi.importModel(importSource.trim(), importName.trim(), (prog) => {
      setImportLog(prog.message)
      if (prog.percent > 0) setImportPercent(prog.percent)
    })

    setImporting(false)
    if (res.ok) {
      message.success(t('models.import_success'))
      setIsImportVisible(false)
      setImportSource('')
      setImportName('')
      loadAll()
    } else {
      message.error(t('models.import_failed', { msg: res.message }))
    }
  }

  const handleSelectLocal = async () => {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog')
      const file = await open({
        multiple: false,
        filters: [{ name: 'Model Files', extensions: ['zip', 'pth'] }],
      })
      if (file && typeof file === 'string') {
        setImportSource(file)
        if (!importName) {
          const name = file.split(/[\\/]/).pop() || ''
          if (name) setImportName(name.replace(/\.(zip|pth)$/i, ''))
        }
      }
    } catch (e) {
      message.error(t('models.pick_file_failed', { msg: String(e) }))
    }
  }

  const handleDownloadBase = async (modelId: string) => {
    if (isDownloading(modelId)) return
    const name = baseModelName(modelId)
    startDownload(modelId, name)
    try {
      // 下载进度由全局订阅同步到此模型条目
      const ok = await inferApi.downloadBaseModel(modelId)
      if (ok) {
        message.success(t('dep.download_done', { name }))
        finishDownload(modelId)
        loadAll()
      } else {
        // ok 为 false 但无具体错误信息，从日志中取最后一条
        const dlItem = getDownloadItem(modelId)
        const errMsg = dlItem?.log || t('dep.unknown_error')
        message.error(t('dep.download_failed', { name, msg: errMsg }))
        finishDownload(modelId)
      }
    } catch (e) {
      message.error(t('dep.download_failed', { name, msg: String(e) }))
      finishDownload(modelId)
    }
  }

  const baseModelMissing = baseModels.some((m) => !m.exists)

  const columns = [
    {
      title: t('models.col_type'),
      dataIndex: 'category',
      key: 'category',
      width: 90,
      render: (cat: string) => (
        <Tag color={cat === 'models.base' ? 'blue' : 'green'} style={{ fontSize: 11 }}>
          {t(cat)}
        </Tag>
      ),
    },
    {
      title: t('models.col_model'),
      dataIndex: 'icon',
      key: 'icon',
      width: 50,
      render: (icon: string) => <span style={{ fontSize: 18 }}>{icon}</span>,
    },
    {
      title: t('models.col_name'),
      dataIndex: 'name',
      key: 'name',
      render: (name: string, record: ModelItem) => (
        <div style={{ lineHeight: 1.4 }}>
          <div style={{ fontWeight: 600, fontSize: 13, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{name}</div>
          {record.type === 'base' && (
            <div style={{ fontSize: 11, color: 'var(--color-text-tertiary)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', height: 16 }}>
              {record.descKey ? t(record.descKey) : ''}
            </div>
          )}
          {record.type === 'voice' && record.subtitle && (
            <div style={{ fontSize: 11, color: 'var(--color-text-tertiary)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', height: 16 }}>{record.subtitle}</div>
          )}
        </div>
      ),
    },
    {
      title: t('models.col_status'),
      key: 'status',
      width: 180,
      render: (_: any, record: ModelItem) =>
        record.exists ? (
          <Tag icon={<CheckCircleOutlined />} color="success">
            {record.statusText}
          </Tag>
        ) : (
          <Tag icon={<CloseCircleOutlined />} color="error">
            {record.statusText}
          </Tag>
        ),
    },
    {
      title: t('models.col_action'),
      key: 'action',
      width: 220,
      render: (_: any, record: ModelItem) => {
        if (record.type === 'base') {
          if (record.exists) {
            return (
              <Space>
                <Button size="small" icon={<CheckCircleOutlined />} disabled>
                  {t('models.status_ready')}
                </Button>
                <Button size="small" danger type="text" icon={<DeleteOutlined />} onClick={() => handleDeleteBase(record.key.replace('base-', ''))}>
                  {t('models.uninstall')}
                </Button>
              </Space>
            )
          }
          const modelId = record.key.replace('base-', '')
          const dlItem = getDownloadItem(modelId)
          return (
            <Space direction="vertical" size={4} style={{ width: '100%' }}>
              <Button size="small" type="primary" icon={<CloudDownloadOutlined />} loading={isDownloading(modelId)} onClick={() => handleDownloadBase(modelId)}>
                {isDownloading(modelId) ? t('models.status_downloading') : t('common.download')}
              </Button>
              {dlItem && (
                <div className="action-cell" style={{ lineHeight: '16px', overflow: 'hidden' }}>
                  <Progress
                    percent={dlItem.percent >= 0 ? dlItem.percent : undefined}
                    size="small"
                    status="active"
                    style={{ marginBottom: 0 }}
                    format={(p) => (dlItem.downloadedMb > 0 ? `${dlItem.downloadedMb.toFixed(0)} MB${p !== undefined ? ` (${p}%)` : ''}` : p !== undefined ? `${p}%` : t('dep.downloading'))}
                  />
                  <div style={{ fontSize: 11, color: 'var(--color-text-tertiary)', whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis', height: 16, lineHeight: '16px' }}>
                    {dlItem.log}
                  </div>
                </div>
              )}
            </Space>
          )
        }
        return (
          <Button danger type="text" icon={<DeleteOutlined />} onClick={() => handleDeleteVoice(record.name)}>
            {t('models.uninstall')}
          </Button>
        )
      },
    },
  ]

  const tabItems = [
    { key: 'all', label: `${t('models.all')} (${allModels.length})` },
    { key: 'base', label: `${t('models.base')} (${allModels.filter((m) => m.type === 'base').length})` },
    { key: 'voice', label: `${t('models.voice')} (${allModels.filter((m) => m.type === 'voice').length})` },
  ]

  return (
    <Space orientation="vertical" size="middle" style={{ width: '100%' }}>
      {/* Header */}
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start' }}>
        <div>
          <Title level={3} style={{ margin: 0, fontWeight: 600 }}>
            <Space>
              <AppstoreOutlined />
              {t('models.title')}
            </Space>
          </Title>
          <Text type="secondary" style={{ marginTop: 4, display: 'block', fontSize: 14 }}>
            {t('models.subtitle')}
          </Text>
        </div>
        <Button type="primary" icon={<DownloadOutlined />} onClick={() => setIsImportVisible(true)}>
          {t('models.import_voice')}
        </Button>
      </div>

      {/* Storage paths info */}
      <div
        style={{
          padding: '10px 16px',
          background: 'var(--color-bg-subtle)',
          borderRadius: 'var(--radius-lg)',
          border: '1px solid var(--color-border-subtle)',
          fontSize: 12,
          color: 'var(--color-text-tertiary)',
        }}
      >
        {storagePaths.length > 0 ? (
          <Space direction="vertical" size={2}>
            {storagePaths.map((p, i) => (
              <Text key={i} code style={{ fontSize: 12 }}>
                {p}
              </Text>
            ))}
          </Space>
        ) : (
          <Text type="secondary">{t('models.loading_paths')}</Text>
        )}
      </div>

      {/* Batch download missing base models */}
      {baseModelMissing && (
        <Alert
          type="warning"
          showIcon
          message={t('models.missing_alert')}
          description={`${t('models.missing_desc')}${t('models.missing_impact')}`}
          action={
            <Button
              size="small"
              icon={<CloudDownloadOutlined />}
              onClick={async () => {
                const missing = baseModels.filter((bm) => !bm.exists)
                await Promise.all(missing.map((bm) => handleDownloadBase(bm.id)))
              }}
            >
              {t('models.download_missing')}
            </Button>
          }
        />
      )}

      {/* Unified model table */}
      <div className="ant-card" style={{ padding: 0 }}>
        <Tabs
          activeKey={activeTab}
          onChange={setActiveTab}
          items={tabItems}
          tabBarStyle={{ padding: '0 20px', marginBottom: 0 }}
          tabBarExtraContent={
            <Space>
              <Button size="small" onClick={() => loadAll()} loading={loading || baseModelsLoading}>
                {t('common.refresh')}
              </Button>
            </Space>
          }
        />
        <style>{`
          .models-table .ant-table-row .ant-table-cell { height: 56px; overflow: hidden; }
          .models-table .ant-table-row .ant-table-cell .action-cell { height: 32px; overflow: hidden; }
        `}</style>
        <Table
          className="models-table"
          columns={columns}
          dataSource={filteredModels}
          loading={loading || baseModelsLoading}
          pagination={false}
          rowKey="key"
          size="small"
          expandable={{
            rowExpandable: (record) => record.type === 'voice' && (batchesByVoice.get(record.name)?.length ?? 0) > 0,
            expandedRowRender: (record) => <BatchTable batches={batchesByVoice.get(record.name) ?? []} currentRunKey={currentRunKey(record.name)} onDelete={handleDeleteBatch} showFingerprint />,
          }}
        />
      </div>

      {/* Import modal */}
      <Modal title={t('models.import_voice')} open={isImportVisible} onCancel={() => !importing && setIsImportVisible(false)} footer={null} maskClosable={!importing} closable={!importing}>
        <Space orientation="vertical" size="middle" style={{ width: '100%', marginTop: 16 }}>
          <div>
            <div style={{ marginBottom: 8 }}>
              <Text strong>{t('models.source_label')}</Text>
            </div>
            <Space.Compact style={{ width: '100%' }}>
              <Input value={importSource} onChange={(e) => setImportSource(e.target.value)} placeholder={t('models.source_placeholder')} disabled={importing} />
              <Button onClick={handleSelectLocal} disabled={importing}>
                {t('common.browse')}
              </Button>
            </Space.Compact>
            <Text type="secondary" style={{ fontSize: 12, marginTop: 4, display: 'block' }}>
              {t('models.supported_formats')}
            </Text>
          </div>

          <div>
            <div style={{ marginBottom: 8 }}>
              <Text strong>{t('models.name_label')}</Text>
            </div>
            <Input value={importName} onChange={(e) => setImportName(e.target.value)} placeholder={t('models.name_placeholder')} disabled={importing} />
            <Text type="secondary" style={{ fontSize: 12, marginTop: 4, display: 'block' }}>
              {t('models.save_location', { dir: storagePaths[1] ? `${storagePaths[1].split(': ')[1] || ''}` : '', name: importName || t('models.name_fallback') })}
            </Text>
          </div>

          {importing && (
            <div style={{ background: 'var(--color-bg-subtle)', padding: 12, borderRadius: 8 }}>
              <Text style={{ fontSize: 13, marginBottom: 8, display: 'block' }}>{importLog}</Text>
              <Progress percent={importPercent} status="active" />
            </div>
          )}

          <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8 }}>
            <Button onClick={() => setIsImportVisible(false)} disabled={importing}>
              {t('common.cancel')}
            </Button>
            <Button type="primary" onClick={handleImport} loading={importing}>
              {importing ? t('models.importing') : t('models.start_import')}
            </Button>
          </div>
        </Space>
      </Modal>
    </Space>
  )
}
