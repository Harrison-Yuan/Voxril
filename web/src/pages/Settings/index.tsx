import { useState, useEffect } from 'react'
import { Input, Button, message, Alert, Select, Modal, Tag, Space, Spin, Progress } from 'antd'
import {
  FolderOpenOutlined,
  SaveOutlined,
  ReloadOutlined,
  SettingOutlined,
  DatabaseOutlined,
  ClearOutlined,
  ArrowRightOutlined,
  CloudDownloadOutlined,
  SyncOutlined,
  ExportOutlined,
  InfoCircleOutlined,
  SwapOutlined,
} from '@ant-design/icons'
import { invoke } from '@tauri-apps/api/core'
import { getVersion } from '@tauri-apps/api/app'
import { workspaceApi } from '@/api/database'
import { downloadApi } from '@/api/download'
import type { DownloadSettings, DownloadSource, PipSource } from '@/api/download'
import { updateApi } from '@/api/update'
import { useUpdaterStore } from '@/store/updaterStore'
import { envApi } from '@/api/env'
import type { DataDirStatus } from '@/api/env'
import { GUIDE_URLS, RELEASES_URL } from '@/lib/links'
import { layoutApi } from '@/api/layout'
import type { LayoutStatus, MigrationPreview } from '@/api/layout'
import { storageApi } from '@/api/storage'
import type { StorageReport } from '@/api/storage'
import { useI18n, type Lang } from '../../i18n'
import { formatBytes } from '@/lib/formatBytes'

/** 下载源选项，顺序即设置页展示顺序。 */
const SOURCE_OPTIONS: { value: DownloadSource; labelKey: string }[] = [
  { value: 'auto', labelKey: 'settings.src_auto' },
  { value: 'official', labelKey: 'settings.src_official' },
  { value: 'cn', labelKey: 'settings.src_cn_mirror' },
  { value: 'custom', labelKey: 'settings.src_custom' },
]

