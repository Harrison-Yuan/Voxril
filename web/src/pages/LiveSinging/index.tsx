import { useEffect, useState } from 'react'
import { Form, Select, Button, Row, Col, Alert, message, Card, Progress, Statistic, Space, Tag, Slider } from 'antd'
import { AudioOutlined, SettingOutlined, PlayCircleOutlined, StopOutlined, SoundOutlined, QuestionCircleOutlined, RadarChartOutlined, LoadingOutlined } from '@ant-design/icons'
import { invoke } from '@tauri-apps/api/core'
import { useLiveSingingStore } from '@/store/liveSingingStore'
import { inferApi, type SeparatedSong } from '@/api/infer'
import { useI18n } from '@/i18n'
import LogViewer from '../../components/LogViewer'
import LyricsPlayer from '../../components/LyricsPlayer'
import LyricsSearchModal from '../../components/LyricsSearchModal'

/**
 * Ant Design Select 的下拉面板渲染在 body 的 portal 中，
 * 所以样式必须放在全局作用域而非组件局部。使用 :global 穿透 CSS Modules。
 */
const styles = `
.song-option-item .f0-extract-btn {
  opacity: 0.35;
  transition: opacity 0.15s ease;
}
.song-option-item:hover .f0-extract-btn {
  opacity: 1;
}
`
if (typeof document !== 'undefined' && !document.getElementById('f0-extract-style')) {
  const el = document.createElement('style')
  el.id = 'f0-extract-style'
  el.textContent = styles
  document.head.appendChild(el)
}

interface AudioDevice {
  name: string
  is_default: boolean
  kind: 'physical' | 'blackhole' | 'virtual'
}

function getStateLabels(t: (key: string) => string) {
  return {
    mute: { label: t('live.voice_mute'), color: 'var(--color-text-quaternary)' },
    speaking: { label: t('live.voice_active'), color: 'var(--color-warning)' },
    singing: { label: t('live.voice_active'), color: 'var(--color-success)' },
  } as Record<string, { label: string; color: string }>
}

