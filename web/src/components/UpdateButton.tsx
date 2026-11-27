import { useEffect } from 'react'
import { Badge, Button, Popover, Progress, Space, Typography } from 'antd'
import { CheckCircleFilled, CheckCircleOutlined, CloudDownloadOutlined, ExclamationCircleOutlined, LoadingOutlined } from '@ant-design/icons'
import { useUpdaterStore } from '@/store/updaterStore'
import { useI18n } from '@/i18n'

const { Text, Paragraph } = Typography

function formatSize(bytes: number): string {
  if (!bytes) return '—'
  const mb = bytes / 1024 ** 2
  return mb >= 1024 ? `${(mb / 1024).toFixed(1)} GB` : `${mb.toFixed(1)} MB`
}

/**
 * 标题栏更新入口：发现新版本时图标高亮并带红点，
 * 点击展开面板完成「下载 → 提示安装 → 重启」。
 */
export default function UpdateButton() {
  const { t } = useI18n()
  const { phase, currentVersion, version, notes, error, downloaded, total, panelOpen, dismissed, bootstrap, check, download, install, openPanel, closePanel, ignore } = useUpdaterStore()

  // 启动后自动检查一次（StrictMode 下内部已去重）
  useEffect(() => {
    bootstrap()
  }, [bootstrap])

  const hasUpdate = phase === 'available' || phase === 'downloading' || phase === 'ready'
  const percent = total > 0 ? Math.min(100, Math.floor((downloaded / total) * 100)) : 0

  const icon = (() => {
    if (phase === 'checking' || phase === 'downloading') {
      return <LoadingOutlined spin style={{ fontSize: 15 }} />
    }
    if (phase === 'ready') return <CheckCircleFilled style={{ fontSize: 15, color: 'var(--color-success)' }} />
    if (phase === 'uptodate') return <CheckCircleOutlined style={{ fontSize: 15 }} />
    if (phase === 'error') return <ExclamationCircleOutlined style={{ fontSize: 15 }} />
    return <CloudDownloadOutlined style={{ fontSize: 15 }} />
  })()

  // 面板开合交给受控的 Popover；这里只负责首次点开时顺手检查一次
  const handleClick = () => {
    if (!panelOpen && (phase === 'idle' || phase === 'error')) void check(true)
  }

  const content = (
    <div style={{ width: 320 }}>
      {phase === 'checking' && (
        <Space size={8}>
          <LoadingOutlined spin />
          <Text style={{ fontSize: 13 }}>{t('update.check')}…</Text>
        </Space>
      )}

      {phase === 'uptodate' && (
        <Space orientation="vertical" size={6} style={{ width: '100%' }}>
          <Space size={8}>
            <CheckCircleOutlined style={{ color: 'var(--color-success)' }} />
            <Text style={{ fontSize: 13 }}>{t('update.up_to_date')}</Text>
          </Space>
          {currentVersion ? (
            <Text type="secondary" style={{ fontSize: 11 }}>
              {t('settings.version')} v{currentVersion}
            </Text>
          ) : null}
          <Button size="small" onClick={() => void check(true)}>
            {t('update.check')}
          </Button>
        </Space>
      )}

      {phase === 'available' && (
        <Space orientation="vertical" size={10} style={{ width: '100%' }}>
          <Text strong style={{ fontSize: 13 }}>
            {t('update.dialog_title').replace('{version}', version)}
          </Text>
          {notes ? (
            <Paragraph type="secondary" style={{ fontSize: 12, margin: 0, maxHeight: 120, overflow: 'auto', whiteSpace: 'pre-wrap' }} ellipsis={{ rows: 6 }}>
              {notes}
            </Paragraph>
          ) : null}
          <Space size={8}>
            <Button size="small" type="primary" icon={<CloudDownloadOutlined />} onClick={() => void download()}>
              {t('update.download')}
            </Button>
            <Button size="small" type="text" onClick={ignore}>
              {t('update.ignore')}
            </Button>
          </Space>
          <Text type="secondary" style={{ fontSize: 11 }}>
            {t('update.dialog_desc')}
          </Text>
        </Space>
      )}

      {phase === 'downloading' && (
        <Space orientation="vertical" size={8} style={{ width: '100%' }}>
          <Text style={{ fontSize: 13 }}>{t('update.downloading').replace('{version}', version)}</Text>
          <Progress percent={percent} size="small" status="active" />
          <Text type="secondary" style={{ fontSize: 11 }}>
            {formatSize(downloaded)} / {formatSize(total)}
          </Text>
        </Space>
      )}

      {phase === 'ready' && (
        <Space orientation="vertical" size={10} style={{ width: '100%' }}>
          <Space size={8}>
            <CheckCircleFilled style={{ color: 'var(--color-success)' }} />
            <Text strong style={{ fontSize: 13 }}>
              {t('update.ready_title').replace('{version}', version)}
            </Text>
          </Space>
          <Text type="secondary" style={{ fontSize: 12 }}>
            {t('update.ready_desc')}
          </Text>
          <Button size="small" type="primary" icon={<CheckCircleOutlined />} onClick={() => void install()}>
            {t('update.install_now')}
          </Button>
        </Space>
      )}

      {phase === 'error' && (
        <Space orientation="vertical" size={10} style={{ width: '100%' }}>
          <Text style={{ fontSize: 13 }} type="danger">
            {t('update.failed')}
          </Text>
          {error ? (
            <Text type="secondary" style={{ fontSize: 11, wordBreak: 'break-all' }}>
              {error}
            </Text>
          ) : null}
          <Button size="small" onClick={() => void check(true)}>
            {t('update.retry')}
          </Button>
        </Space>
      )}

      {phase === 'idle' && (
        <Space orientation="vertical" size={8} style={{ width: '100%' }}>
          <Text style={{ fontSize: 13 }}>{t('update.idle_hint')}</Text>
          <Button size="small" onClick={() => void check(true)}>
            {t('update.check')}
          </Button>
        </Space>
      )}
    </div>
  )

  return (
    <Popover open={panelOpen} onOpenChange={(open) => (open ? openPanel() : closePanel())} content={content} trigger="click" placement="bottomRight" arrow={false}>
      <Badge dot={hasUpdate && !dismissed} offset={[-2, 2]}>
        <Button
          type="text"
          size="small"
          aria-label={t('update.check')}
          title={t('update.check')}
          onClick={handleClick}
          style={{
            height: 30,
            width: 30,
            color: hasUpdate ? 'var(--color-primary-hover)' : 'var(--color-text-secondary)',
          }}
        >
          {icon}
        </Button>
      </Badge>
    </Popover>
  )
}
