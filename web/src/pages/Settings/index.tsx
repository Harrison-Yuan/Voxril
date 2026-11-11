import { useState, useEffect } from 'react'
import { Input, Button, message, Alert, Select } from 'antd'
import { FolderOpenOutlined, SaveOutlined, ReloadOutlined, SettingOutlined } from '@ant-design/icons'
import { workspaceApi } from '@/api/database'
import { useI18n, type Lang } from '../../i18n'

export default function SettingsPage() {
  const { t, lang, setLang } = useI18n()
  const [workspace, setWorkspace] = useState('')
  const [loading, setLoading] = useState(false)
  const [fetching, setFetching] = useState(true)

  useEffect(() => { loadWorkspace() }, [])

  const loadWorkspace = async () => {
    setFetching(true)
    try {
      setWorkspace(await workspaceApi.get())
    } catch { message.error(t('common.failed')) }
    finally { setFetching(false) }
  }

  const handleSave = async () => {
    if (!workspace.trim()) { message.warning(t('common.no_data')); return }
    setLoading(true)
    try {
      const result = await workspaceApi.set(workspace.trim())
      message.success(t('common.done'))
      setWorkspace(result.workspace)
    } catch (e: any) { message.error(e.message || t('common.failed')) }
    finally { setLoading(false) }
  }

  const handleSelectFolder = async () => {
    try {
      const path = await workspaceApi.select()
      if (path) { setWorkspace(path); message.success(t('common.done')) }
    } catch (e: any) { message.error(e.message || t('common.failed')) }
  }

  return (
    <div style={{ maxWidth: 640, margin: '0 auto' }}>
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
      <div className="ant-card" style={{ padding: 0 }}>
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
    </div>
  )
}
