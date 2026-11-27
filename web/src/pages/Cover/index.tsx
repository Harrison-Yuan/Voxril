import { useEffect, useMemo, useState } from 'react'
import { Alert, Button, Col, Drawer, Empty, Progress, Row, Select, Slider, Spin, Steps, Tag, message } from 'antd'
import { AudioOutlined, CustomerServiceOutlined, PlayCircleOutlined, ReloadOutlined, SettingOutlined, SoundOutlined, StopOutlined, ThunderboltOutlined, UploadOutlined } from '@ant-design/icons'
import { useNavigate } from 'react-router-dom'
import { open } from '@tauri-apps/plugin-dialog'
import { convertFileSrc } from '@tauri-apps/api/core'
import LogViewer from '../../components/LogViewer'
import { OutputLibrary } from '../../components/OutputLibrary'
import { coverApi, DEFAULT_COVER_PARAMS } from '@/api/cover'
import type { CoverParams, CoverVoice } from '@/api/cover'
import { useCoverStore, type CoverStage } from '@/store/coverStore'
import { useI18n } from '@/i18n'

const STAGES: { key: CoverStage; titleKey: string }[] = [
  { key: 'separate', titleKey: 'cover.stage_separate' },
  { key: 'convert', titleKey: 'cover.stage_convert' },
  { key: 'merge', titleKey: 'infer.merge' },
]

const lbl = {
  fontSize: 11,
  fontWeight: 600,
  color: 'var(--color-text-tertiary)',
  textTransform: 'uppercase' as const,
  letterSpacing: '0.06em',
  marginBottom: 8,
}

function SectionHeader({ children }: { children: React.ReactNode }) {
  return <div style={{ padding: '12px 20px', borderBottom: '1px solid var(--color-border-subtle)', fontSize: 13, fontWeight: 600 }}>{children}</div>
}

function formatDate(secs: number): string {
  if (!secs) return ''
  return new Date(secs * 1000).toLocaleDateString()
}