export default function LiveSingingPage() {
  const { t } = useI18n()
  const [form] = Form.useForm()
  const [inputDevices, setInputDevices] = useState<AudioDevice[]>([])
  const [outputDevices, setOutputDevices] = useState<AudioDevice[]>([])
  const [separatedSongs, setSeparatedSongs] = useState<SeparatedSong[]>([])
  const [loadingSongs, setLoadingSongs] = useState(false)

  const connected = useLiveSingingStore(s => s.connected)
  const loading = useLiveSingingStore(s => s.loading)
  const metrics = useLiveSingingStore(s => s.metrics)
  const startError = useLiveSingingStore(s => s.startError)
  const start = useLiveSingingStore(s => s.start)
  const stop = useLiveSingingStore(s => s.stop)
  const extractF0 = useLiveSingingStore(s => s.extractF0)

  const handleExtractF0 = async (song: SeparatedSong) => {
    setExtractingSongs(prev => ({ ...prev, [song.name]: true }))
    try {
      await extractF0(song.vocal_path)
      await fetchSeparatedSongs()
      message.success(`${t('live.f0_extract_done')}：${song.name}`)
    } catch (err) {
      message.error(`${t('live.f0_extract_failed')}: ${err}`)
    } finally {
      setExtractingSongs(prev => ({ ...prev, [song.name]: false }))
    }
  }

  const [extractingSongs, setExtractingSongs] = useState<Record<string, boolean>>({})
  const [selectedSongForPreview, setSelectedSongForPreview] = useState<string>()

  useEffect(() => {
    fetchDevices()
    fetchSeparatedSongs()
    useLiveSingingStore.getState().checkStatus()
  }, [])

  const fetchSeparatedSongs = async () => {
    setLoadingSongs(true)
    try {
      const songs = await inferApi.listSeparatedSongs()
      console.log('[LiveSinging] separated songs:', songs.length, songs)
      setSeparatedSongs(songs)
    } catch (e) {
      console.warn('[LiveSinging] failed to fetch songs:', e)
    } finally {
      setLoadingSongs(false)
    }
  }

  const fetchDevices = async () => {
    try {
      const [inputs, outputs] = await invoke<[AudioDevice[], AudioDevice[]]>('get_audio_devices')
      setInputDevices(inputs)
      setOutputDevices(outputs)
      const defIn = inputs.find(d => d.is_default)?.name || inputs[0]?.name
      const defOut = outputs.find(d => d.is_default)?.name || outputs[0]?.name
      form.setFieldsValue({ inputDevice: defIn, outputDevice: defOut })
    } catch (e) {
      message.error(`获取音频设备失败: ${e}`)
    }
  }

  const handleToggleStream = async () => {
    if (connected) {
      const ok = await stop()
      if (ok) message.success(t('live.stopped'))
      else message.error(t('live.stop_failed'))
    } else {
      const v = form.getFieldsValue()
      if (!v.inputDevice) {
        message.warning(t('live.select_mic'))
        return
      }
      const result = await start({
        input_device: v.inputDevice,
        output_device: v.outputDevice,
        model_path: '',
        f0_up_key: v.f0UpKey || 0,
        accompaniment_path: '',
        cover_path: '',
        duck_gain: v.duckGain || 0.3,
      })
      if (result.ok) {
        message.success(t('live.engine_started'))
      } else {
        message.error(`${t('live.start_failed_generic')}: ${result.error || t('status.failed')}`)
      }
    }
  }

  const stateLabels = getStateLabels(t)
  const stateInfo = stateLabels[metrics.state] || stateLabels.mute
  const meterPercent = Math.min(metrics.rms * 200, 100)

  const progressPercent = useLiveSingingStore(s => s.progressPercent)
  const progressMessage = useLiveSingingStore(s => s.progressMessage)
  const log = useLiveSingingStore(s => s.log)
  const accompanimentKey = useLiveSingingStore(s => s.accompanimentKey)

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 20 }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 12, marginBottom: 4 }}>
        <div className="icon-badge icon-badge--md" style={{
          borderRadius: 10,
          background: connected ? 'var(--color-success-subtle)' : undefined,
        }}>
          <AudioOutlined style={{ color: connected ? 'var(--color-success)' : undefined }} />
        </div>
        <div>
          <h1 className="page-header__title">{t('live.title')}</h1>
          <p className="page-header__subtitle">{t('live.subtitle')}</p>
        </div>
      </div>

      {startError && !connected && (
        <Alert
          type="error"
          showIcon
          closable
          message={t('live.start_failed')}
          description={
            <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
              <span style={{ flex: 1 }}>{startError}</span>
              {(startError.includes('运行中') || startError.includes('超时') || startError.includes('模型不存在')) && (
                <Button size="small" danger onClick={async () => {
                  await useLiveSingingStore.getState().stop()
                  useLiveSingingStore.setState({ startError: null })
                }}>
                  {t('live.terminate_reset')}
                </Button>
              )}
            </div>
          }
          onClose={() => useLiveSingingStore.setState({ startError: null })}
        />
      )}

      <Alert
        type={connected ? "success" : "info"}
        showIcon
        icon={<AudioOutlined />}
        message={connected ? t('live.engine_running') : t('live.engine_ready_desc')}
        description={
          connected
            ? `${t('live.current_status')}: ${stateInfo.label} · ${t('live.engine_latency')} ${metrics.engine_load_ms.toFixed(1)}ms`
            : undefined
        }
      />

      <Row gutter={16}>
        <Col span={14}>
          <Card title={<><SettingOutlined /> {t('live.config')}</>} variant="borderless">
            <Form
              form={form}
              layout="vertical"
              initialValues={{ f0UpKey: 0, duckGain: 0.3 }}
            >
              <Row gutter={16}>
                <Col span={12}>
                  <Form.Item label={t('live.microphone')} name="inputDevice" tooltip={t('live.microphone_tooltip')}>
                    <Select
                      disabled={connected || loading}
                      options={inputDevices.map(d => ({
                        value: d.name,
                        label: d.name + (d.is_default ? t('live.default_suffix') : ''),
                      }))}
                    />
                  </Form.Item>
                </Col>
                <Col span={12}>
                  <Form.Item label={t('live.output_device')} name="outputDevice" tooltip={t('live.output_device_tooltip')}>
                    <Select
                      disabled={connected || loading}
                      options={outputDevices.map(d => ({
                        value: d.name,
                        label: d.name + (d.is_default ? t('live.default_suffix') : ''),
                      }))}
                    />
                  </Form.Item>
                </Col>
              </Row>

              <Form.Item label={t('live.preset_song')} name="selectedSong"
                tooltip={t('live.preset_song_tooltip')}>
                <Select
                  disabled={loading}
                  loading={loadingSongs}
                  placeholder={t('live.select_song_placeholder')}
                  allowClear
                  onChange={(songName: string) => {
                    if (!songName) {
                      form.setFieldValue('accompanimentPath', undefined)
                      setSelectedSongForPreview(undefined)
                      return
                    }
                    const song = separatedSongs.find(s => s.name === songName)
                    if (song) {
                      form.setFieldValue('accompanimentPath', song.instrumental_path)
                      form.setFieldsValue({ accompanimentPath: song.instrumental_path })
                      setSelectedSongForPreview(songName)
                    }
                  }}
                  optionRender={(option) => {
                    const song = separatedSongs.find(s => s.name === option.value)
                    if (!song) return <>{option.label}</>
                    const isExtracting = extractingSongs[song.name]
                    return (
                      <div
                        style={{
                          display: 'flex',
                          alignItems: 'center',
                          justifyContent: 'space-between',
                          height: 32,
                          lineHeight: '32px',
                        }}
                        className="song-option-item"
                      >
                        <Space>
                          <span>{song.name}</span>
                          {song.f0_exists ? (
                            <Tag color="success" style={{ fontSize: 10, lineHeight: '18px', padding: '0 6px' }}>{t('live.f0_ready')}</Tag>
                          ) : (
                            <Tag style={{ fontSize: 10, lineHeight: '18px', padding: '0 6px' }}>{t('live.f0_pending')}</Tag>
                          )}
                          {song.lrc_exists ? (
                            <Tag color="purple" style={{ fontSize: 10, lineHeight: '18px', padding: '0 6px' }}>{t('live.lyrics_ready')}</Tag>
                          ) : (
                            <Tag style={{ fontSize: 10, lineHeight: '18px', padding: '0 6px' }}>{t('live.lyrics_pending')}</Tag>
                          )}
                        </Space>
                        {isExtracting ? (
                          <LoadingOutlined style={{ color: 'var(--color-primary)' }} />
                        ) : (
                          <Space size={0}>
                            {!song.f0_exists && (
                              <Button
                                type="link"
                                size="small"
                                style={{ minWidth: 0, padding: '0 6px', fontSize: 12 }}
                                onClick={async (e) => {
                                  e.stopPropagation()
                                  e.preventDefault()
                                  await handleExtractF0(song)
                                }}
                              >
                                <RadarChartOutlined style={{ marginRight: 2 }} />
                                {t('live.extract_f0')}
                              </Button>
                            )}
                            <LyricsSearchModal
                              audioPath={song.vocal_path}
                              songName={song.name}
                              onDownloaded={() => {
                                fetchSeparatedSongs()
                              }}
                            >
                              {(openModal) => (
                                <Button
                                  type="link"
                                  size="small"
                                  style={{ minWidth: 0, padding: '0 6px', fontSize: 12 }}
                                  onClick={(e) => {
                                    e.stopPropagation()
                                    e.preventDefault()
                                    openModal()
                                  }}
                                >
                                  <RadarChartOutlined style={{ marginRight: 2 }} />
                                  {song.lrc_exists ? t('live.research_lyrics') : t('live.search_lyrics')}
                                </Button>
                              )}
                            </LyricsSearchModal>
                          </Space>
                        )}
                      </div>
                    )
                  }}
                >
                  {separatedSongs.length === 0 ? (
                    <Select.Option value="" disabled>{t('live.no_songs_available')}</Select.Option>
                  ) : (
                    separatedSongs.map(song => (
                      <Select.Option key={song.name} value={song.name}>
                        <span>{song.name}</span>
                      </Select.Option>
                    ))
                  )}
                </Select>
              </Form.Item>

              <Form.Item label={t('live.pitch_shift')} name="f0UpKey" tooltip={t('live.pitch_shift_tooltip')}>
                <Slider
                  min={-12}
                  max={12}
                  step={1}
                  marks={{
                    [-12]: '-12',
                    [-6]: '-6',
                    0: '0',
                    6: '6',
                    12: '+12',
                  } as Record<number, string>}
                  onChange={(val: number) => {
                    if (connected) {
                      invoke('set_live_f0_up_key', { semitones: val }).catch(() => { })
                    }
                  }}
                />
              </Form.Item>
            </Form>
          </Card>
        </Col>

        <Col span={10}>
          <Card
            title={<><SoundOutlined /> {t('live.console')}</>}
            variant="borderless"
            style={{ height: '100%' }}
          >
            <div style={{
              display: 'flex',
              flexDirection: 'column',
              gap: 16,
              alignItems: 'center',
              justifyContent: 'center',
              height: '100%',
              paddingBottom: 20,
            }}>
              <div style={{ textAlign: 'center' }}>
                <div style={{
                  fontSize: 48,
                  color: connected ? stateInfo.color : 'var(--color-text-quaternary)',
                  transition: 'color 0.3s',
                }}>
                  <AudioOutlined />
                </div>
                <div style={{ marginTop: 8, fontWeight: 600, fontSize: 16 }}>
                  {connected ? stateInfo.label : t('live.offline')}
                </div>
                {connected && (
                  <div style={{ fontSize: 12, color: 'var(--color-text-tertiary)', marginTop: 4 }}>
                    {t('live.continuity')}: {(metrics.f0_continuity * 100).toFixed(0)}% · RMS: {metrics.rms.toFixed(4)}
                  </div>
                )}
              </div>

              {connected && (
                <>
                  <div style={{
                    width: '100%',
                    padding: '8px 0',
                    textAlign: 'center',
                    background: metrics.f0 > 0 ? 'var(--color-success-subtle)' : 'transparent',
                    borderRadius: 8,
                    transition: 'background 0.15s',
                  }}>
                    <div style={{ fontSize: 28, fontWeight: 700, fontFamily: 'monospace', lineHeight: 1.2 }}>
                      {metrics.f0 > 0 ? `${metrics.f0.toFixed(0)} Hz` : '—'}
                    </div>
                    <div style={{ fontSize: 11, color: 'var(--color-text-tertiary)', marginTop: 2 }}>
                      {t('live.live_pitch')} {metrics.f0 > 0 ? t('live.voice_active') : t('live.voice_mute')}
                    </div>
                  </div>

                  <Progress
                    percent={meterPercent}
                    showInfo={false}
                    strokeColor={{
                      '0%': '#7c3aed',
                      '50%': stateInfo.color,
                      '100%': metrics.state === 'singing' ? '#22c55e' : '#eab308',
                    }}
                    trailColor="var(--color-bg-hover)"
                    size="small"
                    style={{ width: '100%' }}
                  />
                  <div style={{ display: 'flex', gap: 8, fontSize: 11, color: 'var(--color-text-tertiary)', justifyContent: 'space-between', width: '100%', marginTop: -8 }}>
                    <span>{t('live.level')}: {meterPercent.toFixed(0)}%</span>
                    <span>{t('live.continuity')}: {(metrics.f0_continuity * 100).toFixed(0)}%</span>
                  </div>

                  <Space size={12} style={{ width: '100%', justifyContent: 'center' }}>
                    <Card size="small" variant="borderless" style={{ flex: 1, textAlign: 'center', background: 'var(--color-bg-base)' }}>
                      <Statistic
                        title={t('live.engine_latency')}
                        value={metrics.engine_load_ms}
                        suffix="ms"
                        precision={1}
                        valueStyle={{ fontSize: 16, color: metrics.engine_load_ms < 30 ? 'var(--color-success)' : 'var(--color-warning)' }}
                      />
                    </Card>
                    <Card size="small" variant="borderless" style={{ flex: 1, textAlign: 'center', background: 'var(--color-bg-base)' }}>
                      <Statistic
                        title={t('live.f0_stability')}
                        value={metrics.f0_continuity * 100}
                        suffix="%"
                        precision={0}
                        valueStyle={{ fontSize: 16, color: metrics.f0_continuity > 0.6 ? 'var(--color-success)' : 'var(--color-text-tertiary)' }}
                      />
                    </Card>
                    <Card size="small" variant="borderless" style={{ flex: 1, textAlign: 'center', background: 'var(--color-bg-base)' }}>
                      <div style={{ fontSize: 11, color: 'var(--color-text-tertiary)', marginBottom: 2 }}>调性</div>
                      <div style={{ fontSize: 16, fontWeight: 700, fontFamily: 'monospace' }}>
                        {metrics.detected_key || accompanimentKey || '—'}
                      </div>
                      <div style={{ fontSize: 10, color: 'var(--color-text-quaternary)', marginTop: 1 }}>
                        {accompanimentKey ? '伴奏' : metrics.detected_key ? '人声' : ''}
                      </div>
                    </Card>
                  </Space>
                </>
              )}

              {loading && (
                <div style={{ width: '100%', textAlign: 'center', padding: '4px 0' }}>
                  <Progress
                    percent={progressPercent}
                    strokeColor={{
                      '0%': '#7c3aed',
                      '100%': '#a78bfa',
                    }}
                    style={{ margin: '4px 0' }}
                  />
                  <div style={{ fontSize: 12, color: 'var(--color-text-tertiary)' }}>
                    {progressMessage || t('common.loading')}
                </div>
              </div>
              )}

              <Button
                type="primary"
                danger={connected}
                size="large"
                icon={connected ? <StopOutlined /> : <PlayCircleOutlined />}
                loading={loading}
                onClick={handleToggleStream}
                style={{
                  width: 180,
                  height: 54,
                  borderRadius: 27,
                  fontSize: 18,
                  fontWeight: 700,
                  marginTop: 8,
                }}
              >
                {connected ? t('live.go_offline') : t('live.go_live')}
              </Button>
            </div>
          </Card>
        </Col>
      </Row>

      {/* 歌词 + 播放控制 */}
      <LyricsPlayer
        connected={connected}
        songs={separatedSongs}
        selectedSongName={selectedSongForPreview}
      />

      <Card
        title={<><QuestionCircleOutlined /> {t('live.how_it_works')}</>}
        variant="borderless"
      >
        <div style={{ fontSize: 13, color: 'var(--color-text-secondary)', lineHeight: 2 }}>
          <div style={{ fontWeight: 600, color: 'var(--color-text-primary)', marginBottom: 8 }}>
            {t('live.how_it_works_title')}
          </div>
          <ol style={{ paddingLeft: 18, margin: 0, marginBottom: 12 }}>
            <li dangerouslySetInnerHTML={{ __html: t('live.how_it_works_step1') }} />
            <li dangerouslySetInnerHTML={{ __html: t('live.how_it_works_step2') }} />
            <li dangerouslySetInnerHTML={{ __html: t('live.how_it_works_step3') }} />
            <li dangerouslySetInnerHTML={{ __html: t('live.how_it_works_step4') }} />
            <li dangerouslySetInnerHTML={{ __html: t('live.how_it_works_step5') }} />
          </ol>
          <div style={{ fontWeight: 600, color: 'var(--color-text-primary)', marginBottom: 8 }}>
            {t('live.f0_profile_title')}
          </div>
          <ul style={{ paddingLeft: 18, margin: 0 }}>
            <li>{t('live.f0_profile_desc_1')}</li>
            <li>{t('live.f0_profile_desc_2')}</li>
            <li>{t('live.f0_profile_desc_3')}</li>
          </ul>
        </div>
      </Card>

      {(log || loading || startError) && (
        <div className="ant-card animate-fade-in" style={{ padding: 0 }}>
          <div style={{ padding: '12px 20px', borderBottom: '1px solid var(--color-border-subtle)', fontSize: 13, fontWeight: 600 }}>
            {t('live.engine_log')}
          </div>
          <div style={{ padding: 20 }}>
            <LogViewer content={log} height={200} />
          </div>
        </div>
      )}
    </div>
  )
}
