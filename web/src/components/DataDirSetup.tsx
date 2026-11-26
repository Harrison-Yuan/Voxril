import { useEffect, useState } from 'react'
import { Alert, Button, Modal, Space, Typography, message } from 'antd'
import { FolderOpenOutlined, ReloadOutlined, RocketOutlined } from '@ant-design/icons'
import { envApi } from '@/api/env'
import type { DataDirStatus } from '@/api/env'
import { useI18n } from '@/i18n'

const { Text } = Typography

function formatGB(bytes: number | null): string {
  if (bytes === null || !Number.isFinite(bytes)) return '—'
  const gb = bytes / 1024 ** 3
  return gb >= 10 ? `${gb.toFixed(0)} GB` : `${gb.toFixed(1)} GB`
}

/** 系统盘判断：Windows 上的 C:，用于提示别把数 GB 数据塞进系统盘 */
function isSystemDrive(path: string): boolean {
  return /^[cC]:([\\/]|$)/.test(path)
}

/**
 * 首启数据目录向导：全新安装时在下载任何东西之前，
 * 让用户确认（或改选）Python 环境与基础模型的存放位置。
 *
 * 沿用默认目录则直接放行后端安装，无需重启；改选别处才需要重启一次，
 * 因为进程内的数据目录与 Python 执行器已经按旧路径创建。
 */
export default function DataDirSetup() {
  const { t } = useI18n()
  const [status, setStatus] = useState<DataDirStatus | null>(null)
  const [picked, setPicked] = useState<{ path: string; free: number | null } | null>(null)
  const [busy, setBusy] = useState(false)
  const [needRestart, setNeedRestart] = useState(false)
  const [done, setDone] = useState(false)

  useEffect(() => {
    envApi
      .getDataDirStatus()
      .then(setStatus)
      .catch(() => {})
  }, [])

  // 不可写的情况由 DataDirGuard 处理；已有数据或已确认过的老用户不打扰
  if (done || !status || !status.writable || status.confirmed || status.has_data) return null

  const path = picked?.path ?? status.path
  const free = picked?.free ?? status.free_bytes
  const recommended = status.recommended_bytes
  const lowSpace = free !== null && free < recommended

  const pickDir = async () => {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog')
      const dir = await open({
        directory: true,
        multiple: false,
        title: t('datadir.pick_title'),
        defaultPath: path,
      })
      if (!dir || typeof dir !== 'string') return
      setPicked({ path: dir, free: await envApi.getDirFreeSpace(dir) })
    } catch (e: unknown) {
      const detail = e instanceof Error ? e.message : String(e)
      message.error(`${t('datadir.save_failed')}: ${detail}`)
    }
  }

  const start = async () => {
    setBusy(true)
    try {
      const restartNeeded = await envApi.confirmDataDir(path)
      if (restartNeeded) {
        setNeedRestart(true)
      } else {
        setDone(true)
      }
    } catch (e: unknown) {
      const detail = e instanceof Error ? e.message : String(e)
      message.error(`${t('datadir.save_failed')}: ${detail}`)
    } finally {
      setBusy(false)
    }
  }

  return (
    <Modal
      open
      centered
      closable={false}
      mask={{ closable: false }}
      keyboard={false}
      title={needRestart ? t('datadir.saved_title') : t('datadir.setup_title')}
      footer={
        needRestart
          ? [
              <Button key="restart" type="primary" icon={<ReloadOutlined />} onClick={() => void envApi.restart()}>
                {t('datadir.restart')}
              </Button>,
            ]
          : [
              <Button key="pick" icon={<FolderOpenOutlined />} onClick={() => void pickDir()}>
                {t('datadir.change_to')}
              </Button>,
              <Button key="start" type="primary" icon={<RocketOutlined />} loading={busy} onClick={() => void start()}>
                {t('datadir.start')}
              </Button>,
            ]
      }
    >
      <Space orientation="vertical" size={12} style={{ width: '100%' }}>
        <Text>{needRestart ? t('datadir.saved_desc') : t('datadir.setup_desc')}</Text>
        <Text code style={{ wordBreak: 'break-all', fontSize: 12 }}>
          {path}
        </Text>
        {!needRestart && (
          <>
            <Text type="secondary" style={{ fontSize: 12 }}>
              {t('datadir.setup_free').replace('{n}', formatGB(free))}
              {' · '}
              {t('datadir.setup_need').replace('{n}', formatGB(recommended))}
            </Text>
            {lowSpace ? <Alert type="warning" showIcon title={t('datadir.setup_low_space')} description={t('datadir.setup_low_space_desc')} /> : null}
            {isSystemDrive(path) ? <Alert type="info" showIcon title={t('datadir.setup_system')} description={t('datadir.warn_c')} /> : null}
          </>
        )}
      </Space>
    </Modal>
  )
}