export default function CoverPage() {
  const navigate = useNavigate()
  const { t } = useI18n()
  const running = useCoverStore((s) => s.running)
  const stage = useCoverStore((s) => s.stage)
  const percent = useCoverStore((s) => s.percent)
  const log = useCoverStore((s) => s.log)
  const result = useCoverStore((s) => s.result)
  const error = useCoverStore((s) => s.error)
  const start = useCoverStore((s) => s.start)
  const cancel = useCoverStore((s) => s.cancel)
  const reset = useCoverStore((s) => s.reset)

  const [voices, setVoices] = useState<CoverVoice[]>([])
  const [loadingVoices, setLoadingVoices] = useState(true)
  const [selected, setSelected] = useState('')
  const [params, setParams] = useState<CoverParams>(DEFAULT_COVER_PARAMS)
  const [songPath, setSongPath] = useState('')
  const [settingsOpen, setSettingsOpen] = useState(false)

  useEffect(() => {
    let cancelled = false
    coverApi
      .listVoices()
      .then((list) => {
        if (cancelled) return
        setVoices(list)
        const remembered = localStorage.getItem('voxril-cover-voice')
        const pick = list.find((v) => v.voice === remembered) ?? list[0]
        if (pick) setSelected(pick.voice)
      })
      .catch((e: any) => message.error(t('cover.load_voices_failed', { msg: e?.message || e })))
      .finally(() => {
        if (!cancelled) setLoadingVoices(false)
      })
    return () => {
      cancelled = true
    }
  }, [])

  // 切换声音时带出该声音保存过的默认生成配置
  useEffect(() => {
    if (!selected) return
    let cancelled = false
    coverApi
      .getParams(selected)
      .then((p) => {
        if (!cancelled) setParams(p)
      })
      .catch(() => {})
    return () => {
      cancelled = true
    }
  }, [selected])

  const selectedVoice = useMemo(() => voices.find((v) => v.voice === selected), [voices, selected])
  const songName = songPath.split('/').pop() || songPath

  const selectVoice = (voice: string) => {
    if (running) return
    setSelected(voice)
    localStorage.setItem('voxril-cover-voice', voice)
  }

  const selectSong = async () => {
    try {
      const sel = await open({
        directory: false,
        multiple: false,
        title: t('cover.pick_song'),
        filters: [{ name: 'Audio', extensions: ['mp3', 'wav', 'flac', 'ogg', 'm4a', 'aac'] }],
      })
      if (typeof sel === 'string') setSongPath(sel)
    } catch (e) {
      message.error(t('cover.pick_song_failed', { msg: String(e) }))
    }
  }

  const closeSettings = () => {
    setSettingsOpen(false)
    if (selected) coverApi.saveParams(selected, params).catch(() => {})
  }

  const handleGenerate = async () => {
    if (!selectedVoice) {
      message.warning(t('cover.warn_select_voice'))
      return
    }
    if (!songPath) {
      message.warning(t('cover.warn_select_song'))
      return
    }
    await start({ model_name: selectedVoice.model_name, song_path: songPath, params })
  }

  const stepIndex = stage === 'done' ? STAGES.length : STAGES.findIndex((s) => s.key === stage)

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 20 }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 12, marginBottom: 4 }}>
        <div className="icon-badge icon-badge--md" style={{ borderRadius: 10 }}>
          <CustomerServiceOutlined />
        </div>
        <div>
          <h1 className="page-header__title">{t('cover.title')}</h1>
          <p className="page-header__subtitle">{t('cover.subtitle')}</p>
        </div>
      </div>

      {/* ① 选声音 */}
      <div className="ant-card" style={{ padding: 0 }}>
        <SectionHeader>
          <span style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <SoundOutlined style={{ color: 'var(--color-primary-hover)' }} />① {t('cover.pick_voice')}
            <span style={{ marginLeft: 'auto', fontSize: 11, fontWeight: 400, color: 'var(--color-text-tertiary)' }}>{t('cover.pick_voice_hint')}</span>
          </span>
        </SectionHeader>
        <div style={{ padding: 20 }}>
          {loadingVoices ? (
            <Spin size="small" />
          ) : voices.length === 0 ? (
            <Empty description={t('cover.no_voices')}>
              <Button type="primary" onClick={() => navigate('/train')}>
                {t('cover.go_train')}
              </Button>
            </Empty>
          ) : (
            <Row gutter={[12, 12]}>
              {voices.map((v) => {
                const active = v.voice === selected
                return (
                  <Col key={v.voice} xs={24} sm={12} lg={8}>
                    <div
                      role="button"
                      onClick={() => selectVoice(v.voice)}
                      style={{
                        border: `1px solid ${active ? 'var(--color-primary-border)' : 'var(--color-border-subtle)'}`,
                        background: active ? 'var(--color-primary-subtle)' : 'transparent',
                        borderRadius: 'var(--radius-lg)',
                        padding: '12px 14px',
                        cursor: running ? 'not-allowed' : 'pointer',
                        transition: 'var(--transition-fast)',
                        display: 'flex',
                        flexDirection: 'column',
                        gap: 8,
                        height: '100%',
                      }}
                    >
                      <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                        <AudioOutlined style={{ color: active ? 'var(--color-primary-hover)' : 'var(--color-text-tertiary)' }} />
                        <span style={{ fontSize: 14, fontWeight: 600, flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{v.voice}</span>
                        {active && (
                          <Tag color="purple" style={{ marginInlineEnd: 0, fontSize: 10 }}>
                            {t('cover.selected')}
                          </Tag>
                        )}
                      </div>
                      <div style={{ fontSize: 11, color: 'var(--color-text-tertiary)' }}>{t('cover.updated_at', { date: formatDate(v.updated_at) || t('cover.date_unknown') })}</div>
                      <Button
                        size="small"
                        type="link"
                        icon={<SettingOutlined />}
                        style={{ padding: 0, alignSelf: 'flex-start', height: 20 }}
                        onClick={(e) => {
                          e.stopPropagation()
                          navigate(`/train?voice=${encodeURIComponent(v.voice)}`)
                        }}
                      >
                        {t('cover.detail')}
                      </Button>
                    </div>
                  </Col>
                )
              })}
            </Row>
          )}
        </div>
      </div>

      {/* ② 选歌 */}
      <div className="ant-card" style={{ padding: 0 }}>
        <SectionHeader>
          <span style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <PlayCircleOutlined style={{ color: 'var(--color-primary-hover)' }} />② {t('cover.pick_song')}
          </span>
        </SectionHeader>
        <div style={{ padding: 20, display: 'flex', flexDirection: 'column', gap: 12 }}>
          <Row gutter={12} align="middle">
            <Col flex="auto">
              <Button icon={<UploadOutlined />} onClick={selectSong} disabled={running} block style={{ height: 44 }}>
                {songPath ? t('cover.change_song') : t('cover.pick_song')}
              </Button>
            </Col>
            <Col>
              <Button icon={<SettingOutlined />} onClick={() => setSettingsOpen(true)} disabled={running} style={{ height: 44 }}>
                {t('cover.settings')}
              </Button>
            </Col>
          </Row>
          {songPath && <Alert type="info" showIcon message={songName} style={{ fontSize: 12 }} />}
          <div style={{ fontSize: 11, color: 'var(--color-text-tertiary)' }}>{t('cover.default_params_hint', { voice: selectedVoice?.voice || t('cover.this_voice') })}</div>
        </div>
      </div>

      {/* ③ 生成 */}
      <Button
        type="primary"
        size="large"
        icon={<ThunderboltOutlined />}
        loading={running}
        disabled={running || !selectedVoice || !songPath}
        onClick={handleGenerate}
        block
        style={{ height: 52, fontSize: 15, fontWeight: 600 }}
      >
        {running ? t('cover.generating') : t('cover.generate')}
      </Button>

      {error && <Alert type="error" showIcon message={t('cover.failed')} description={error} />}

      {(running || log) && (
        <div className="ant-card animate-fade-in" style={{ padding: 0 }}>
          <SectionHeader>
            <span style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              {t('cover.progress')}
              {running && (
                <Button danger size="small" icon={<StopOutlined />} onClick={cancel} style={{ marginLeft: 'auto' }}>
                  {t('common.cancel')}
                </Button>
              )}
            </span>
          </SectionHeader>
          <div style={{ padding: 20, display: 'flex', flexDirection: 'column', gap: 16 }}>
            <Steps size="small" current={stepIndex < 0 ? 0 : stepIndex} status={error ? 'error' : stage === 'done' ? 'finish' : 'process'} items={STAGES.map((s) => ({ title: t(s.titleKey) }))} />
            <div>
              <Progress
                percent={Math.round(percent)}
                status={error ? 'exception' : stage === 'done' ? 'success' : 'active'}
                strokeColor={{ '0%': '#7c3aed', '100%': '#a78bfa' }}
                trailColor="var(--color-bg-hover)"
              />
            </div>
            <LogViewer content={log} height={120} />
          </div>
        </div>
      )}

      {result && (
        <div className="ant-card animate-fade-in" style={{ padding: 0 }}>
          <SectionHeader>
            <span style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <PlayCircleOutlined style={{ color: 'var(--color-primary-hover)' }} />
              {t('cover.work')}
            </span>
          </SectionHeader>
          <div style={{ padding: 20, display: 'flex', flexDirection: 'column', gap: 12 }}>
            <audio controls autoPlay src={convertFileSrc(result.mixed_path)} style={{ width: '100%' }} />
            <div style={{ fontSize: 11, color: 'var(--color-text-tertiary)', wordBreak: 'break-all' }}>{result.mixed_path}</div>
            <div style={{ display: 'flex', gap: 8 }}>
              <Button size="small" icon={<ReloadOutlined />} disabled={running} onClick={handleGenerate}>
                {t('cover.regenerate')}
              </Button>
              <Button
                size="small"
                onClick={() => {
                  setSongPath('')
                  reset()
                }}
                disabled={running}
              >
                {t('cover.another_song')}
              </Button>
              <Button size="small" type="link" onClick={() => navigate('/infer')}>
                {t('cover.open_library')}
              </Button>
            </div>
          </div>
        </div>
      )}

      {/* 语音转换的作品列表：与「语音转换」页共用同一份成品库数据 */}
      <OutputLibrary onlyWorks refreshToken={result?.mixed_path || ''} title={t('library.works_title')} />

      <Drawer title={selectedVoice ? t('cover.settings_with_voice', { voice: selectedVoice.voice }) : t('cover.settings')} open={settingsOpen} onClose={closeSettings} width={380} destroyOnClose>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 24 }}>
          <div>
            <div style={lbl}>{t('cover.transpose', { value: params.transpose })}</div>
            <Slider min={-12} max={12} step={1} value={params.transpose} marks={{ '-12': '-12', 0: '0', 12: '+12' }} onChange={(v) => setParams((p) => ({ ...p, transpose: v }))} />
          </div>
          <div>
            <div style={lbl}>{t('cover.f0_method')}</div>
            <Select
              value={params.f0_method}
              style={{ width: '100%' }}
              onChange={(v) => setParams((p) => ({ ...p, f0_method: v }))}
              options={[
                { value: 'rmvpe', label: t('cover.f0_rmvpe') },
                { value: 'harvest', label: t('cover.f0_harvest') },
                { value: 'pm', label: t('cover.f0_pm') },
              ]}
            />
          </div>
          <div>
            <div style={lbl}>{t('cover.index_rate', { value: params.index_rate })}</div>
            <Slider min={0} max={1} step={0.05} value={params.index_rate} marks={{ 0: '0', 0.5: '0.5', 1: '1' }} onChange={(v) => setParams((p) => ({ ...p, index_rate: v }))} />
          </div>
          <div>
            <div style={lbl}>{t('cover.vocal_gain', { value: params.vocal_gain })}</div>
            <Slider min={-6} max={12} step={1} value={params.vocal_gain} marks={{ 0: '0', 6: '+6', 12: '+12' }} onChange={(v) => setParams((p) => ({ ...p, vocal_gain: v }))} />
          </div>
          <Button onClick={() => setParams(DEFAULT_COVER_PARAMS)}>{t('cover.reset_params')}</Button>
          <div style={{ fontSize: 11, color: 'var(--color-text-tertiary)', lineHeight: 1.6 }}>{t('cover.params_saved_hint', { voice: selectedVoice?.voice || t('cover.current_voice') })}</div>
        </div>
      </Drawer>
    </div>
  )
}
