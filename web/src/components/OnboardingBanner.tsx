import { useCallback, useEffect, useState } from 'react'
import { Button, Space } from 'antd'
import { useLocation, useNavigate } from 'react-router-dom'
import { listen } from '@tauri-apps/api/event'
import { CheckCircleFilled, CloseOutlined, CloudDownloadOutlined, LoadingOutlined, RightOutlined, ThunderboltOutlined } from '@ant-design/icons'
import { envApi } from '@/api/env'
import { inferApi } from '@/api/infer'
import type { BaseModelStatus } from '@/api/infer'
import { coverApi } from '@/api/cover'
import { updateApi } from '@/api/update'
import { GUIDE_URLS } from '@/lib/links'
import { useI18n } from '@/i18n'

/** 「不再提示」的持久化标记。 */
const DISMISS_KEY = 'voxril-onboarding-dismissed'

/** 引导只关心核心依赖，其余 MDX 人声模型属于可选项。 */
const REQUIRED_BASE = ['hubert', 'rmvpe', 'bs_roformer', 'f0G40k', 'f0D40k']

function StepRow({ status, title, desc, action }: { status: 'done' | 'pending' | 'todo'; title: string; desc?: string; action?: React.ReactNode }) {
  const icon =
    status === 'done' ? (
      <CheckCircleFilled style={{ color: 'var(--color-success)' }} />
    ) : status === 'pending' ? (
      <LoadingOutlined style={{ color: 'var(--color-primary-hover)' }} />
    ) : (
      <RightOutlined style={{ color: 'var(--color-text-tertiary)', fontSize: 11 }} />
    )

  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
      <span style={{ fontSize: 14, width: 16, display: 'inline-flex', justifyContent: 'center' }}>{icon}</span>
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ fontSize: 13, fontWeight: 500, color: status === 'done' ? 'var(--color-text-secondary)' : 'var(--color-text-primary)' }}>{title}</div>
        {desc && <div style={{ fontSize: 11.5, color: 'var(--color-text-tertiary)', marginTop: 2 }}>{desc}</div>}
      </div>
      {action}
    </div>
  )
}

/**
 * 首次启动引导：运行环境 → 基础模型 → 声音模型。
 *
 * 只在首页（一键翻唱）显示，三步都完成后自动消失；用户也可以「不再提示」。
 * 环境状态在后端只是文件检查，准备期间每 5 秒轮询一次，就绪后立即停止。
 */
