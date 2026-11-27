import { useEffect, useState } from 'react'
import { Form, Input, Select, Slider, Button, Progress, Row, Col, Alert, message, Space } from 'antd'
import { ScissorOutlined, FolderOpenOutlined, FileOutlined, CheckCircleOutlined, WarningOutlined, MergeOutlined, StopOutlined } from '@ant-design/icons'
import { Link, useNavigate } from 'react-router-dom'
import { open } from '@tauri-apps/plugin-dialog'
import { useI18n } from '../../i18n'
import LogViewer from '../../components/LogViewer'
import { workspaceApi } from '../../api/workspace'
import { useUvrStore } from '../../store/uvrStore'

function CardHeader({ title }: { title: React.ReactNode }) {
  return <div style={{ padding: '12px 20px', borderBottom: '1px solid var(--color-border-subtle)', fontSize: 13, fontWeight: 600 }}>{title}</div>
}

function InfoSection({ title, items }: { title: string; items: { label?: string; desc: string }[] }) {
  return (
    <div>
      <div style={{ fontSize: 11, fontWeight: 600, color: 'var(--color-text-tertiary)', textTransform: 'uppercase', letterSpacing: '0.06em', marginBottom: 8 }}>{title}</div>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
        {items.map((item, i) => (
          <div key={i} style={{ display: 'flex', alignItems: 'flex-start', gap: 8, fontSize: 12 }}>
            {item.label ? (
              <code style={{ background: 'var(--color-bg-hover)', padding: '1px 6px', borderRadius: 4, fontSize: 11, color: 'var(--color-primary-hover)', flexShrink: 0 }}>{item.label}</code>
            ) : (
              <span style={{ color: 'var(--color-primary-hover)', flexShrink: 0 }}>·</span>
            )}
            <span style={{ color: 'var(--color-text-secondary)', lineHeight: 1.5 }}>{item.desc}</span>
          </div>
        ))}
      </div>
    </div>
  )
}

