import { Card, Form, Input, Select, Slider, Button, Space, Typography, Row, Col, Alert, Tag, AutoComplete } from 'antd'
import { DatabaseOutlined } from '@ant-design/icons'
import type { WorkflowStage } from '@/store/trainStore'
import type { ExperimentConfig } from '../hooks/useExperimentConfig'
import type { RunUsage } from '../services/trainApi'
import { formatBytes } from '@/lib/formatBytes'
import { useI18n } from '@/i18n'

const { Text } = Typography

interface PreprocessStepProps {
  config: ExperimentConfig
  status: WorkflowStage
  running: boolean
  canRun: boolean
  experimentOptions: string[]
  usage?: RunUsage | null
  hasStartedWorkflow: boolean
  onConfigChange: (values: Partial<ExperimentConfig>) => void
  onExpDirChange: (expDir: string) => void
  onSelectDirectory: () => void
  onRun: () => void
}

/** 阶段状态 → 文案键，渲染时再经 t() 取词，切换语言才会刷新 */
const statusTextKeyMap: Record<WorkflowStage, string> = {
  pending: 'common.pending',
  running: 'common.running',
  done: 'common.done',
  error: 'common.failed',
  stale: 'trainStep.status_stale',
  reclaimed: 'trainStep.status_reclaimed',
}

const statusColorMap: Record<WorkflowStage, string> = {
  pending: 'default',
  running: 'processing',
  done: 'success',
  error: 'error',
  stale: 'warning',
  reclaimed: 'default',
}

export const PreprocessStep = ({
  config,
  status,
  running,
  canRun,
  experimentOptions: _experimentOptions,
  usage,
  hasStartedWorkflow,
  onConfigChange,
  onExpDirChange,
  onSelectDirectory,
  onRun,
}: PreprocessStepProps) => {
  const { t } = useI18n()
  return (
    <Card
      title={
        <Space>
          <Text>{t('trainStep.step_title', { n: 1, stage: t('train.preprocess') })}</Text>
          <Tag color={statusColorMap[status]}>{t(statusTextKeyMap[status])}</Tag>
        </Space>
      }
      size="small"
      extra={
        <Button type="primary" onClick={onRun} loading={running} disabled={!canRun} icon={<DatabaseOutlined />}>
          {status === 'done' ? t('trainStep.rerun_preprocess') : t('trainStep.start_preprocess')}
        </Button>
      }
    >
      {!config.trainsetDir && <Alert type="warning" showIcon style={{ marginBottom: 16 }} message={t('trainStep.select_trainset_required')} description={t('trainStep.select_trainset_desc')} />}

      {config.trainsetDir && (
        <Alert type="success" showIcon style={{ marginBottom: 16 }} message={t('trainStep.trainset_configured')} description={t('trainStep.current_path', { path: config.trainsetDir })} />
      )}

      <Row gutter={16}>
        <Col span={12}>
          <Form.Item label={t('train.exp_name')} required extra={hasStartedWorkflow ? t('trainStep.exp_switch_extra') : t('trainStep.exp_input_extra')}>
            <AutoComplete
              value={config.expDir}
              onChange={(value) => {
                onConfigChange({ expDir: value })
              }}
              onSelect={(value) => {
                onExpDirChange(value)
              }}
              options={_experimentOptions.map((name) => ({ value: name }))}
              placeholder={t('trainStep.exp_placeholder')}
              disabled={running}
              filterOption={(inputValue, option) =>
                String(option?.value ?? '')
                  .toUpperCase()
                  .includes(String(inputValue).toUpperCase())
              }
            >
              <Input />
            </AutoComplete>
          </Form.Item>
        </Col>

        <Col span={12}>
          <Form.Item label={t('train.model_dir')} required extra={t('trainStep.trainset_extra')} validateStatus={!config.trainsetDir ? 'warning' : 'success'}>
            <Input
              value={config.trainsetDir}
              onChange={(e) => onConfigChange({ trainsetDir: e.target.value })}
              disabled={running}
              placeholder={t('trainStep.trainset_placeholder')}
              status={!config.trainsetDir ? 'warning' : undefined}
              addonAfter={
                <Button type="link" size="small" disabled={running} onClick={onSelectDirectory}>
                  {t('trainStep.select_folder')}
                </Button>
              }
            />
          </Form.Item>
        </Col>

        <Col span={12}>
          <Form.Item label={t('train.sr')} extra={t('trainStep.sr_extra')}>
            <Select
              value={config.sr}
              onChange={(value) => onConfigChange({ sr: value })}
              disabled={running}
              options={[
                { value: '32k', label: t('trainStep.sr_32k') },
                { value: '40k', label: t('trainStep.sr_40k') },
                { value: '48k', label: t('trainStep.sr_48k') },
              ]}
            />
          </Form.Item>
        </Col>

        <Col span={12}>
          <Form.Item label={t('train.f0_method')} extra={t('trainStep.f0_method_extra')}>
            <Select
              value={config.f0Method}
              onChange={(value) => onConfigChange({ f0Method: value })}
              disabled={running}
              options={[
                { value: 'rmvpe', label: t('trainStep.f0_rmvpe') },
                { value: 'harvest', label: t('trainStep.f0_harvest') },
                { value: 'pm', label: t('trainStep.f0_pm') },
              ]}
            />
          </Form.Item>
        </Col>

        <Col span={12}>
          <Form.Item label={t('trainStep.np_process_label', { n: config.npProcess })} extra={t('trainStep.np_process_extra')}>
            <Slider value={config.npProcess} onChange={(value) => onConfigChange({ npProcess: value })} min={1} max={8} step={1} disabled={running} />
          </Form.Item>
        </Col>
      </Row>

      {usage && (
        <Text type="secondary" style={{ fontSize: 12 }}>
          {t('trainStep.step_output', { size: formatBytes(usage.preprocess) })}
        </Text>
      )}
    </Card>
  )
}
