import { useEffect } from 'react'
import { Form, Select, Slider, Button, Progress, Row, Col, Alert, message, Input, Collapse, Modal, List } from 'antd'
import { UploadOutlined, SoundOutlined, MergeOutlined, StopOutlined } from '@ant-design/icons'
import LogViewer from '../../components/LogViewer'
import { OutputLibrary } from '../../components/OutputLibrary'
import { useInferState } from './useInferState'
import { useInferStore } from '@/store/inferStore'
import { useI18n } from '@/i18n'

function SectionHeader({ children }: { children: React.ReactNode }) {
  return <div style={{ padding: '12px 20px', borderBottom: '1px solid var(--color-border-subtle)', fontSize: 13, fontWeight: 600 }}>{children}</div>
}

const lbl = { fontSize: 11, fontWeight: 600, color: 'var(--color-text-tertiary)', textTransform: 'uppercase' as const, letterSpacing: '0.06em', marginBottom: 8 }

export default function InferPage() {
  const s = useInferState()
  const storeCancel = useInferStore((x) => x.cancel)
  const { t } = useI18n()

  useEffect(() => {
    if (!s.instrumentalFiles.length) return
    Modal.info({
      title: t('infer.pick_instrumental'),
      width: 600,
      content: (
        <List
          dataSource={s.instrumentalFiles}
          renderItem={(f: { name: string; path: string }) => (
            <List.Item
              style={{ cursor: 'pointer' }}
              onClick={() => {
                s.mergeForm.setFieldValue('instrumentalPath', f.path)
                message.success(t('infer.selected', { name: f.name }))
                s.setInstrumentalFiles([])
                Modal.destroyAll()
              }}
            >
              {f.name}
            </List.Item>
          )}
        />
      ),
      onCancel: () => s.setInstrumentalFiles([]),
    })
  }, [s.instrumentalFiles])

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 20 }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 12, marginBottom: 4 }}>
        <div className="icon-badge icon-badge--md" style={{ borderRadius: 10 }}>
          <SoundOutlined />
        </div>
        <div>
          <h1 className="page-header__title">{t('infer.title')}</h1>
          <p className="page-header__subtitle">{t('infer.subtitle')}</p>
        </div>
      </div>

      <Row gutter={16}>
        <Col xs={24} lg={15}>
          <div className="ant-card" style={{ padding: 0 }}>
            <SectionHeader>
              <span style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                <SoundOutlined style={{ color: 'var(--color-primary-hover)' }} />
                {t('infer.audio_convert')}
              </span>
            </SectionHeader>
            <div style={{ padding: 20 }}>
              <Form
                form={s.form}
                layout="vertical"
                initialValues={{ modelName: '', transpose: 0, f0Method: 'rmvpe', indexRate: 0.75, filterRadius: 3, resampleSr: 0, rmsMixRate: 1.0, protect: 0.33 }}
                onValuesChange={(_, all) => s.setFv(all)}
              >
                <Row gutter={16}>
                  <Col span={12}>
                    <Form.Item label={t('infer.model')} name="modelName" rules={[{ required: true, message: t('infer.model_required') }]}>
                      <Select
                        placeholder={s.models.length === 0 ? t('infer.no_model_hint') : t('infer.select_model')}
                        options={s.models.map((m) => {
                          let label = m
                          if (m.includes('/checkpoints/')) {
                            const parts = m.split('/')
                            const ckptName = parts[parts.length - 1].replace('.pth', '')
                            const match = ckptName.match(/_e(\d+)/)
                            const base = parts.slice(0, -2).join('/') + '/' + parts[parts.length - 3]
                            label = match ? `${base} · epoch ${match[1]}` : m
                          } else {
                            label = m + ' · latest'
                          }
                          return { value: m, label }
                        })}
                        notFoundContent={t('common.no_data')}
                      />
                    </Form.Item>
                  </Col>
                  <Col span={12}>
                    <Form.Item label={t('infer.f0_method')} name="f0Method">
                      <Select
                        options={[
                          { value: 'rmvpe', label: t('infer.f0_rmvpe') },
                          { value: 'harvest', label: t('infer.f0_harvest') },
                          { value: 'pm', label: t('infer.f0_pm') },
                        ]}
                      />
                    </Form.Item>
                  </Col>
                </Row>
                <Form.Item label={t('infer.audio_file')}>
                  <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
                    <Button icon={<UploadOutlined />} onClick={s.selectSource} block>
                      {s.sourcePath ? t('infer.change_audio') : t('infer.source_audio')}
                    </Button>
                    {s.sourcePath && <Alert type="info" showIcon message={s.sourceName || s.sourcePath} style={{ fontSize: 12 }} />}
                  </div>
                </Form.Item>
                <Form.Item label={t('infer.transpose_semitone', { value: s.fv.transpose ?? 0 })} name="transpose">
                  <Slider min={-24} max={24} step={1} marks={{ '-12': '-12', 0: '0', 12: '+12' }} />
                </Form.Item>
                <Form.Item label={t('infer.index_rate_label', { value: s.fv.indexRate ?? 0.75 })} name="indexRate">
                  <Slider min={0} max={1} step={0.05} marks={{ 0: '0', 0.5: '0.5', 1: '1' }} />
                </Form.Item>
                <Form.Item name="protect" hidden>
                  <Input type="hidden" />
                </Form.Item>
                <Form.Item name="filterRadius" hidden>
                  <Input type="hidden" />
                </Form.Item>
                <Form.Item name="rmsMixRate" hidden>
                  <Input type="hidden" />
                </Form.Item>
                <Row gutter={12}>
                  <Col flex="auto">
                    <Button type="primary" size="large" icon={<SoundOutlined />} loading={s.loading} onClick={s.handleConvert} block style={{ height: 44, fontSize: 14, fontWeight: 600 }}>
                      {s.loading ? t('infer.converting') : t('infer.start')}
                    </Button>
                  </Col>
                  {s.loading && (
                    <Col>
                      <Button danger size="large" icon={<StopOutlined />} onClick={storeCancel} style={{ height: 44, fontSize: 14 }}>
                        {t('common.cancel')}
                      </Button>
                    </Col>
                  )}
                </Row>
              </Form>
            </div>
          </div>
        </Col>

        <Col xs={24} lg={9}>
          <div className="ant-card" style={{ padding: 0 }}>
            <SectionHeader>{t('infer.param_tips')}</SectionHeader>
            <div style={{ padding: 20, display: 'flex', flexDirection: 'column', gap: 20 }}>
              <div>
                <div style={lbl}>{t('infer.transpose_tips')}</div>
                {[
                  { label: t('infer.female_to_male'), desc: t('infer.tip_female_to_male') },
                  { label: t('infer.male_to_female'), desc: t('infer.tip_male_to_female') },
                  { label: t('infer.same_gender'), desc: t('infer.tip_same_gender') },
                ].map((item, i) => (
                  <div key={i} style={{ display: 'flex', alignItems: 'flex-start', gap: 8, fontSize: 12, marginBottom: 5 }}>
                    <code style={{ background: 'var(--color-bg-hover)', padding: '1px 6px', borderRadius: 4, fontSize: 11, color: 'var(--color-primary-hover)', flexShrink: 0 }}>{item.label}</code>
                    <span style={{ color: 'var(--color-text-secondary)', lineHeight: 1.5 }}>{item.desc}</span>
                  </div>
                ))}
              </div>
              <div>
                <div style={lbl}>{t('infer.workflow')}</div>
                {[t('infer.workflow_1'), t('infer.workflow_2'), t('infer.merge_desc')].map((desc, i) => (
                  <div key={i} style={{ display: 'flex', alignItems: 'flex-start', gap: 8, fontSize: 12, marginBottom: 5 }}>
                    <span style={{ color: 'var(--color-primary-hover)', flexShrink: 0 }}>·</span>
                    <span style={{ color: 'var(--color-text-secondary)', lineHeight: 1.5 }}>{desc}</span>
                  </div>
                ))}
              </div>
            </div>
          </div>
        </Col>
      </Row>

      {s.loading && (
        <div className="ant-card animate-fade-in" style={{ padding: 0 }}>
          <SectionHeader>{t('infer.progress')}</SectionHeader>
          <div style={{ padding: 20, display: 'flex', flexDirection: 'column', gap: 12 }}>
            <div>
              <Progress percent={s.pct} status={s.pct === 100 ? 'success' : 'active'} strokeColor={{ '0%': '#7c3aed', '100%': '#a78bfa' }} trailColor="var(--color-bg-hover)" />
              <div style={{ fontSize: 12, color: 'var(--color-text-tertiary)', marginTop: 4 }}>{s.tot > 1 ? t('infer.segment', { cur: s.cur, tot: s.tot }) : t('common.running')}</div>
            </div>
            <LogViewer content={s.log} height={120} />
          </div>
        </div>
      )}

      {!s.loading && s.log && (
        <div className="ant-card animate-fade-in" style={{ padding: 0 }}>
          <SectionHeader>{t('infer.log')}</SectionHeader>
          <div style={{ padding: 20 }}>
            <LogViewer content={s.log} height={120} />
          </div>
        </div>
      )}

      {s.outputUrl && (
        <Alert
          type="success"
          showIcon
          message={t('infer.done')}
          description={
            <div style={{ marginTop: 4, display: 'flex', flexDirection: 'column', gap: 2 }}>
              <span>
                {t('infer.output_file')}
                <code style={{ fontSize: 12 }}>{s.outputUrl.split('/').pop()}</code>
              </span>
              <span style={{ fontSize: 11, color: 'var(--color-text-tertiary)' }}>{s.outputUrl}</span>
            </div>
          }
        />
      )}

      <Collapse
        bordered={false}
        style={{ background: 'transparent' }}
        items={[
          {
            key: 'merge',
            label: (
              <span style={{ display: 'flex', alignItems: 'center', gap: 8, fontSize: 13, fontWeight: 500 }}>
                <MergeOutlined style={{ color: 'var(--color-primary-hover)' }} />
                {t('infer.merge')}
              </span>
            ),
            children: (
              <Form form={s.mergeForm} layout="vertical" initialValues={{ vocalGain: 6 }}>
                <Row gutter={16}>
                  <Col span={12}>
                    <Form.Item label={t('infer.new_vocal')} name="vocalPath">
                      <Input
                        readOnly
                        placeholder={t('infer.vocal_auto_fill')}
                        addonAfter={
                          <Button type="link" size="small" onClick={() => s.selectMerge('vocalPath', t('infer.select_vocal'))}>
                            {t('common.browse')}
                          </Button>
                        }
                      />
                    </Form.Item>
                  </Col>
                  <Col span={12}>
                    <Form.Item label={t('infer.accompaniment')} name="instrumentalPath">
                      <Input
                        readOnly
                        placeholder={t('infer.instrumental_auto_detect')}
                        addonAfter={
                          <span style={{ display: 'flex' }}>
                            {s.workspaceRoot && (
                              <Button type="link" size="small" onClick={s.quickInstrumental}>
                                {t('infer.quick')}
                              </Button>
                            )}
                            <Button type="link" size="small" onClick={() => s.selectMerge('instrumentalPath', t('infer.select_accompaniment'))}>
                              {t('common.browse')}
                            </Button>
                          </span>
                        }
                      />
                    </Form.Item>
                  </Col>
                </Row>
                <Row gutter={16}>
                  <Col span={12}>
                    <Form.Item label={t('infer.vocal_gain')} name="vocalGain">
                      <Slider min={-6} max={12} step={1} marks={{ 0: '0dB', 3: '+3', 6: '+6', 12: '+12' }} />
                    </Form.Item>
                  </Col>
                </Row>
                <Button type="primary" loading={s.mergeLoading} onClick={s.handleMerge} block icon={<MergeOutlined />}>
                  {t('infer.merge')}
                </Button>
                {s.mergeOutputPath && (
                  <Alert style={{ marginTop: 16 }} type="success" showIcon message={t('infer.merge_done')} description={<code style={{ fontSize: 12 }}>{s.mergeOutputPath}</code>} />
                )}
              </Form>
            ),
          },
        ]}
      />

      <OutputLibrary refreshToken={`${s.outputUrl}|${s.mergeOutputPath}`} />
    </div>
  )
}