/** pip 源选项。 */
const PIP_SOURCE_OPTIONS: { value: PipSource; labelKey: string }[] = [
  { value: 'auto', labelKey: 'settings.src_auto' },
  { value: 'official', labelKey: 'settings.src_pypi_official' },
  { value: 'tuna', labelKey: 'settings.src_pypi_tuna' },
  { value: 'aliyun', labelKey: 'settings.src_pypi_aliyun' },
  { value: 'custom', labelKey: 'settings.src_custom' },
]

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

  // 模型 / 依赖下载源
  const [download, setDownload] = useState<DownloadSettings>({
    source: 'auto',
    hf_endpoint: '',
    gh_proxy: '',
    pip_source: 'auto',
    pip_index: '',
  })
  const [downloadSaving, setDownloadSaving] = useState(false)

  // 关于 / 检查更新
  const [checkingUpdate, setCheckingUpdate] = useState(false)
  const [currentVersion, setCurrentVersion] = useState('')

  // 数据目录（Python 环境 + 基础模型）
  const [dataDir, setDataDir] = useState<DataDirStatus | null>(null)
  const [dataMigrating, setDataMigrating] = useState(false)
  const [dataMigratePercent, setDataMigratePercent] = useState(0)
  const [dataMigrateLog, setDataMigrateLog] = useState('')
  const [dataMigrateResult, setDataMigrateResult] = useState<string | null>(null)

  const loadDataDir = async () => {
    try {
      setDataDir(await envApi.getDataDirStatus())
    } catch {
      /* 数据目录状态仅用于展示 */
    }
  }

  const loadDownloadSettings = async () => {
    try {
      setDownload(await downloadApi.get())
    } catch {
      /* 下载设置仅用于配置，读取失败保持默认值 */
    }
  }

  const loadWorkspace = async () => {
    setFetching(true)
    try {
      setWorkspace(await workspaceApi.get())
    } catch {
      message.error(t('common.failed'))
    } finally {
      setFetching(false)
    }
    loadLayoutStatus()
    loadReport()
  }

  const loadLayoutStatus = async () => {
    try {
      setLayoutStatus(await layoutApi.getStatus())
    } catch {
      /* 状态仅用于展示，失败不打断页面 */
    }
  }

  const loadReport = async () => {
    setReportLoading(true)
    try {
      setReport(await storageApi.getReport())
    } catch (e: any) {
      message.error(t('settings.storage_scan_failed', { err: e?.message || e }))
    } finally {
      setReportLoading(false)
    }
  }

  useEffect(() => {
    loadWorkspace()
    loadDownloadSettings()
    loadDataDir()
    getVersion()
      .then(setCurrentVersion)
      .catch(() => {})
  }, [])

  const handleSaveDownload = async () => {
    setDownloadSaving(true)
    try {
      await downloadApi.set(download)
      message.success(t('common.done'))
    } catch (e: any) {
      message.error(t('settings.download_save_failed', { err: e?.message || e }))
    } finally {
      setDownloadSaving(false)
    }
  }

  const handleCheckUpdate = async () => {
    setCheckingUpdate(true)
    try {
      // 走应用内更新：发现新版本会在标题栏弹出下载 / 安装面板
      await useUpdaterStore.getState().check(true)
      const updater = useUpdaterStore.getState()
      if (updater.phase === 'uptodate') {
        message.success(t('update.up_to_date') + (currentVersion ? ` · v${currentVersion}` : ''))
      }
    } finally {
      setCheckingUpdate(false)
    }
  }

  const openDataDir = async () => {
    if (!dataDir) return
    try {
      await invoke('open_in_finder', { path: dataDir.path })
    } catch {
      /* 打开目录失败不打断设置页 */
    }
  }

  const handleChangeDataDir = async () => {
    if (!dataDir) return
    const { open } = await import('@tauri-apps/plugin-dialog')
    const dir = await open({ directory: true, multiple: false, title: t('datadir.pick_title'), defaultPath: dataDir.path })
    if (!dir || typeof dir !== 'string') return

    Modal.confirm({
      title: t('datadir.migrate_confirm_title'),
      content: (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
          <span>{t('datadir.migrate_confirm_desc')}</span>
          <code style={{ fontSize: 12, wordBreak: 'break-all' }}>{dir}</code>
        </div>
      ),
      okText: t('common.continue'),
      cancelText: t('common.cancel'),
      onOk: async () => {
        setDataMigrating(true)
        setDataMigratePercent(0)
        setDataMigrateLog('')
        try {
          const msg = await envApi.migrateDataDir(dir, (p) => {
            setDataMigratePercent(p.percent)
            setDataMigrateLog(p.message)
          })
          setDataMigratePercent(100)
          await loadDataDir()
          setDataMigrateResult(msg)
        } catch (e: unknown) {
          message.error(t('settings.migrate_failed_detail', { err: e instanceof Error ? e.message : String(e) }))
        } finally {
          setDataMigrating(false)
        }
      },
    })
  }

  const handleSave = async () => {
    if (!workspace.trim()) {
      message.warning(t('common.no_data'))
      return
    }
    setLoading(true)
    try {
      const result = await workspaceApi.set(workspace.trim())
      message.success(t('common.done'))
      setWorkspace(result.workspace)
      loadLayoutStatus()
      loadReport()
    } catch (e: any) {
      message.error(e.message || t('common.failed'))
    } finally {
      setLoading(false)
    }
  }

  const handleSelectFolder = async () => {
    try {
      const path = await workspaceApi.select()
      if (path) {
        setWorkspace(path)
        message.success(t('common.done'))
      }
    } catch (e: any) {
      message.error(e.message || t('common.failed'))
    }
  }

  const openPreview = async () => {
    setPreviewing(true)
    try {
      const data = await layoutApi.preview()
      setPreview(data)
      setPreviewOpen(true)
    } catch (e: any) {
      message.error(t('settings.preview_failed', { err: e?.message || e }))
    } finally {
      setPreviewing(false)
    }
  }

  const confirmMigrate = async () => {
    setMigrating(true)
    try {
      const result = await layoutApi.migrate()
      message.success(t('settings.migrate_done', { n: result.moved, size: formatBytes(result.total_bytes) }))
      setPreviewOpen(false)
      setPreview(null)
      await loadLayoutStatus()
      await loadReport()
    } catch (e: any) {
      message.error(t('settings.migrate_failed', { err: e?.message || e }))
    } finally {
      setMigrating(false)
    }
  }

  const handleReclaim = () => {
    const bytes = report?.reclaimable_now ?? 0
    Modal.confirm({
      title: t('settings.reclaim_confirm_title'),
      content: t('settings.reclaim_confirm_desc', { size: formatBytes(bytes) }),
      okText: t('settings.reclaim_confirm_ok'),
      cancelText: t('common.cancel'),
      onOk: async () => {
        setReclaiming(true)
        try {
          const res = await storageApi.reclaimAll()
          message.success(t('settings.reclaim_done', { size: formatBytes(res.freed_bytes), n: res.batches }))
          await loadReport()
        } catch (e: any) {
          message.error(t('settings.reclaim_failed', { err: e?.message || e }))
        } finally {
          setReclaiming(false)
        }
      },
    })
  }

  const handleDeepClean = async () => {
    setDeepCleaning(true)
    try {
      const res = await storageApi.deepClean()
      message.success(res.freed_gb > 0 ? `${res.message}` : res.message)
      await loadReport()
    } catch (e: any) {
      message.error(t('settings.deep_clean_failed', { err: e?.message || e }))
    } finally {
      setDeepCleaning(false)
    }
  }

  const previewBlocked = (preview?.blockers.length ?? 0) > 0

  const sourceHint = (() => {
    switch (download.source) {
      case 'official':
        return t('settings.source_hint_official')
      case 'cn':
        return t('settings.source_hint_cn')
      case 'custom':
        return t('settings.source_hint_custom')
      default:
        return t('settings.source_hint_auto')
    }
  })()

  const pipHint = (() => {
    switch (download.pip_source) {
      case 'official':
        return t('settings.pip_hint_official')
      case 'tuna':
        return t('settings.pip_hint_tuna')
      case 'aliyun':
        return t('settings.pip_hint_aliyun')
      case 'custom':
        return t('settings.pip_hint_custom')
      default:
        return t('settings.pip_hint_auto')
    }
  })()

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
        <div
          style={{
            padding: '12px 20px',
            borderBottom: '1px solid var(--color-border-subtle)',
            fontSize: 13,
            fontWeight: 600,
          }}
        >
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
          <p style={{ fontSize: 12, color: 'var(--color-text-tertiary)', margin: '8px 0 0' }}>{t('settings.language_hint')}</p>
        </div>
      </div>

      {/* Model Download Source Card */}
      <div className="ant-card" style={{ padding: 0, marginBottom: 20 }}>
        <div
          style={{
            padding: '12px 20px',
            borderBottom: '1px solid var(--color-border-subtle)',
            display: 'flex',
            alignItems: 'center',
            gap: 8,
            fontSize: 13,
            fontWeight: 600,
          }}
        >
          <CloudDownloadOutlined />
          {t('settings.download_card')}
        </div>
        <div style={{ padding: 20, display: 'flex', flexDirection: 'column', gap: 14 }}>
          <p style={{ fontSize: 13, color: 'var(--color-text-secondary)', margin: 0 }}>{t('settings.download_desc')}</p>

          <Select
            value={download.source}
            onChange={(v: DownloadSource) => setDownload((d) => ({ ...d, source: v }))}
            style={{ width: 240 }}
            options={SOURCE_OPTIONS.map((o) => ({ value: o.value, label: t(o.labelKey) }))}
          />
          <p style={{ fontSize: 12, color: 'var(--color-text-tertiary)', margin: 0 }}>{sourceHint}</p>

          {download.source === 'custom' && (
            <>
              <div>
                <div
                  style={{
                    fontSize: 11,
                    fontWeight: 600,
                    color: 'var(--color-text-tertiary)',
                    textTransform: 'uppercase',
                    letterSpacing: '0.06em',
                    marginBottom: 8,
                  }}
                >
                  {t('settings.hf_endpoint')}
                </div>
                <Input value={download.hf_endpoint} onChange={(e) => setDownload((d) => ({ ...d, hf_endpoint: e.target.value }))} placeholder="https://hf-mirror.com" />
              </div>
              <div>
                <div
                  style={{
                    fontSize: 11,
                    fontWeight: 600,
                    color: 'var(--color-text-tertiary)',
                    textTransform: 'uppercase',
                    letterSpacing: '0.06em',
                    marginBottom: 8,
                  }}
                >
                  {t('settings.gh_proxy')}
                </div>
                <Input value={download.gh_proxy} onChange={(e) => setDownload((d) => ({ ...d, gh_proxy: e.target.value }))} placeholder="https://ghfast.top" />
              </div>
            </>
          )}

          <div style={{ height: 1, background: 'var(--color-border-subtle)', margin: '4px 0' }} />

          <div>
            <div style={{ fontSize: 11, fontWeight: 600, color: 'var(--color-text-tertiary)', textTransform: 'uppercase', letterSpacing: '0.06em', marginBottom: 8 }}>{t('settings.pip_source')}</div>
            <Select
              value={download.pip_source}
              onChange={(v: PipSource) => setDownload((d) => ({ ...d, pip_source: v }))}
              style={{ width: 240 }}
              options={PIP_SOURCE_OPTIONS.map((o) => ({ value: o.value, label: t(o.labelKey) }))}
            />
          </div>
          <p style={{ fontSize: 12, color: 'var(--color-text-tertiary)', margin: 0 }}>{pipHint}</p>

          {download.pip_source === 'custom' && (
            <div>
              <div style={{ fontSize: 11, fontWeight: 600, color: 'var(--color-text-tertiary)', textTransform: 'uppercase', letterSpacing: '0.06em', marginBottom: 8 }}>{t('settings.pip_index')}</div>
              <Input value={download.pip_index} onChange={(e) => setDownload((d) => ({ ...d, pip_index: e.target.value }))} placeholder="https://pypi.tuna.tsinghua.edu.cn/simple" />
            </div>
          )}

          <div>
            <Button type="primary" icon={<SaveOutlined />} onClick={handleSaveDownload} loading={downloadSaving}>
              {t('common.save')}
            </Button>
          </div>
        </div>
      </div>

      {/* Workspace Card */}
      <div className="ant-card" style={{ padding: 0, marginBottom: 20 }}>
        <div
          style={{
            padding: '12px 20px',
            borderBottom: '1px solid var(--color-border-subtle)',
            fontSize: 13,
            fontWeight: 600,
          }}
        >
          {t('settings.workspace')}
        </div>
        <div style={{ padding: 20, display: 'flex', flexDirection: 'column', gap: 16 }}>
          <p style={{ fontSize: 13, color: 'var(--color-text-secondary)', margin: 0 }}>{t('settings.workspace_desc')}</p>

          <div
            style={{
              padding: '10px 14px',
              background: 'var(--color-bg-base)',
              border: '1px solid var(--color-border-subtle)',
              borderRadius: 'var(--radius-lg)',
              display: 'flex',
              alignItems: 'center',
              gap: 10,
            }}
          >
            <FolderOpenOutlined style={{ color: 'var(--color-primary-hover)', flexShrink: 0 }} />
            <code style={{ fontSize: 12, color: 'var(--color-text-secondary)', wordBreak: 'break-all', flex: 1 }}>{fetching ? t('common.loading') : workspace || t('common.no_data')}</code>
          </div>

          <div style={{ display: 'flex', gap: 8 }}>
            <Input value={workspace} onChange={(e) => setWorkspace(e.target.value)} placeholder="/Users/username/Desktop/RVC-Workspace" size="large" style={{ flex: 1 }} />
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

          <Alert message={t('settings.workspace_restart_warn')} type="warning" showIcon style={{ fontSize: 13 }} />
        </div>
      </div>

      {/* Data Directory Card */}
      <div className="ant-card" style={{ padding: 0, marginBottom: 20 }}>
        <div
          style={{
            padding: '12px 20px',
            borderBottom: '1px solid var(--color-border-subtle)',
            display: 'flex',
            alignItems: 'center',
            gap: 8,
            fontSize: 13,
            fontWeight: 600,
          }}
        >
          <FolderOpenOutlined />
          {t('datadir.card_title')}
        </div>
        <div style={{ padding: 20, display: 'flex', flexDirection: 'column', gap: 14 }}>
          <p style={{ fontSize: 13, color: 'var(--color-text-secondary)', margin: 0 }}>{t('datadir.card_desc')}</p>

          <div
            style={{
              padding: '10px 14px',
              background: 'var(--color-bg-base)',
              border: '1px solid var(--color-border-subtle)',
              borderRadius: 'var(--radius-lg)',
              display: 'flex',
              alignItems: 'center',
              gap: 10,
            }}
          >
            <FolderOpenOutlined style={{ color: 'var(--color-primary-hover)', flexShrink: 0 }} />
            <code style={{ fontSize: 12, color: 'var(--color-text-secondary)', wordBreak: 'break-all', flex: 1 }}>{dataDir?.path || t('common.loading')}</code>
          </div>

          <Space wrap>
            <Button icon={<SwapOutlined />} onClick={handleChangeDataDir} loading={dataMigrating} disabled={dataMigrating || !dataDir}>
              {t('datadir.change')}
            </Button>
            <Button icon={<FolderOpenOutlined />} onClick={openDataDir} disabled={!dataDir}>
              {t('datadir.open')}
            </Button>
          </Space>

          {dataMigrating && (
            <div>
              <Progress percent={dataMigratePercent} size="small" status="active" />
              <span style={{ fontSize: 12, color: 'var(--color-text-tertiary)' }}>{dataMigrateLog}</span>
            </div>
          )}

          <Alert type="info" showIcon style={{ fontSize: 12 }} title={t('datadir.migrate_note')} />
        </div>
      </div>

      {/* Layout Upgrade Card */}
      <div className="ant-card" style={{ padding: 0, marginBottom: 20 }}>
        <div
          style={{
            padding: '12px 20px',
            borderBottom: '1px solid var(--color-border-subtle)',
            display: 'flex',
            alignItems: 'center',
            gap: 8,
            fontSize: 13,
            fontWeight: 600,
          }}
        >
          <DatabaseOutlined />
          {t('settings.layout_card')}
        </div>
        <div style={{ padding: 20, display: 'flex', flexDirection: 'column', gap: 14 }}>
          {!layoutStatus ? (
            <Spin size="small" />
          ) : layoutStatus.is_v2 ? (
            <>
              <Space>
                <Tag color="success">{t('settings.layout_v2_tag')}</Tag>
                {layoutStatus.migrated_at && (
                  <span style={{ fontSize: 12, color: 'var(--color-text-tertiary)' }}>
                    {t('settings.layout_migrated_at')}
                    {new Date(layoutStatus.migrated_at * 1000).toLocaleString()}
                  </span>
                )}
              </Space>
              <p style={{ fontSize: 13, color: 'var(--color-text-secondary)', margin: 0 }}>{t('settings.layout_v2_desc')}</p>
            </>
          ) : (
            <>
              <Space>
                <Tag color="warning">{t('settings.layout_v1_tag')}</Tag>
              </Space>
              <p style={{ fontSize: 13, color: 'var(--color-text-secondary)', margin: 0 }}>{t('settings.layout_v1_desc')}</p>
              <div>
                <Button type="primary" icon={<ArrowRightOutlined />} onClick={openPreview} loading={previewing}>
                  {t('settings.layout_preview_btn')}
                </Button>
              </div>
            </>
          )}
        </div>
      </div>

      {/* Cleanup Center Card */}
      <div className="ant-card" style={{ padding: 0 }}>
        <div
          style={{
            padding: '12px 20px',
            borderBottom: '1px solid var(--color-border-subtle)',
            display: 'flex',
            alignItems: 'center',
            gap: 8,
            fontSize: 13,
            fontWeight: 600,
          }}
        >
          <ClearOutlined />
          {t('settings.cleanup_card')}
        </div>
        <div style={{ padding: 20, display: 'flex', flexDirection: 'column', gap: 16 }}>
          {reportLoading && !report ? (
            <Spin size="small" />
          ) : report ? (
            <>
              <div
                style={{
                  display: 'flex',
                  justifyContent: 'space-between',
                  alignItems: 'center',
                  gap: 12,
                  padding: '12px 16px',
                  background: 'var(--color-bg-base)',
                  border: '1px solid var(--color-border-subtle)',
                  borderRadius: 'var(--radius-lg)',
                }}
              >
                <div>
                  <div style={{ fontSize: 12, color: 'var(--color-text-tertiary)' }}>{t('settings.reclaimable_now')}</div>
                  <div style={{ fontSize: 22, fontWeight: 600 }}>{formatBytes(report.reclaimable_now)}</div>
                </div>
                <Button type="primary" icon={<ClearOutlined />} onClick={handleReclaim} loading={reclaiming} disabled={report.reclaimable_now <= 0}>
                  {t('settings.reclaim')}
                </Button>
              </div>

              <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
                {report.zones.map((zone) => (
                  <div key={zone.key} style={{ display: 'flex', justifyContent: 'space-between', gap: 12, fontSize: 13 }}>
                    <div style={{ minWidth: 0 }}>
                      <div style={{ color: 'var(--color-text-secondary)' }}>{zone.label}</div>
                      <div style={{ fontSize: 11, color: 'var(--color-text-tertiary)' }}>{zone.note}</div>
                    </div>
                    <div style={{ fontWeight: 600, whiteSpace: 'nowrap' }}>{formatBytes(zone.bytes)}</div>
                  </div>
                ))}
              </div>

              <div
                style={{
                  display: 'flex',
                  justifyContent: 'space-between',
                  alignItems: 'center',
                  gap: 12,
                  paddingTop: 4,
                }}
              >
                <span style={{ fontSize: 12, color: 'var(--color-text-tertiary)' }}>{t('settings.deep_clean_note')}</span>
                <Button size="small" onClick={handleDeepClean} loading={deepCleaning}>
                  {t('settings.deep_clean_btn')}
                </Button>
              </div>

              <div>
                <Button size="small" icon={<ReloadOutlined />} onClick={loadReport} loading={reportLoading}>
                  {t('common.refresh')}
                </Button>
              </div>
            </>
          ) : (
            <Button size="small" onClick={loadReport} loading={reportLoading}>
              {t('settings.scan_storage')}
            </Button>
          )}
        </div>
      </div>

      {/* About Card */}
      <div className="ant-card" style={{ padding: 0, marginTop: 20 }}>
        <div
          style={{
            padding: '12px 20px',
            borderBottom: '1px solid var(--color-border-subtle)',
            display: 'flex',
            alignItems: 'center',
            gap: 8,
            fontSize: 13,
            fontWeight: 600,
          }}
        >
          <InfoCircleOutlined />
          {t('about.title')}
        </div>
        <div style={{ padding: 20, display: 'flex', flexDirection: 'column', gap: 14 }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', gap: 12 }}>
            <span style={{ fontSize: 13, color: 'var(--color-text-secondary)' }}>
              {t('settings.version')}
              {currentVersion ? ` · v${currentVersion}` : ''}
            </span>
            <Button size="small" icon={<SyncOutlined />} loading={checkingUpdate} onClick={handleCheckUpdate}>
              {t('update.check')}
            </Button>
          </div>

          <Space size={16} wrap>
            <Button
              type="link"
              size="small"
              style={{ padding: 0, height: 'auto', fontSize: 12 }}
              icon={<ExportOutlined />}
              onClick={() => void updateApi.open(lang === 'zh' ? GUIDE_URLS.zh : GUIDE_URLS.en)}
            >
              {t('about.guide')}
            </Button>
            <Button type="link" size="small" style={{ padding: 0, height: 'auto', fontSize: 12 }} icon={<ExportOutlined />} onClick={() => void updateApi.open(RELEASES_URL)}>
              {t('about.releases')}
            </Button>
          </Space>
        </div>
      </div>

      {/* 数据目录迁移结果：旧数据已清理，必须重启才能继续使用 */}
      <Modal
        open={dataMigrateResult !== null}
        title={t('datadir.migrate_done')}
        closable={false}
        maskClosable={false}
        keyboard={false}
        footer={[
          <Button key="restart" type="primary" icon={<ReloadOutlined />} onClick={() => void envApi.restart()}>
            {t('datadir.restart')}
          </Button>,
        ]}
      >
        <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
          <span style={{ fontSize: 13 }}>{dataMigrateResult}</span>
          <span style={{ fontSize: 12, color: 'var(--color-text-tertiary)' }}>{t('datadir.migrate_restart_hint')}</span>
        </div>
      </Modal>

      {/* Migration Preview Modal */}
      <Modal
        title={t('settings.migrate_preview_title')}
        open={previewOpen}
        onCancel={() => !migrating && setPreviewOpen(false)}
        maskClosable={!migrating}
        closable={!migrating}
        width={640}
        footer={[
          <Button key="cancel" onClick={() => setPreviewOpen(false)} disabled={migrating}>
            {t('common.cancel')}
          </Button>,
          <Button key="ok" type="primary" danger loading={migrating} disabled={previewBlocked || !preview} onClick={confirmMigrate}>
            {t('settings.migrate_start')}
          </Button>,
        ]}
      >
        {preview && (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
            {preview.blockers.length > 0 && <Alert type="error" showIcon message={preview.blockers.join(t('settings.list_sep'))} />}
            <div style={{ fontSize: 13, color: 'var(--color-text-secondary)' }}>{t('settings.migrate_preview_summary', { n: preview.items.length, size: formatBytes(preview.total_bytes) })}</div>
            <div
              style={{
                maxHeight: 320,
                overflow: 'auto',
                border: '1px solid var(--color-border-subtle)',
                borderRadius: 'var(--radius-lg)',
              }}
            >
              {preview.items.length === 0 ? (
                <div style={{ padding: 16, fontSize: 13, color: 'var(--color-text-tertiary)' }}>{t('settings.migrate_preview_empty')}</div>
              ) : (
                preview.items.map((item) => (
                  <div
                    key={`${item.from}->${item.to}`}
                    style={{
                      display: 'flex',
                      justifyContent: 'space-between',
                      gap: 12,
                      padding: '8px 12px',
                      borderBottom: '1px solid var(--color-border-subtle)',
                      fontSize: 12,
                    }}
                  >
                    <div style={{ minWidth: 0, wordBreak: 'break-all' }}>
                      <code>{item.from}</code>
                      <ArrowRightOutlined style={{ margin: '0 6px', color: 'var(--color-text-tertiary)' }} />
                      <code>{item.to}</code>
                    </div>
                    <span style={{ whiteSpace: 'nowrap', color: 'var(--color-text-tertiary)' }}>{formatBytes(item.bytes)}</span>
                  </div>
                ))
              )}
            </div>
          </div>
        )}
      </Modal>
    </div>
  )
}
