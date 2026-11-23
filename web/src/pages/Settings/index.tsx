import { useState, useEffect } from 'react'
import { Input, Button, message, Alert, Select, Modal, Tag, Space, Spin } from 'antd'
import {
  FolderOpenOutlined, SaveOutlined, ReloadOutlined, SettingOutlined,
  DatabaseOutlined, ClearOutlined, ArrowRightOutlined,
} from '@ant-design/icons'
import { workspaceApi } from '@/api/database'
import { layoutApi } from '@/api/layout'
import type { LayoutStatus, MigrationPreview } from '@/api/layout'
import { storageApi } from '@/api/storage'
import type { StorageReport } from '@/api/storage'
import { useI18n, type Lang } from '../../i18n'
import { formatBytes } from '@/lib/formatBytes'

export default function SettingsPage() {
  const { t, lang, setLang } = useI18n()
  const [workspace, setWorkspace] = useState('')
  const [loading, setLoading] = useState(false)
  const [fetching, setFetching] = useState(true)

  // 布局升级向导
  const [layoutStatus, setLayoutStatus] = useState<LayoutStatus | null>(null)
  const [preview, setPreview] = useState<MigrationPreview | null>(null)
  const [previewOpen, setPreviewOpen] = useState(false)
  const [previewing, setPreviewing] = useState(false)
  const [migrating, setMigrating] = useState(false)

  // 清理中心
  const [report, setReport] = useState<StorageReport | null>(null)
  const [reportLoading, setReportLoading] = useState(false)
  const [reclaiming, setReclaiming] = useState(false)
  const [deepCleaning, setDeepCleaning] = useState(false)

  useEffect(() => { loadWorkspace() }, [])

  const loadWorkspace = async () => {
    setFetching(true)
    try {
      setWorkspace(await workspaceApi.get())
    } catch { message.error(t('common.failed')) }
    finally { setFetching(false) }
    loadLayoutStatus()
    loadReport()
  }

  const loadLayoutStatus = async () => {
    try { setLayoutStatus(await layoutApi.getStatus()) }
    catch { /* 状态仅用于展示，失败不打断页面 */ }
  }

  const loadReport = async () => {
    setReportLoading(true)
    try { setReport(await storageApi.getReport()) }
    catch (e: any) { message.error(`统计存储占用失败：${e?.message || e}`) }
    finally { setReportLoading(false) }
  }

  const handleSave = async () => {
    if (!workspace.trim()) { message.warning(t('common.no_data')); return }
    setLoading(true)
    try {
      const result = await workspaceApi.set(workspace.trim())
      message.success(t('common.done'))
      setWorkspace(result.workspace)
      loadLayoutStatus()
      loadReport()
    } catch (e: any) { message.error(e.message || t('common.failed')) }
    finally { setLoading(false) }
  }

  const handleSelectFolder = async () => {
    try {
      const path = await workspaceApi.select()
      if (path) { setWorkspace(path); message.success(t('common.done')) }
    } catch (e: any) { message.error(e.message || t('common.failed')) }
  }

  const openPreview = async () => {
    setPreviewing(true)
    try {
      const data = await layoutApi.preview()
      setPreview(data)
      setPreviewOpen(true)
    } catch (e: any) { message.error(`生成迁移预览失败：${e?.message || e}`) }
    finally { setPreviewing(false) }
  }

  const confirmMigrate = async () => {
    setMigrating(true)
    try {
      const result = await layoutApi.migrate()
      message.success(`升级完成：移动 ${result.moved} 项，共 ${formatBytes(result.total_bytes)}`)
      setPreviewOpen(false)
      setPreview(null)
      await loadLayoutStatus()
      await loadReport()
    } catch (e: any) { message.error(`升级失败：${e?.message || e}`) }
    finally { setMigrating(false) }
  }

  const handleReclaim = () => {
    const bytes = report?.reclaimable_now ?? 0
    Modal.confirm({
      title: lang === 'zh' ? '确认释放空间' : 'Reclaim space',
      content: lang === 'zh'
        ? `将删除所有批次的 1_16k_wavs、eval、TensorBoard 事件与孤立 STFT 缓存，预计释放 ${formatBytes(bytes)}。训练与转换不受影响。`
        : `This removes 1_16k_wavs, eval, TensorBoard events and orphan STFT caches for all batches. About ${formatBytes(bytes)} will be freed.`,
      okText: lang === 'zh' ? '释放' : 'Reclaim',
      cancelText: lang === 'zh' ? '取消' : 'Cancel',
      onOk: async () => {
        setReclaiming(true)
        try {
          const res = await storageApi.reclaimAll()
          message.success(`已释放 ${formatBytes(res.freed_bytes)}（${res.batches} 个批次）`)
          await loadReport()
        } catch (e: any) { message.error(`释放失败：${e?.message || e}`) }
        finally { setReclaiming(false) }
      },
    })
  }

  const handleDeepClean = async () => {
    setDeepCleaning(true)
    try {
      const res = await storageApi.deepClean()
      message.success(res.freed_gb > 0 ? `${res.message}` : res.message)
      await loadReport()
    } catch (e: any) { message.error(`清理失败：${e?.message || e}`) }
    finally { setDeepCleaning(false) }
  }

  const previewBlocked = (preview?.blockers.length ?? 0) > 0

  return (
    <div style={{ maxWidth: 720, margin: '0 auto' }}>
      {/* Header */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 12, marginBottom: 24 }}>
        <div className="icon-badge icon-badge--md" style={{ borderRadius: 10 }}>
          <SettingOutlined />
        </div>
        <div>
          <h1 className="page-header__title">{t('settings.title')}</h1>
          <p className="page-header__subtitle">{t('settings.workspace')}</p>
        </div>
      </div>

      {/* Language Card */}
      <div className="ant-card" style={{ padding: 0, marginBottom: 20 }}>
        <div style={{ padding: '12px 20px', borderBottom: '1px solid var(--color-border-subtle)', fontSize: 13, fontWeight: 600 }}>
          {t('settings.language')}
        </div>
        <div style={{ padding: 20 }}>
          <Select
            value={lang}
            onChange={(v: Lang) => setLang(v)}
            style={{ width: 200 }}
            options={[
              { value: 'zh', label: '中文' },
              { value: 'en', label: 'English' },
            ]}
          />
          <p style={{ fontSize: 12, color: 'var(--color-text-tertiary)', margin: '8px 0 0' }}>
            {lang === 'zh' ? '切换后立即生效，下次启动自动记住。' : 'Takes effect immediately. Your preference will be remembered on next launch.'}
          </p>
        </div>
      </div>

      {/* Workspace Card */}
      <div className="ant-card" style={{ padding: 0, marginBottom: 20 }}>
        <div style={{ padding: '12px 20px', borderBottom: '1px solid var(--color-border-subtle)', fontSize: 13, fontWeight: 600 }}>
          {t('settings.workspace')}
        </div>
        <div style={{ padding: 20, display: 'flex', flexDirection: 'column', gap: 16 }}>
          <p style={{ fontSize: 13, color: 'var(--color-text-secondary)', margin: 0 }}>
            {lang === 'zh' ? '所有训练数据、模型文件和索引文件都存储在此目录下' : 'All training data, model files, and index files are stored in this directory.'}
          </p>

          <div style={{
            padding: '10px 14px',
            background: 'var(--color-bg-base)',
            border: '1px solid var(--color-border-subtle)',
            borderRadius: 'var(--radius-lg)',
            display: 'flex', alignItems: 'center', gap: 10,
          }}>
            <FolderOpenOutlined style={{ color: 'var(--color-primary-hover)', flexShrink: 0 }} />
            <code style={{ fontSize: 12, color: 'var(--color-text-secondary)', wordBreak: 'break-all', flex: 1 }}>
              {fetching ? t('common.loading') : (workspace || t('common.no_data'))}
            </code>
          </div>

          <div style={{ display: 'flex', gap: 8 }}>
            <Input
              value={workspace}
              onChange={e => setWorkspace(e.target.value)}
              placeholder="/Users/username/Desktop/RVC-Workspace"
              size="large"
              style={{ flex: 1 }}
            />
            <Button icon={<FolderOpenOutlined />} size="large" onClick={handleSelectFolder}>
              {t('common.browse')}
            </Button>
          </div>

          <div style={{ display: 'flex', gap: 8 }}>
            <Button type="primary" icon={<SaveOutlined />} onClick={handleSave} loading={loading} size="large">
              {t('common.save')}
            </Button>
            <Button icon={<ReloadOutlined />} onClick={loadWorkspace} disabled={loading}>
              {t('common.refresh')}
            </Button>
          </div>

          <Alert
            message={lang === 'zh' ? '修改后需要重启应用才能生效' : 'Changes require a restart to take effect'}
            type="warning" showIcon style={{ fontSize: 13 }}
          />
        </div>
      </div>

      {/* Layout Upgrade Card */}
      <div className="ant-card" style={{ padding: 0, marginBottom: 20 }}>
        <div style={{ padding: '12px 20px', borderBottom: '1px solid var(--color-border-subtle)', display: 'flex', alignItems: 'center', gap: 8, fontSize: 13, fontWeight: 600 }}>
          <DatabaseOutlined />
          {lang === 'zh' ? '工作区结构' : 'Workspace Layout'}
        </div>
        <div style={{ padding: 20, display: 'flex', flexDirection: 'column', gap: 14 }}>
          {!layoutStatus ? (
            <Spin size="small" />
          ) : layoutStatus.is_v2 ? (
            <>
              <Space>
                <Tag color="success">{lang === 'zh' ? '新布局（v2）' : 'New layout (v2)'}</Tag>
                {layoutStatus.migrated_at && (
                  <span style={{ fontSize: 12, color: 'var(--color-text-tertiary)' }}>
                    {lang === 'zh' ? '升级于 ' : 'Migrated '}
                    {new Date(layoutStatus.migrated_at * 1000).toLocaleString()}
                  </span>
                )}
              </Space>
              <p style={{ fontSize: 13, color: 'var(--color-text-secondary)', margin: 0 }}>
                {lang === 'zh'
                  ? '目录已整理为「进 / 算 / 出 / 留」四层：inputs、work、outputs、models。'
                  : 'Directories are organized into inputs / work / outputs / models.'}
              </p>
            </>
          ) : (
            <>
              <Space>
                <Tag color="warning">{lang === 'zh' ? '旧布局（v1）' : 'Old layout (v1)'}</Tag>
              </Space>
              <p style={{ fontSize: 13, color: 'var(--color-text-secondary)', margin: 0 }}>
                {lang === 'zh'
                  ? '可将工作区整理为「进 / 算 / 出 / 留」四层结构：素材进 inputs，计算产物进 work，成品进 outputs，模型资产进 models。迁移前可先预览，失败会自动回滚。'
                  : 'Reorganize the workspace into inputs / work / outputs / models. You can preview first; any failure rolls back automatically.'}
              </p>
              <div>
                <Button type="primary" icon={<ArrowRightOutlined />} onClick={openPreview} loading={previewing}>
                  {lang === 'zh' ? '预览升级' : 'Preview upgrade'}
                </Button>
              </div>
            </>
          )}
        </div>
      </div>

      {/* Cleanup Center Card */}
      <div className="ant-card" style={{ padding: 0 }}>
        <div style={{ padding: '12px 20px', borderBottom: '1px solid var(--color-border-subtle)', display: 'flex', alignItems: 'center', gap: 8, fontSize: 13, fontWeight: 600 }}>
          <ClearOutlined />
          {lang === 'zh' ? '清理中心' : 'Cleanup Center'}
        </div>
        <div style={{ padding: 20, display: 'flex', flexDirection: 'column', gap: 16 }}>
          {reportLoading && !report ? (
            <Spin size="small" />
          ) : report ? (
            <>
              <div style={{
                display: 'flex', justifyContent: 'space-between', alignItems: 'center', gap: 12,
                padding: '12px 16px', background: 'var(--color-bg-base)',
                border: '1px solid var(--color-border-subtle)', borderRadius: 'var(--radius-lg)',
              }}>
                <div>
                  <div style={{ fontSize: 12, color: 'var(--color-text-tertiary)' }}>
                    {lang === 'zh' ? '可立即释放' : 'Reclaimable now'}
                  </div>
                  <div style={{ fontSize: 22, fontWeight: 600 }}>{formatBytes(report.reclaimable_now)}</div>
                </div>
                <Button
                  type="primary"
                  icon={<ClearOutlined />}
                  onClick={handleReclaim}
                  loading={reclaiming}
                  disabled={report.reclaimable_now <= 0}
                >
                  {lang === 'zh' ? '释放空间' : 'Reclaim'}
                </Button>
              </div>

              <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
                {report.zones.map(zone => (
                  <div key={zone.key} style={{ display: 'flex', justifyContent: 'space-between', gap: 12, fontSize: 13 }}>
                    <div style={{ minWidth: 0 }}>
                      <div style={{ color: 'var(--color-text-secondary)' }}>{zone.label}</div>
                      <div style={{ fontSize: 11, color: 'var(--color-text-tertiary)' }}>{zone.note}</div>
                    </div>
                    <div style={{ fontWeight: 600, whiteSpace: 'nowrap' }}>{formatBytes(zone.bytes)}</div>
                  </div>
                ))}
              </div>

              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', gap: 12, paddingTop: 4 }}>
                <span style={{ fontSize: 12, color: 'var(--color-text-tertiary)' }}>
                  {lang === 'zh' ? '构建缓存（应用本身）单独清理，不影响工作区数据' : 'Build caches are cleaned separately without touching workspace data.'}
                </span>
                <Button size="small" onClick={handleDeepClean} loading={deepCleaning}>
                  {lang === 'zh' ? '清理构建缓存' : 'Clean build cache'}
                </Button>
              </div>

              <div>
                <Button size="small" icon={<ReloadOutlined />} onClick={loadReport} loading={reportLoading}>
                  {lang === 'zh' ? '刷新统计' : 'Refresh'}
                </Button>
              </div>
            </>
          ) : (
            <Button size="small" onClick={loadReport} loading={reportLoading}>
              {lang === 'zh' ? '统计存储占用' : 'Scan storage'}
            </Button>
          )}
        </div>
      </div>

      {/* Migration Preview Modal */}
      <Modal
        title={lang === 'zh' ? '升级工作区结构（预览）' : 'Upgrade workspace layout (preview)'}
        open={previewOpen}
        onCancel={() => !migrating && setPreviewOpen(false)}
        maskClosable={!migrating}
        closable={!migrating}
        width={640}
        footer={[
          <Button key="cancel" onClick={() => setPreviewOpen(false)} disabled={migrating}>
            {lang === 'zh' ? '取消' : 'Cancel'}
          </Button>,
          <Button
            key="ok"
            type="primary"
            danger
            loading={migrating}
            disabled={previewBlocked || !preview}
            onClick={confirmMigrate}
          >
            {lang === 'zh' ? '开始升级' : 'Upgrade'}
          </Button>,
        ]}
      >
        {preview && (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
            {preview.blockers.length > 0 && (
              <Alert
                type="error"
                showIcon
                message={preview.blockers.join('；')}
              />
            )}
            <div style={{ fontSize: 13, color: 'var(--color-text-secondary)' }}>
              {lang === 'zh'
                ? `共 ${preview.items.length} 项，合计 ${formatBytes(preview.total_bytes)}。迁移为同卷重命名，瞬时完成；任一项失败会整体回滚。`
                : `${preview.items.length} items, ${formatBytes(preview.total_bytes)} total. Moves are same-volume renames and roll back on any failure.`}
            </div>
            <div style={{ maxHeight: 320, overflow: 'auto', border: '1px solid var(--color-border-subtle)', borderRadius: 'var(--radius-lg)' }}>
              {preview.items.length === 0 ? (
                <div style={{ padding: 16, fontSize: 13, color: 'var(--color-text-tertiary)' }}>
                  {lang === 'zh' ? '没有需要移动的内容，升级只会建立新目录结构。' : 'Nothing to move; the upgrade only creates the new directory structure.'}
                </div>
              ) : preview.items.map(item => (
                <div key={`${item.from}->${item.to}`} style={{
                  display: 'flex', justifyContent: 'space-between', gap: 12,
                  padding: '8px 12px', borderBottom: '1px solid var(--color-border-subtle)', fontSize: 12,
                }}>
                  <div style={{ minWidth: 0, wordBreak: 'break-all' }}>
                    <code>{item.from}</code>
                    <ArrowRightOutlined style={{ margin: '0 6px', color: 'var(--color-text-tertiary)' }} />
                    <code>{item.to}</code>
                  </div>
                  <span style={{ whiteSpace: 'nowrap', color: 'var(--color-text-tertiary)' }}>{formatBytes(item.bytes)}</span>
                </div>
              ))}
            </div>
          </div>
        )}
      </Modal>
    </div>
  )
}