export default function UVRPage() {
  const navigate = useNavigate()
  const { t } = useI18n()
  const [form] = Form.useForm()
  const [selectedSongPath, setSelectedSongPath] = useState('')
  const [datasetsRoot, setDatasetsRoot] = useState('')
  const [aggValue, setAggValue] = useState(10)

  const loading = useUvrStore((s) => s.loading)
  const log = useUvrStore((s) => s.log)
  const progressPercent = useUvrStore((s) => s.progressPercent)
  const progressStep = useUvrStore((s) => s.progressStep)
  const progressCurrent = useUvrStore((s) => s.progressCurrent)
  const progressTotal = useUvrStore((s) => s.progressTotal)
  const result = useUvrStore((s) => s.result)
  const startSeparate = useUvrStore((s) => s.startSeparate)
  const cancelSeparate = useUvrStore((s) => s.cancel)

  useEffect(() => {
    workspaceApi
      .getDatasetsRoot()
      .then(setDatasetsRoot)
      .catch(() => {})
  }, [])

  const selectFolder = async (field: 'inputPath' | 'outputDir', title: string) => {
    try {
      const sel = await open({ directory: true, multiple: false, title })
      if (sel) {
        form.setFieldValue(field, sel)
        if (field === 'inputPath') setSelectedSongPath('')
      }
    } catch (e) {
      message.error(t('uvr.folder_pick_failed', { err: String(e) }))
    }
  }

  const selectSong = async () => {
    try {
      const sel = await open({ directory: false, multiple: false, title: t('uvr.pick_song_title'), filters: [{ name: 'Audio', extensions: ['mp3', 'wav', 'flac', 'ogg', 'm4a', 'aac'] }] })
      if (sel) {
        form.setFieldValue('inputPath', sel)
        setSelectedSongPath(sel)
        message.success(t('uvr.song_selected', { name: String(sel.split('/').pop()) }))
      }
    } catch (e) {
      message.error(t('uvr.song_pick_failed', { err: String(e) }))
    }
  }

  const handleSeparate = async () => {
    const v = form.getFieldsValue()
    if (!v.inputPath) {
      message.warning(t('uvr.input_required'))
      return
    }
    await startSeparate({
      model_name: v.model_name,
      input_path: v.inputPath,
      output_dir: v.outputDir || undefined,
      agg: v.agg,
      format: v.format,
    })
  }

  const UVR_MODELS = [
    { value: 'model_bs_roformer_ep_317_sdr_12.9755.ckpt', label: t('uvr.model_bs_roformer'), desc: t('uvr.model_bs_roformer_desc') },
    { value: 'UVR_MDXNET_Main.onnx', label: t('uvr.model_mdx_main'), desc: t('uvr.model_mdx_main_desc') },
    { value: 'UVR-MDX-NET-KARA_2.onnx', label: t('uvr.model_mdx_karaoke2'), desc: t('uvr.model_mdx_karaoke2_desc') },
    { value: 'UVR-MDX-NET-Voc_FT.onnx', label: t('uvr.model_mdx_voc_ft'), desc: t('uvr.model_mdx_voc_ft_desc') },
    { value: 'Kim_Vocal_2.onnx', label: t('uvr.model_kim_v2'), desc: t('uvr.model_kim_v2_desc') },
  ]

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 20 }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 12, marginBottom: 4 }}>
        <div className="icon-badge icon-badge--md" style={{ borderRadius: 10 }}>
          <ScissorOutlined />
        </div>
        <div>
          <h1 className="page-header__title">{t('uvr.title')}</h1>
          <p className="page-header__subtitle">{t('uvr.subtitle')}</p>
        </div>
      </div>

      <Alert type="info" showIcon message={t('uvr.usage')} description="" />

      <Row gutter={16}>
        <Col span={14}>
          <div className="ant-card" style={{ padding: 0 }}>
            <CardHeader title={t('uvr.input_config')} />
            <div style={{ padding: 20 }}>
              <Form form={form} layout="vertical" initialValues={{ format: 'flac', agg: 10, model_name: 'model_bs_roformer_ep_317_sdr_12.9755.ckpt' }}>
                <Form.Item label={t('uvr.model')} name="model_name" tooltip={t('uvr.model_tooltip')}>
                  <Select
                    options={UVR_MODELS.map((m) => ({
                      value: m.value,
                      label: (
                        <div style={{ display: 'flex', flexDirection: 'column', padding: '2px 0' }}>
                          <span style={{ fontWeight: 500 }}>{m.label}</span>
                          <span style={{ fontSize: 11, color: 'var(--color-text-tertiary)' }}>{m.desc}</span>
                        </div>
                      ),
                    }))}
                    listHeight={400}
                  />
                </Form.Item>
                <Form.Item label={t('uvr.source')} name="inputPath" rules={[{ required: true }]}>
                  <Input
                    placeholder={t('common.select')}
                    readOnly
                    addonAfter={
                      <Space size={0}>
                        <Button type="link" size="small" icon={<FileOutlined />} onClick={selectSong}>
                          {t('common.select')}
                        </Button>
                        <Button type="link" size="small" icon={<FolderOpenOutlined />} onClick={() => selectFolder('inputPath', t('common.select'))}>
                          {t('common.browse')}
                        </Button>
                      </Space>
                    }
                  />
                </Form.Item>
                {selectedSongPath && (
                  <Alert
                    type="info"
                    showIcon
                    style={{ marginBottom: 18, marginTop: -10 }}
                    message={t('uvr.single_mode')}
                    description={<span style={{ fontSize: 11, fontFamily: 'var(--font-mono)', wordBreak: 'break-all' }}>{selectedSongPath}</span>}
                  />
                )}
                <Form.Item label={t('uvr.output_dir')} name="outputDir">
                  <Input
                    placeholder={datasetsRoot ? t('uvr.output_dir_default', { path: `${datasetsRoot}/` }) : t('uvr.output_dir_placeholder')}
                    addonAfter={
                      <Button type="link" size="small" icon={<FolderOpenOutlined />} onClick={() => selectFolder('outputDir', t('common.browse'))}>
                        {t('common.browse')}
                      </Button>
                    }
                  />
                </Form.Item>
                <Row gutter={16}>
                  <Col span={12}>
                    <Form.Item label={t('uvr.output_format')} name="format">
                      <Select
                        options={[
                          { value: 'wav', label: 'WAV' },
                          { value: 'flac', label: 'FLAC' },
                          { value: 'mp3', label: 'MP3' },
                        ]}
                      />
                    </Form.Item>
                  </Col>
                  <Col span={12}>
                    <Form.Item label={t('uvr.agg_label', { value: aggValue })} name="agg">
                      <Slider min={0} max={20} step={1} marks={{ 0: t('uvr.agg_mild'), 10: t('uvr.agg_default'), 20: t('uvr.agg_aggressive') }} onChange={setAggValue} />
                    </Form.Item>
                  </Col>
                </Row>
              </Form>
            </div>
          </div>
        </Col>
        <Col span={10}>
          <div className="ant-card" style={{ padding: 0, height: '100%' }}>
            <CardHeader title={t('uvr.info')} />
            <div style={{ padding: 20, display: 'flex', flexDirection: 'column', gap: 20 }}>
              <InfoSection
                title={t('uvr.info_output_files')}
                items={[
                  { label: 'vocal.wav', desc: t('uvr.info_vocal_desc') },
                  { label: 'instrument.wav', desc: t('uvr.info_instrument_desc') },
                ]}
              />
              <InfoSection title={t('uvr.info_tips')} items={[{ desc: t('uvr.tip_use_vocal') }, { desc: t('uvr.tip_keep_instrument') }]} />
            </div>
          </div>
        </Col>
      </Row>

      <Row gutter={12}>
        <Col flex="auto">
          <Button type="primary" size="large" icon={<ScissorOutlined />} loading={loading} onClick={handleSeparate} block style={{ height: 44, fontSize: 14, fontWeight: 600 }}>
            {loading ? t('uvr.separating') : t('uvr.start')}
          </Button>
        </Col>
        {loading && (
          <Col>
            <Button danger size="large" icon={<StopOutlined />} onClick={cancelSeparate} style={{ height: 44, fontSize: 14 }}>
              {t('common.cancel')}
            </Button>
          </Col>
        )}
      </Row>

      {(loading || log) && (
        <div className="ant-card animate-fade-in" style={{ padding: 0 }}>
          <CardHeader title={t('uvr.progress')} />
          <div style={{ padding: 20, display: 'flex', flexDirection: 'column', gap: 12 }}>
            {loading && (
              <div>
                <Progress percent={progressPercent} status={progressPercent === 100 ? 'success' : 'active'} strokeColor={{ '0%': '#7c3aed', '100%': '#a78bfa' }} trailColor="var(--color-bg-hover)" />
                <div style={{ fontSize: 12, color: 'var(--color-text-tertiary)', marginTop: 4 }}>
                  {progressStep === 'file'
                    ? t('uvr.progress_file', { cur: progressCurrent, total: progressTotal })
                    : progressStep === 'segment'
                      ? t('uvr.progress_segment', { cur: progressCurrent, total: progressTotal })
                      : t('uvr.progress_preparing')}
                </div>
              </div>
            )}
            <LogViewer content={log} height={140} />
          </div>
        </div>
      )}

      {result && (
        <div className="ant-card animate-fade-in" style={{ padding: 0 }}>
          <CardHeader
            title={
              <span style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                {result.ok ? <CheckCircleOutlined style={{ color: 'var(--color-success)' }} /> : <WarningOutlined style={{ color: 'var(--color-warning)' }} />}
                {t('uvr.result')}
              </span>
            }
          />
          <div style={{ padding: 20, display: 'flex', flexDirection: 'column', gap: 14 }}>
            <Alert type={result.ok ? 'success' : 'warning'} showIcon message={result.ok ? t('uvr.result_detected') : t('uvr.result_missing')} />
            {[
              { label: t('uvr.vocal_dir'), value: result.vocalDir, files: result.vocalFiles },
              { label: t('uvr.instrument_dir'), value: result.instrumentDir, files: result.instrumentFiles },
            ].map((row) => (
              <div key={row.label}>
                <div style={{ fontSize: 11, fontWeight: 600, color: 'var(--color-text-tertiary)', textTransform: 'uppercase', letterSpacing: '0.06em', marginBottom: 6 }}>{row.label}</div>
                <code
                  style={{
                    fontSize: 11,
                    color: 'var(--color-text-secondary)',
                    background: 'var(--color-bg-base)',
                    padding: '3px 8px',
                    borderRadius: 4,
                    display: 'block',
                    marginBottom: 6,
                    wordBreak: 'break-all',
                  }}
                >
                  {row.value}
                </code>
                {row.files.length > 0 ? (
                  row.files.map((f) => (
                    <div key={f} style={{ fontSize: 12, color: 'var(--color-text-tertiary)', paddingLeft: 8, display: 'flex', alignItems: 'center', gap: 8, marginBottom: 2 }}>
                      <span style={{ flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>· {f}</span>
                      {row.label === t('uvr.instrument_dir') && (
                        <Button
                          type="link"
                          size="small"
                          style={{ fontSize: 11, flexShrink: 0 }}
                          onClick={() => {
                            navigate('/infer')
                            message.success(t('uvr.instrument_fill_hint'))
                          }}
                        >
                          {t('uvr.use_instrument')}
                        </Button>
                      )}
                    </div>
                  ))
                ) : (
                  <div style={{ fontSize: 12, color: 'var(--color-text-tertiary)', paddingLeft: 8 }}>· {t('uvr.not_detected')}</div>
                )}
              </div>
            ))}
            {result.ok && result.instrumentFiles.length > 0 && (
              <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
                <Button
                  type="primary"
                  icon={<MergeOutlined />}
                  onClick={() => {
                    navigate('/infer')
                    message.success(t('uvr.instrument_merge_hint'))
                  }}
                >
                  {t('infer.merge')}
                </Button>
              </div>
            )}
          </div>
        </div>
      )}

      <div className="ant-card" style={{ padding: 0 }}>
        <CardHeader title={t('uvr.next_steps')} />
        <div style={{ padding: 20, display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 16 }}>
          <div style={{ fontSize: 13, color: 'var(--color-text-secondary)' }}>{t('uvr.next_step')}</div>
          <Link to="/infer">
            <Button type="primary" style={{ flexShrink: 0 }}>
              {t('uvr.go_infer')}
            </Button>
          </Link>
        </div>
      </div>
    </div>
  )
}
