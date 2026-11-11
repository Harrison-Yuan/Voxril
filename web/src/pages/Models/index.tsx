import { useState, useEffect, useMemo } from 'react'
import { Typography, Space, Button, Table, Modal, Input, message, Progress, Tag, Alert, Tabs } from 'antd'
import {
  AppstoreOutlined, DownloadOutlined, DeleteOutlined,
  CloudDownloadOutlined,
  CheckCircleOutlined, CloseCircleOutlined,
} from '@ant-design/icons'
import { inferApi } from '@/api/infer'
import type { BaseModelStatus as BaseModelStatusType } from '@/api/infer'
import { workspaceApi } from '@/api/workspace'
import { useModelDownloadStore } from '@/store/modelDownloadStore'

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
}

const BASE_MODEL_META: Record<string, { name: string; icon: string; desc: string }> = {
  hubert: { name: 'ContentVec (HuBERT 特征提取)', icon: '🧠', desc: '训练和转换必需，提取人声深层特征' },
  rmvpe: { name: 'RMVPE (F0 音高提取)', icon: '🎵', desc: '训练和转换必需，预测 F0 音高曲线' },
  bs_roformer: { name: 'BS-RoFormer (人声/伴奏分离)', icon: '🎤', desc: 'UVR 分离必需，将伴奏与人声分离' },
  inst_main: { name: 'MDX-Net Main Vocals (主唱提取)', icon: '🎙️', desc: '仅提取主唱人声，过滤和声与背景音' },
  karaoke: { name: 'MDX-Net Karaoke 2 (去和声)', icon: '🎶', desc: '去除伴奏同时剥离背景和声' },
  voc_ft: { name: 'MDX-Net Voc FT (经典提取)', icon: '🎧', desc: '经典 MDX-Net 人声提取，稳定性好' },
  kim_vocal: { name: 'Kim Vocal 2 (清晰人声)', icon: '🎼', desc: '人声清晰度优化的 MDX-Net 模型' },
  f0G40k: { name: 'RVC v2 Generator f0G40k (预训练)', icon: '🧬', desc: '训练必需，提供 Generator 初始权重' },
  f0D40k: { name: 'RVC v2 Discriminator f0D40k (预训练)', icon: '🛡️', desc: '训练必需，提供 Discriminator 初始权重' },
}