export default function OnboardingBanner() {
  const { t, lang } = useI18n()
  const navigate = useNavigate()
  const location = useLocation()
  const isHome = location.pathname === '/' || location.pathname === '/cover'

  const [hidden, setHidden] = useState(() => localStorage.getItem(DISMISS_KEY) === '1')
  const [envReady, setEnvReady] = useState<boolean | null>(null)
  const [setupLog, setSetupLog] = useState('')
  const [baseModels, setBaseModels] = useState<BaseModelStatus[] | null>(null)
  const [voiceCount, setVoiceCount] = useState<number | null>(null)

  const loadEnv = useCallback(async () => {
    try {
      const status = await envApi.getStatus()
      setEnvReady(status.venv_ready)
    } catch {
      /* 状态读取失败时保持上一次结果 */
    }
  }, [])

  const loadModels = useCallback(async () => {
    const [base, voices] = await Promise.all([inferApi.getBaseModelsStatus().catch(() => null), coverApi.listVoices().catch(() => null)])
    if (base) setBaseModels(base)
    if (voices) setVoiceCount(voices.length)
  }, [])

  useEffect(() => {
    if (hidden || !isHome) return
    // 放到下一个 tick，避免在 effect 同步阶段触发状态更新
    const initial = setTimeout(() => {
      void loadEnv()
      void loadModels()
    }, 0)

    let unlistenSetup: (() => void) | undefined
    listen<string>('setup-log', (event) => setSetupLog(event.payload.replace(/^\[SETUP\]\s*/, '')))
      .then((fn) => {
        unlistenSetup = fn
      })
      .catch(() => {})
    // 启动自检下载结束后刷新模型状态
    const unlistenDone = listen('model-download-done', () => {
      void loadModels()
      void loadEnv()
    })

    return () => {
      clearTimeout(initial)
      unlistenSetup?.()
      unlistenDone.then((fn) => fn()).catch(() => {})
    }
  }, [hidden, isHome, loadEnv, loadModels])

  // 环境准备中才轮询，就绪后自动停下
  useEffect(() => {
    if (hidden || !isHome || envReady !== false) return
    const timer = setInterval(() => {
      void loadEnv()
    }, 5000)
    return () => clearInterval(timer)
  }, [hidden, isHome, envReady, loadEnv])

  if (hidden || !isHome) return null

  const missingBase = baseModels ? baseModels.filter((m) => REQUIRED_BASE.includes(m.id) && !m.exists).length : null
  const allDone = envReady === true && missingBase === 0 && (voiceCount ?? 0) > 0
  if (allDone) return null

  const dismiss = () => {
    localStorage.setItem(DISMISS_KEY, '1')
    setHidden(true)
  }

  return (
    <div
      style={{
        marginBottom: 20,
        padding: '14px 18px',
        background: 'var(--color-bg-elevated)',
        border: '1px solid var(--color-border-subtle)',
        borderRadius: 'var(--radius-lg)',
      }}
    >
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 12 }}>
        <div style={{ fontSize: 13, fontWeight: 600 }}>{t('onboarding.title')}</div>
        <Space size={12}>
          <Button type="link" size="small" style={{ padding: 0, height: 'auto', fontSize: 12 }} onClick={() => void updateApi.open(lang === 'zh' ? GUIDE_URLS.zh : GUIDE_URLS.en)}>
            {t('onboarding.guide')}
          </Button>
          <Button type="text" size="small" icon={<CloseOutlined />} onClick={dismiss} aria-label={t('onboarding.dismiss')}>
            {t('onboarding.dismiss')}
          </Button>
        </Space>
      </div>

      <Space orientation="vertical" size={10} style={{ width: '100%' }}>
        <StepRow
          status={envReady ? 'done' : 'pending'}
          title={envReady ? t('onboarding.env_ready') : t('onboarding.env_pending')}
          desc={envReady ? undefined : setupLog || t('onboarding.env_pending_desc')}
        />

        <StepRow
          status={missingBase === 0 ? 'done' : missingBase === null ? 'pending' : 'todo'}
          title={missingBase === 0 ? t('onboarding.models_ready') : missingBase === null ? t('onboarding.models_loading') : t('onboarding.models_missing').replace('{n}', String(missingBase))}
          desc={missingBase === 0 || missingBase === null ? undefined : t('onboarding.models_missing_desc')}
          action={
            missingBase === null || missingBase === 0 ? undefined : (
              <Button size="small" icon={<CloudDownloadOutlined />} onClick={() => navigate('/models')}>
                {t('onboarding.go_models')}
              </Button>
            )
          }
        />

        <StepRow
          status={voiceCount === null ? 'pending' : voiceCount > 0 ? 'done' : 'todo'}
          title={voiceCount === null ? t('onboarding.voice_loading') : voiceCount > 0 ? t('onboarding.voice_ready').replace('{n}', String(voiceCount)) : t('onboarding.voice_none')}
          desc={voiceCount === null || voiceCount > 0 ? undefined : t('onboarding.voice_none_desc')}
          action={
            voiceCount === null || voiceCount > 0 ? undefined : (
              <Button size="small" type="primary" ghost icon={<ThunderboltOutlined />} onClick={() => navigate('/train')}>
                {t('onboarding.go_train')}
              </Button>
            )
          }
        />
      </Space>
    </div>
  )
}
