import { useEffect } from 'react'
import { Form, Select, Slider, Button, Progress, Row, Col, Alert, message, Input, Collapse, Modal, List } from 'antd'
import { UploadOutlined, SoundOutlined, MergeOutlined, FolderOpenOutlined, StopOutlined } from '@ant-design/icons'
import { open } from '@tauri-apps/plugin-dialog'
import LogViewer from '../../components/LogViewer'
import { useInferState } from './useInferState'
import { useInferStore } from '@/store/inferStore'

function SectionHeader({ children }: { children: React.ReactNode }) {
  return (
    <div style={{ padding: '12px 20px', borderBottom: '1px solid var(--color-border-subtle)', fontSize: 13, fontWeight: 600 }}>
      {children}
    </div>
  )
}

const lbl = { fontSize: 11, fontWeight: 600, color: 'var(--color-text-tertiary)', textTransform: 'uppercase' as const, letterSpacing: '0.06em', marginBottom: 8 }

export default function InferPage() {
  const s = useInferState()
  const storeCancel = useInferStore(x => x.cancel)

  useEffect(() => {
    if (!s.instrumentalFiles.length) return
    Modal.info({
      title: '选择伴奏文件', width: 600,
      content: (
        <List dataSource={s.instrumentalFiles} renderItem={(f: { name: string; path: string }) => (
          <List.Item style={{ cursor: 'pointer' }} onClick={() => {
            s.mergeForm.setFieldValue('instrumentalPath', f.path)
            message.success(`已选择：${f.name}`)
            s.setInstrumentalFiles([])
            Modal.destroyAll()
          }}>{f.name}</List.Item>
        )} />
      ),
      onCancel: () => s.setInstrumentalFiles([]),
    })
  }, [s.instrumentalFiles])

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 20 }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 12, marginBottom: 4 }}>
        <div className="icon-badge icon-badge--md" style={{ borderRadius: 10 }}><SoundOutlined /></div>
        <div>
          <h1 className="page-header__title">语音转换</h1>
          <p className="page-header__subtitle">选择已训练的声音模型，将音频转换成目标音色</p>
        </div>
      </div>

      <Row gutter={16}>
        <Col xs={24} lg={15}>
          <div className="ant-card" style={{ padding: 0 }}>
            <SectionHeader>
              <span style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                <SoundOutlined style={{ color: 'var(--color-primary-hover)' }} />音频转换
              </span>
            </SectionHeader>
            <div style={{ padding: 20 }}>
              <Form form={s.form} layout="vertical" initialValues={{ modelName: '', transpose: 0, f0Method: 'rmvpe', indexRate: 0.75, filterRadius: 3, resampleSr: 0, rmsMixRate: 1.0, protect: 0.5 }} onValuesChange={(_, all) => s.setFv(all)}>
                <Row gutter={16}>
                  <Col span={12}>
                    <Form.Item label="声音模型" name="modelName" rules={[{ required: true, message: '请选择一个模型' }]}>
                      <Select placeholder={s.models.length === 0 ? '暂无可用模型，请先训练' : '选择已训练的模型'}
                        options={s.models.map(m => {
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
                        notFoundContent="暂无模型" />
                    </Form.Item>
                  </Col>
                  <Col span={12}>
                    <Form.Item label="音高提取算法" name="f0Method">
                      <Select options={[{ value: 'rmvpe', label: 'RMVPE（推荐，ONNX）' }, { value: 'harvest', label: 'Harvest（低音好）' }, { value: 'pm', label: 'PM（最快）' }]} />
                    </Form.Item>
                  </Col>
                </Row>
                <Form.Item label="音频文件">
                  <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
                    <Button icon={<UploadOutlined />} onClick={s.selectSource} block>
                      {s.sourcePath ? '更换音频' : '选择音频文件'}
                    </Button>
                    {s.sourcePath && <Alert type="info" showIcon message={s.sourceName || s.sourcePath} style={{ fontSize: 12 }} />}
                  </div>
                </Form.Item>
                <Form.Item label={`变调（半音）：${s.fv.transpose ?? 0}`} name="transpose">
                  <Slider min={-24} max={24} step={1} marks={{ '-12': '-12', 0: '0', 12: '+12' }} />
                </Form.Item>
                <Form.Item label={`检索特征比例：${s.fv.indexRate ?? 0.75}`} name="indexRate">
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
                      {s.loading ? '转换中...' : '开始转换'}
                    </Button>
                  </Col>
                  {s.loading && (
                    <Col>
                      <Button danger size="large" icon={<StopOutlined />} onClick={storeCancel} style={{ height: 44, fontSize: 14 }}>
                        取消
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
            <SectionHeader>参数建议</SectionHeader>
            <div style={{ padding: 20, display: 'flex', flexDirection: 'column', gap: 20 }}>
              <div>
                <div style={lbl}>变调建议</div>
                {[{ label: '女→男', desc: '变调 -12，算法选 RMVPE' }, { label: '男→女', desc: '变调 +12，算法选 RMVPE' }, { label: '同性别', desc: '变调保持 0' }].map((item, i) => (
                  <div key={i} style={{ display: 'flex', alignItems: 'flex-start', gap: 8, fontSize: 12, marginBottom: 5 }}>
                    <code style={{ background: 'var(--color-bg-hover)', padding: '1px 6px', borderRadius: 4, fontSize: 11, color: 'var(--color-primary-hover)', flexShrink: 0 }}>{item.label}</code>
                    <span style={{ color: 'var(--color-text-secondary)', lineHeight: 1.5 }}>{item.desc}</span>
                  </div>
                ))}
              </div>
              <div>
                <div style={lbl}>工作流程</div>
                {['先用 10~30 秒片段测试参数', '满意后再转换完整歌曲', '转换结果可在下方合成伴奏'].map((desc, i) => (
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
          <SectionHeader>转换进度</SectionHeader>
          <div style={{ padding: 20, display: 'flex', flexDirection: 'column', gap: 12 }}>
            <div>
              <Progress percent={s.pct} status={s.pct === 100 ? 'success' : 'active'} strokeColor={{ '0%': '#7c3aed', '100%': '#a78bfa' }} trailColor="var(--color-bg-hover)" />
              <div style={{ fontSize: 12, color: 'var(--color-text-tertiary)', marginTop: 4 }}>
                {s.tot > 1 ? `音频分段 ${s.cur}/${s.tot}` : '处理中...'}
              </div>
            </div>
            <LogViewer content={s.log} height={120} />
          </div>
        </div>
      )}

      {!s.loading && s.log && (
        <div className="ant-card animate-fade-in" style={{ padding: 0 }}>
          <SectionHeader>转换日志</SectionHeader>
          <div style={{ padding: 20 }}><LogViewer content={s.log} height={120} /></div>
        </div>
      )}

      {s.outputUrl && (
        <Alert type="success" showIcon message="转换完成"
          description={
            <div style={{ marginTop: 4, display: 'flex', flexDirection: 'column', gap: 2 }}>
              <span>输出文件：<code style={{ fontSize: 12 }}>{s.outputUrl.split('/').pop()}</code></span>
              <span style={{ fontSize: 11, color: 'var(--color-text-tertiary)' }}>{s.outputUrl}</span>
            </div>
          }
        />
      )}

      <Collapse bordered={false} style={{ background: 'transparent' }} items={[{
        key: 'merge',
        label: <span style={{ display: 'flex', alignItems: 'center', gap: 8, fontSize: 13, fontWeight: 500 }}><MergeOutlined style={{ color: 'var(--color-primary-hover)' }} />合成伴奏</span>,
        children: (
          <Form form={s.mergeForm} layout="vertical" initialValues={{ vocalGain: 6 }}>
            <Row gutter={16}>
              <Col span={12}>
                <Form.Item label="新的人声音频" name="vocalPath">
                  <Input readOnly placeholder="转换后自动填充，也可手动选择"
                    addonAfter={<Button type="link" size="small" onClick={() => s.selectMerge('vocalPath', '选择人声音频')}>浏览</Button>}
                  />
                </Form.Item>
              </Col>
              <Col span={12}>
                <Form.Item label="伴奏音频" name="instrumentalPath">
                  <Input readOnly placeholder="自动检测伴奏，也可手动切换"
                    addonAfter={
                      <span style={{ display: 'flex' }}>
                        {s.workspaceRoot && <Button type="link" size="small" onClick={s.quickInstrumental}>快速</Button>}
                        <Button type="link" size="small" onClick={() => s.selectMerge('instrumentalPath', '选择伴奏音频')}>浏览</Button>
                      </span>
                    }
                  />
                </Form.Item>
              </Col>
            </Row>
            <Row gutter={16}>
              <Col span={12}>
                <Form.Item label="输出文件夹" name="outputDir">
                  <Input placeholder={s.workspaceRoot ? `默认：${s.workspaceRoot}/mixed/` : '选择输出目录'}
                    addonAfter={
                      <Button type="link" size="small" icon={<FolderOpenOutlined />} onClick={async () => {
                        try {
                          const sel = await open({ directory: true, multiple: false, title: '选择合成结果输出文件夹' })
                          if (sel) s.mergeForm.setFieldValue('outputDir', sel as string)
                        } catch (e) { message.error(`选择输出文件夹失败：${String(e)}`) }
                      }}>浏览</Button>
                    }
                  />
                </Form.Item>
              </Col>
              <Col span={12}>
                <Form.Item label="人声音量 (dB)" name="vocalGain">
                  <Slider min={-6} max={12} step={1} marks={{ 0: '0dB', 3: '+3', 6: '+6', 12: '+12' }} />
                </Form.Item>
              </Col>
            </Row>
            <Button type="primary" loading={s.mergeLoading} onClick={s.handleMerge} block icon={<MergeOutlined />}>合成</Button>
            {s.mergeOutputPath && (
              <Alert style={{ marginTop: 16 }} type="success" showIcon message="合成完成"
                description={<code style={{ fontSize: 12 }}>{s.mergeOutputPath}</code>}
              />
            )}
          </Form>
        ),
      }]} />
    </div>
  )
}
