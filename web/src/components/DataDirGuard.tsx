import { useEffect, useState } from 'react'
import { Button, Modal, Space, Typography, message } from 'antd'
import { ExclamationCircleOutlined, FolderOpenOutlined, ReloadOutlined } from '@ant-design/icons'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { envApi } from '@/api/env'
import type { DataDirStatus } from '@/api/env'
import { useI18n } from '@/i18n'

const { Text } = Typography

/**
 * 数据目录守卫：安装目录不可写时（例如装进受保护目录），
 * 不把 Python 环境与模型静默塞进系统盘，而是引导用户另选目录。
 */
export default function DataDirGuard() {
  const { t } = useI18n()
  const [status, setStatus] = useState<DataDirStatus | null>(null)
  const [picking, setPicking] = useState(false)
  const [saved, setSaved] = useState(false)

  useEffect(() => {
    envApi
      .getDataDirStatus()
      .then(setStatus)
      .catch(() => {})
  }, [])

  if (!status || status.writable) return null

  const pickDir = async () => {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog')
      const dir = await open({
        directory: true,
        multiple: false,
        title: t('datadir.pick_title'),
        defaultPath: status.path,
      })
      if (!dir || typeof dir !== 'string') return
      setPicking(true)
      await envApi.setDataDir(dir)
      setStatus({ ...status, path: dir })
      setSaved(true)
    } catch (e: unknown) {
      const detail = e instanceof Error ? e.message : String(e)
      message.error(`${t('datadir.save_failed')}: ${detail}`)
    } finally {
      setPicking(false)
    }
  }

  const exitApp = () => {
    void getCurrentWindow().close()
  }

  return (
    <Modal
      open
      centered
      closable={false}
      mask={{ closable: false }}
      keyboard={false}
      title={
        <Space>
          <ExclamationCircleOutlined style={{ color: 'var(--color-warning)' }} />
          {saved ? t('datadir.saved_title') : t('datadir.title')}
        </Space>
      }
      footer={
        saved
          ? [
              <Button key="exit" onClick={exitApp}>
                {t('datadir.exit')}
              </Button>,
              <Button key="restart" type="primary" icon={<ReloadOutlined />} onClick={() => void envApi.restart()}>
                {t('datadir.restart')}
              </Button>,
            ]
          : [
              <Button key="exit" onClick={exitApp}>
                {t('datadir.exit')}
              </Button>,
              <Button key="pick" type="primary" icon={<FolderOpenOutlined />} loading={picking} onClick={pickDir}>
                {t('datadir.pick')}
              </Button>,
            ]
      }
    >
      <Space orientation="vertical" size={12} style={{ width: '100%' }}>
        <Text>{saved ? t('datadir.saved_desc') : t('datadir.desc')}</Text>
        <Text code style={{ wordBreak: 'break-all', fontSize: 12 }}>
          {status.path}
        </Text>
        {saved ? null : (
          <>
            <Text type="secondary" style={{ fontSize: 12 }}>
              {t('datadir.hint')}
            </Text>
            <Text type="warning" style={{ fontSize: 12 }}>
              {t('datadir.warn_c')}
            </Text>
          </>
        )}
      </Space>
    </Modal>
  )
}