export default function ModelsPage() {
  const [models, setModels] = useState<string[]>([])
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

  const isDownloading = useModelDownloadStore(s => s.isDownloading)
  const startDownload = useModelDownloadStore(s => s.startDownload)
  const updateLog = useModelDownloadStore(s => s.updateLog)
  const updateProgress = useModelDownloadStore(s => s.updateProgress)
  const finishDownload = useModelDownloadStore(s => s.finishDownload)
  const getDownloadItem = useModelDownloadStore(s => s.getDownloadItem)
  // Subscribe to all downloads so the table re-renders on progress changes
  void useModelDownloadStore(s => s.downloads)

  useEffect(() => {
    loadAll()
  }, [])

  const loadAll = async () => {
    setLoading(true)
    setBaseModelsLoading(true)
    try {
      const [modelsList, , baseStatus, paths] = await Promise.all([
        inferApi.listModels(),
        workspaceApi.getWorkspace(),
        inferApi.getBaseModelsStatus(),
        inferApi.getModelStoragePaths(),
      ])
      const rootModels = new Set<string>()
      modelsList.forEach(m => {
        const parts = m.split('/')
        if (parts.length > 0) rootModels.add(parts[0])
      })
      setModels(Array.from(rootModels).sort())
      setBaseModels(baseStatus)
      setStoragePaths(paths)
    } catch {
      message.error('加载模型列表失败')
    } finally {
      setLoading(false)
      setBaseModelsLoading(false)
    }
  }

  // Build unified model list
  const allModels: ModelItem[] = useMemo(() => {
    const items: ModelItem[] = []

    // Base dependency models
    for (const bm of baseModels) {
        // 按 id 去重，防止后端返回重复项
        if (items.some(i => i.key === `base-${bm.id}`)) continue
        const meta = BASE_MODEL_META[bm.id]
        items.push({
        key: `base-${bm.id}`,
        type: 'base',
        category: '基础依赖',
        name: meta?.name || bm.id,
        path: bm.exists ? `${bm.id}` : '',
        exists: bm.exists,
        size_mb: bm.size_mb,
        icon: meta?.icon || '📦',
        statusText: bm.exists ? `已就绪 (${bm.size_mb.toFixed(0)}MB)` : '未下载',
        statusColor: bm.exists ? 'success' : 'error',
      })
    }

    // User voice models
    for (const m of models) {
      items.push({
        key: `voice-${m}`,
        type: 'voice',
        category: '声音模型',
        name: m,
        path: m,
        exists: true,
        size_mb: 0,
        icon: '🗣️',
        statusText: '已安装',
        statusColor: 'success',
      })
    }

    return items
  }, [baseModels, models])

  const filteredModels = useMemo(() => {
    if (activeTab === 'all') return allModels
    if (activeTab === 'base') return allModels.filter(m => m.type === 'base')
    return allModels.filter(m => m.type === 'voice')
  }, [allModels, activeTab])

  const handleDeleteVoice = (modelName: string) => {
    Modal.confirm({
      title: '确认删除声音模型',
      content: `确定要彻底删除声音模型 "${modelName}" 吗？此操作不可恢复。`,
      okText: '确认删除',
      okType: 'danger',
      cancelText: '取消',
      onOk: async () => {
        try {
          await inferApi.deleteModel(modelName)
          message.success('声音模型已删除')
          loadAll()
        } catch (e) {
          message.error(`删除失败: ${String(e)}`)
        }
      }
    })
  }

  const handleDeleteBase = (modelId: string) => {
    const meta = BASE_MODEL_META[modelId]
    Modal.confirm({
      title: '确认删除基础模型',
      content: `确定要删除 "${meta?.name || modelId}" 吗？需要时可随时重新下载。`,
      okText: '确认删除',
      okType: 'danger',
      cancelText: '取消',
      onOk: async () => {
        try {
          await inferApi.deleteBaseModel(modelId)
          message.success('基础模型已删除')
          loadAll()
        } catch (e) {
          message.error(`删除失败: ${String(e)}`)
        }
      }
    })
  }

  const handleImport = async () => {
    if (!importSource.trim()) { message.warning('请输入模型下载链接或本地路径'); return }
    if (!importName.trim()) { message.warning('请输入模型名称'); return }

    setImporting(true)
    setImportLog('准备导入...')
    setImportPercent(0)

    const res = await inferApi.importModel(importSource.trim(), importName.trim(), (prog) => {
      setImportLog(prog.message)
      if (prog.percent > 0) setImportPercent(prog.percent)
    })

    setImporting(false)
    if (res.ok) {
      message.success('模型导入成功！')
      setIsImportVisible(false)
      setImportSource('')
      setImportName('')
      loadAll()
    } else {
      message.error(`导入失败: ${res.message}`)
    }
  }

  const handleSelectLocal = async () => {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog')
      const file = await open({
        multiple: false,
        filters: [{ name: 'Model Files', extensions: ['zip', 'pth'] }]
      })
      if (file && typeof file === 'string') {
        setImportSource(file)
        if (!importName) {
          const name = file.split(/[\\/]/).pop() || ''
          if (name) setImportName(name.replace(/\.(zip|pth)$/i, ''))
        }
      }
    } catch (e) {
      message.error(`选择文件失败: ${String(e)}`)
    }
  }

  const handleDownloadBase = async (modelId: string) => {
    if (isDownloading(modelId)) return
    const name = BASE_MODEL_META[modelId]?.name || modelId
    startDownload(modelId, name)
    try {
      const ok = await inferApi.downloadBaseModel(modelId, (prog) => {
        updateLog(modelId, prog.message)
        // 解析进度：优先用后端 percent 字段；同时从消息行解析已下载 MB
        // Python 输出格式: `[MODEL] 下载中 xxx ... 50% (123MB)`
        let pct = prog.percent > 0 ? prog.percent : -1
        let mb = 0
        const pctMatch = prog.message.match(/(\d+)%/)
        const mbMatch = prog.message.match(/(\d+(?:\.\d+)?)MB\)/)
        if (pctMatch) pct = parseInt(pctMatch[1], 10)
        if (mbMatch) mb = parseFloat(mbMatch[1])
        if (pct >= 0 || mb > 0) updateProgress(modelId, pct, mb)
      })
      if (ok) {
        message.success(`${name} 下载完成`)
        loadAll()
      } else {
        // ok 为 false 但无具体错误信息，从日志中取最后一条
        const dlItem = getDownloadItem(modelId)
        const errMsg = dlItem?.log || '未知错误'
        message.error(`${name} 下载失败: ${errMsg}`)
        finishDownload(modelId)
      }
    } catch (e) {
      message.error(`${name} 下载失败: ${String(e)}`)
      finishDownload(modelId)
    }
  }

  const baseModelMissing = baseModels.some(m => !m.exists)

  const columns = [
    {
      title: '类型',
      dataIndex: 'category',
      key: 'category',
      width: 90,
      render: (cat: string) => (
        <Tag color={cat === '基础依赖' ? 'blue' : 'green'} style={{ fontSize: 11 }}>
          {cat}
        </Tag>
      )
    },
    {
      title: '模型',
      dataIndex: 'icon',
      key: 'icon',
      width: 50,
      render: (icon: string) => <span style={{ fontSize: 18 }}>{icon}</span>
    },
    {
      title: '名称',
      dataIndex: 'name',
      key: 'name',
      render: (name: string, record: ModelItem) => (
        <div style={{ lineHeight: 1.4 }}>
          <div style={{ fontWeight: 600, fontSize: 13, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{name}</div>
          {record.type === 'base' && (
            <div style={{ fontSize: 11, color: 'var(--color-text-tertiary)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', height: 16 }}>
              {BASE_MODEL_META[record.key.replace('base-', '')]?.desc || ''}
            </div>
          )}
        </div>
      )
    },
    {
      title: '状态',
      key: 'status',
      width: 180,
      render: (_: any, record: ModelItem) => (
        record.exists
          ? <Tag icon={<CheckCircleOutlined />} color="success">{record.statusText}</Tag>
          : <Tag icon={<CloseCircleOutlined />} color="error">{record.statusText}</Tag>
      )
    },
    {
      title: '操作',
      key: 'action',
      width: 220,
      render: (_: any, record: ModelItem) => {
        if (record.type === 'base') {
          if (record.exists) {
            return (
              <Space>
                <Button size="small" icon={<CheckCircleOutlined />} disabled>已就绪</Button>
                <Button size="small" danger type="text" icon={<DeleteOutlined />}
                  onClick={() => handleDeleteBase(record.key.replace('base-', ''))}>
                  卸载
                </Button>
              </Space>
            )
          }
          const modelId = record.key.replace('base-', '')
          const dlItem = getDownloadItem(modelId)
          return (
            <Space direction="vertical" size={4} style={{ width: '100%' }}>
              <Button
                size="small" type="primary"
                icon={<CloudDownloadOutlined />}
                loading={isDownloading(modelId)}
                onClick={() => handleDownloadBase(modelId)}
              >
                {isDownloading(modelId) ? '下载中...' : '下载'}
              </Button>
              {dlItem && (
                <div className="action-cell" style={{ lineHeight: '16px', overflow: 'hidden' }}>
                  <Progress
                    percent={dlItem.percent >= 0 ? dlItem.percent : undefined}
                    size="small"
                    status="active"
                    style={{ marginBottom: 0 }}
                    format={p => dlItem.downloadedMb > 0
                      ? `${dlItem.downloadedMb.toFixed(0)} MB${p !== undefined ? ` (${p}%)` : ''}`
                      : (p !== undefined ? `${p}%` : '下载中')
                    }
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
          <Button danger type="text" icon={<DeleteOutlined />}
            onClick={() => handleDeleteVoice(record.name)}>
            卸载
          </Button>
        )
      }
    }
  ]

  const tabItems = [
    { key: 'all', label: `全部 (${allModels.length})` },
    { key: 'base', label: `基础依赖 (${allModels.filter(m => m.type === 'base').length})` },
    { key: 'voice', label: `声音模型 (${allModels.filter(m => m.type === 'voice').length})` },
  ]

  return (
    <Space orientation="vertical" size="middle" style={{ width: '100%' }}>
      {/* Header */}
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start' }}>
        <div>
          <Title level={3} style={{ margin: 0, fontWeight: 600 }}>
            <Space><AppstoreOutlined />模型管理</Space>
          </Title>
          <Text type="secondary" style={{ marginTop: 4, display: 'block', fontSize: 14 }}>
            统一管理项目依赖的基础模型和你训练/导入的声音模型
          </Text>
        </div>
        <Button type="primary" icon={<DownloadOutlined />} onClick={() => setIsImportVisible(true)}>
          导入声音模型
        </Button>
      </div>

      {/* Storage paths info */}
      <div style={{
        padding: '10px 16px',
        background: 'var(--color-bg-subtle)',
        borderRadius: 'var(--radius-lg)',
        border: '1px solid var(--color-border-subtle)',
        fontSize: 12,
        color: 'var(--color-text-tertiary)',
      }}>
        {storagePaths.length > 0 ? (
          <Space direction="vertical" size={2}>
            {storagePaths.map((p, i) => <Text key={i} code style={{ fontSize: 12 }}>{p}</Text>)}
          </Space>
        ) : (
          <Text type="secondary">加载存储路径中...</Text>
        )}
      </div>

      {/* Batch download missing base models */}
      {baseModelMissing && (
        <Alert
          type="warning" showIcon
          message="部分基础依赖模型未就绪"
          description="HuBERT、RMVPE、BS-RoFormer 或主唱提取模型缺失，训练、分离和转换功能可能无法正常运行。"
          action={
            <Button size="small" icon={<CloudDownloadOutlined />}
              onClick={async () => {
                const missing = baseModels.filter(bm => !bm.exists)
                await Promise.all(missing.map(bm => handleDownloadBase(bm.id)))
              }}>
              下载全部缺失
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
              <Button size="small" onClick={loadAll} loading={loading || baseModelsLoading}>刷新</Button>
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
        />
      </div>

      {/* Import modal */}
      <Modal
        title="导入声音模型"
        open={isImportVisible}
        onCancel={() => !importing && setIsImportVisible(false)}
        footer={null}
        maskClosable={!importing}
        closable={!importing}
      >
        <Space orientation="vertical" size="middle" style={{ width: '100%', marginTop: 16 }}>
          <div>
            <div style={{ marginBottom: 8 }}><Text strong>模型来源 (URL 或本地路径)</Text></div>
            <Space.Compact style={{ width: '100%' }}>
              <Input
                value={importSource}
                onChange={e => setImportSource(e.target.value)}
                placeholder="https://huggingface.co/.../model.zip 或 本地 .zip/.pth"
                disabled={importing}
              />
              <Button onClick={handleSelectLocal} disabled={importing}>浏览</Button>
            </Space.Compact>
            <Text type="secondary" style={{ fontSize: 12, marginTop: 4, display: 'block' }}>
              支持 .zip (自动解压提取 .pth + .index) 或直接 .pth 文件
            </Text>
          </div>

          <div>
            <div style={{ marginBottom: 8 }}><Text strong>保存名称</Text></div>
            <Input
              value={importName}
              onChange={e => setImportName(e.target.value)}
              placeholder="例如: my_favorite_voice"
              disabled={importing}
            />
            <Text type="secondary" style={{ fontSize: 12, marginTop: 4, display: 'block' }}>
              将保存在 {storagePaths[1] ? `${storagePaths[1].split(': ')[1] || ''}` : ''}{importName || '<名称>'}/v2_40k/
            </Text>
          </div>

          {importing && (
            <div style={{ background: 'var(--color-bg-subtle)', padding: 12, borderRadius: 8 }}>
              <Text style={{ fontSize: 13, marginBottom: 8, display: 'block' }}>{importLog}</Text>
              <Progress percent={importPercent} status="active" />
            </div>
          )}

          <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8 }}>
            <Button onClick={() => setIsImportVisible(false)} disabled={importing}>取消</Button>
            <Button type="primary" onClick={handleImport} loading={importing}>
              {importing ? '正在导入...' : '开始导入'}
            </Button>
          </div>
        </Space>
      </Modal>
    </Space>
  )
}
