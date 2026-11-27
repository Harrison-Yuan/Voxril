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
const SOURCE_OPTIONS: { value: DownloadSource; zh: string; en: string }[] = [
  { value: 'auto', zh: '自动（推荐）', en: 'Auto (recommended)' },
  { value: 'official', zh: '官方源', en: 'Official only' },
  { value: 'cn', zh: '国内镜像', en: 'China mirror' },
  { value: 'custom', zh: '自定义', en: 'Custom' },
]

/** pip 源选项。 */
const PIP_SOURCE_OPTIONS: { value: PipSource; zh: string; en: string }[] = [
  { value: 'auto', zh: '自动（推荐）', en: 'Auto (recommended)' },
  { value: 'official', zh: '官方 PyPI', en: 'Official PyPI' },
  { value: 'tuna', zh: '清华镜像', en: 'Tsinghua mirror' },
  { value: 'aliyun', zh: '阿里云镜像', en: 'Aliyun mirror' },
  { value: 'custom', zh: '自定义', en: 'Custom' },
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
      message.error(`统计存储占用失败：${e?.message || e}`)
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
      message.error(`保存下载设置失败：${e?.message || e}`)
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
          message.error(`${t('datadir.migrate_failed')}：${e instanceof Error ? e.message : String(e)}`)
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
      message.error(`生成迁移预览失败：${e?.message || e}`)
    } finally {
      setPreviewing(false)
    }
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
    } catch (e: any) {
      message.error(`升级失败：${e?.message || e}`)
    } finally {
      setMigrating(false)
    }
  }

  const handleReclaim = () => {
    const bytes = report?.reclaimable_now ?? 0
    Modal.confirm({
      title: lang === 'zh' ? '确认释放空间' : 'Reclaim space',
      content:
        lang === 'zh'
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
        } catch (e: any) {
          message.error(`释放失败：${e?.message || e}`)
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
      message.error(`清理失败：${e?.message || e}`)
    } finally {
      setDeepCleaning(false)
    }
  }

  const previewBlocked = (preview?.blockers.length ?? 0) > 0

  const sourceHint = (() => {
    switch (download.source) {
      case 'official':
        return lang === 'zh' ? '始终使用 huggingface.co 与 github.com 直连，适合海外网络。' : 'Always use huggingface.co and github.com directly; best for networks outside China.'
      case 'cn':
        return lang === 'zh' ? '优先使用 hf-mirror.com 与 GitHub 加速前缀，适合国内网络；失败后仍会回退官方源。' : 'Prefer hf-mirror.com and a GitHub proxy; falls back to official sources on failure.'
      case 'custom':
        return lang === 'zh' ? '先使用你填写的端点，失败后回退官方源。' : 'Use your endpoints first, then fall back to official sources.'
      default:
        return lang === 'zh'
          ? '自动探测官方源，不可达时切换到国内镜像；下载中断后再次点击会从断点继续。'
          : 'Probes the official source first, falls back to a mirror, and resumes interrupted downloads.'
    }
  })()

  const pipHint = (() => {
    switch (download.pip_source) {
      case 'official':
        return lang === 'zh' ? '始终使用官方 PyPI，适合海外网络。' : 'Always use the official PyPI; best for networks outside China.'
      case 'tuna':
        return lang === 'zh' ? '优先清华 PyPI 镜像，失败后回退官方源。' : 'Prefer the Tsinghua PyPI mirror, then fall back to official PyPI.'
      case 'aliyun':
        return lang === 'zh' ? '优先阿里云 PyPI 镜像，失败后回退官方源。' : 'Prefer the Aliyun PyPI mirror, then fall back to official PyPI.'
      case 'custom':
        return lang === 'zh' ? '先使用你填写的索引地址，失败后回退官方源。' : 'Use your index URL first, then fall back to official PyPI.'
      default:
        return lang === 'zh' ? '自动探测官方 PyPI，不可达时切换清华镜像；首次安装或补齐缺失依赖时生效。' : 'Probes the official PyPI and falls back to the Tsinghua mirror; applies on next install.'
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
          <p style={{ fontSize: 12, color: 'var(--color-text-tertiary)', margin: '8px 0 0' }}>
            {lang === 'zh' ? '切换后立即生效，下次启动自动记住。' : 'Takes effect immediately. Your preference will be remembered on next launch.'}
          </p>
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
          {lang === 'zh' ? '模型与依赖下载源' : 'Model & dependency sources'}
        </div>
        <div style={{ padding: 20, display: 'flex', flexDirection: 'column', gap: 14 }}>
          <p style={{ fontSize: 13, color: 'var(--color-text-secondary)', margin: 0 }}>
            {lang === 'zh'
              ? '基础模型支持多源下载、断点续传与 SHA256 校验；Python 依赖支持 PyPI 镜像。失败都会自动切换下一个源。'
              : 'Base models support multiple sources, resume and SHA256 checks; Python packages support PyPI mirrors. Failures fall back automatically.'}
          </p>

          <Select
            value={download.source}
            onChange={(v: DownloadSource) => setDownload((d) => ({ ...d, source: v }))}
            style={{ width: 240 }}
            options={SOURCE_OPTIONS.map((o) => ({ value: o.value, label: lang === 'zh' ? o.zh : o.en }))}
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
                  {lang === 'zh' ? 'HuggingFace 端点' : 'HuggingFace endpoint'}
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
                  {lang === 'zh' ? 'GitHub 加速前缀（可选）' : 'GitHub proxy prefix (optional)'}
                </div>
                <Input value={download.gh_proxy} onChange={(e) => setDownload((d) => ({ ...d, gh_proxy: e.target.value }))} placeholder="https://ghfast.top" />
              </div>
            </>
          )}

          <div style={{ height: 1, background: 'var(--color-border-subtle)', margin: '4px 0' }} />

          <div>
            <div style={{ fontSize: 11, fontWeight: 600, color: 'var(--color-text-tertiary)', textTransform: 'uppercase', letterSpacing: '0.06em', marginBottom: 8 }}>
              {lang === 'zh' ? 'Python 依赖源' : 'Python package source'}
            </div>
            <Select
              value={download.pip_source}
              onChange={(v: PipSource) => setDownload((d) => ({ ...d, pip_source: v }))}
              style={{ width: 240 }}
              options={PIP_SOURCE_OPTIONS.map((o) => ({ value: o.value, label: lang === 'zh' ? o.zh : o.en }))}
            />
          </div>
          <p style={{ fontSize: 12, color: 'var(--color-text-tertiary)', margin: 0 }}>{pipHint}</p>

          {download.pip_source === 'custom' && (
            <div>
              <div style={{ fontSize: 11, fontWeight: 600, color: 'var(--color-text-tertiary)', textTransform: 'uppercase', letterSpacing: '0.06em', marginBottom: 8 }}>
                {lang === 'zh' ? 'PyPI 索引地址' : 'PyPI index URL'}
              </div>
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
          <p style={{ fontSize: 13, color: 'var(--color-text-secondary)', margin: 0 }}>
            {lang === 'zh' ? '所有训练数据、模型文件和索引文件都存储在此目录下' : 'All training data, model files, and index files are stored in this directory.'}
          </p>

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

          <Alert message={lang === 'zh' ? '修改后需要重启应用才能生效' : 'Changes require a restart to take effect'} type="warning" showIcon style={{ fontSize: 13 }} />
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
                {lang === 'zh' ? '目录已整理为「进 / 算 / 出 / 留」四层：inputs、work、outputs、models。' : 'Directories are organized into inputs / work / outputs / models.'}
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
          {lang === 'zh' ? '清理中心' : 'Cleanup Center'}
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
                  <div style={{ fontSize: 12, color: 'var(--color-text-tertiary)' }}>{lang === 'zh' ? '可立即释放' : 'Reclaimable now'}</div>
                  <div style={{ fontSize: 22, fontWeight: 600 }}>{formatBytes(report.reclaimable_now)}</div>
                </div>
                <Button type="primary" icon={<ClearOutlined />} onClick={handleReclaim} loading={reclaiming} disabled={report.reclaimable_now <= 0}>
                  {lang === 'zh' ? '释放空间' : 'Reclaim'}
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
          <Button key="ok" type="primary" danger loading={migrating} disabled={previewBlocked || !preview} onClick={confirmMigrate}>
            {lang === 'zh' ? '开始升级' : 'Upgrade'}
          </Button>,
        ]}
      >
        {preview && (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
            {preview.blockers.length > 0 && <Alert type="error" showIcon message={preview.blockers.join('；')} />}
            <div style={{ fontSize: 13, color: 'var(--color-text-secondary)' }}>
              {lang === 'zh'
                ? `共 ${preview.items.length} 项，合计 ${formatBytes(preview.total_bytes)}。迁移为同卷重命名，瞬时完成；任一项失败会整体回滚。`
                : `${preview.items.length} items, ${formatBytes(preview.total_bytes)} total. Moves are same-volume renames and roll back on any failure.`}
            </div>
            <div
              style={{
                maxHeight: 320,
                overflow: 'auto',
                border: '1px solid var(--color-border-subtle)',
                borderRadius: 'var(--radius-lg)',
              }}
            >
              {preview.items.length === 0 ? (
                <div style={{ padding: 16, fontSize: 13, color: 'var(--color-text-tertiary)' }}>
                  {lang === 'zh' ? '没有需要移动的内容，升级只会建立新目录结构。' : 'Nothing to move; the upgrade only creates the new directory structure.'}
                </div>
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
