import { useEffect, useState } from 'react'
import { Alert, Button, Space } from 'antd'
import { RocketOutlined } from '@ant-design/icons'
import { updateApi } from '@/api/update'
import type { UpdateInfo } from '@/api/update'
import { useI18n } from '@/i18n'

/** 记录用户「忽略此版本」的选择，避免每次启动都弹同一版本。 */
const IGNORED_KEY = 'voxril-ignored-version'

/**
 * 启动后延迟检查 GitHub Release，有新版本时在页面顶部提示。
 * 检查失败（离线、接口不可达）静默忽略，不打扰用户。
 */
export default function UpdateBanner() {
  const { t } = useI18n()
  const [info, setInfo] = useState<UpdateInfo | null>(null)
  const [opening, setOpening] = useState(false)

  useEffect(() => {
    let cancelled = false
    const timer = setTimeout(async () => {
      try {
        const result = await updateApi.check()
        if (cancelled || !result.has_update) return
        if (localStorage.getItem(IGNORED_KEY) === result.latest) return
        setInfo(result)
      } catch {
        /* 无网络或 GitHub API 不可达时静默跳过 */
      }
    }, 3000)
    return () => {
      cancelled = true
      clearTimeout(timer)
    }
  }, [])

  if (!info) return null

  const handleOpen = async () => {
    setOpening(true)
    try {
      await updateApi.open(info.url)
    } finally {
      setOpening(false)
    }
  }

  const handleIgnore = () => {
    localStorage.setItem(IGNORED_KEY, info.latest)
    setInfo(null)
  }

  return (
    <Alert
      type="info"
      showIcon
      icon={<RocketOutlined />}
      closable
      onClose={() => setInfo(null)}
      style={{ marginBottom: 16, borderRadius: 'var(--radius-lg)' }}
      message={t('update.available').replace('{latest}', info.latest).replace('{current}', info.current)}
      action={
        <Space size={4}>
          <Button size="small" type="primary" loading={opening} onClick={handleOpen}>
            {t('update.download')}
          </Button>
          <Button size="small" type="text" onClick={handleIgnore}>
            {t('update.ignore')}
          </Button>
        </Space>
      }
    />
  )
}
