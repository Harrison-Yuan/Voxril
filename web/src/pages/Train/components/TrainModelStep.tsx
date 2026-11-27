import { Card, Slider, Switch, Button, Space, Typography, Row, Col, Alert, Tag, Divider, Collapse } from 'antd'
import { DeleteOutlined, ThunderboltOutlined, ApartmentOutlined } from '@ant-design/icons'
import type { WorkflowStage } from '@/store/trainStore'
import type { ExperimentConfig } from '../hooks/useExperimentConfig'
import type { RunUsage } from '../services/trainApi'
import { formatBytes } from '@/lib/formatBytes'
import { useI18n } from '@/i18n'

const { Text } = Typography

interface TrainModelStepProps {
  config: ExperimentConfig
  trainStatus: WorkflowStage
  featuresStatus: WorkflowStage
  indexStatus: WorkflowStage
  trainEpoch: number
  totalEpoch: number
  running: boolean
  canRun: boolean
  usage?: RunUsage | null
  onConfigChange: (values: Partial<ExperimentConfig>) => void
  onRun: () => void
  onStop: () => void
  onBuildIndex: () => void
  onClearTraining: () => void
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

export const TrainModelStep = ({
  config,
  trainStatus,
  featuresStatus,
  indexStatus,
  trainEpoch,
  totalEpoch,
  running,
  canRun,
  usage,
  onConfigChange,
  onRun,
  onStop,
  onBuildIndex,
  onClearTraining,
}: TrainModelStepProps) => {
  const { t } = useI18n()
  const getButtonText = () => {
    if (trainEpoch > 0 && trainEpoch < totalEpoch) return t('train.retrain')
    if (trainStatus === 'done') return t('trainStep.rerun_train')
    return t('train.start')
  }

  const getIndexButtonText = () => {
    if (indexStatus === 'running') return t('trainStep.index_building')
    if (indexStatus === 'done') return t('train.rebuild_index')
    return t('train.build_index')
  }

  return (
    <Card
      title={
        <Space>
          <Text>{t('trainStep.step_title', { n: 3, stage: t('train.train_model') })}</Text>
          <Tag color={statusColorMap[trainStatus]}>{t(statusTextKeyMap[trainStatus])}</Tag>
        </Space>
      }
      size="small"
      extra={
        <Space>
          {(trainEpoch > 0 || trainStatus === 'done') && (
            <Button danger onClick={onClearTraining} icon={<DeleteOutlined />}>
              {t('trainStep.reset_batch')}
            </Button>
          )}
          {trainStatus === 'running' && (
            <Button danger onClick={onStop}>
              {t('common.stop')}
            </Button>
          )}
          <Button type="primary" onClick={onRun} loading={running} disabled={!canRun} icon={<ThunderboltOutlined />}>
            {getButtonText()}
          </Button>
        </Space>
      }
    >
      <Alert
        type={featuresStatus === 'done' ? 'success' : 'warning'}
        showIcon
        style={{ marginBottom: 16 }}
        message={featuresStatus === 'done' ? t('trainStep.features_done_msg') : t('trainStep.step_required', { n: 2, stage: t('trainStep.feature_extract') })}
      />

      <Row gutter={16}>
        <Col span={12}>
          <div style={{ marginBottom: 24 }}>
            <div style={{ marginBottom: 8 }}>
              <Text>{t('trainStep.total_epochs_value', { n: config.totalEpoch })}</Text>
            </div>
            <Slider value={config.totalEpoch} onChange={(value) => onConfigChange({ totalEpoch: value })} min={10} max={1000} step={10} disabled={running} />
          </div>
        </Col>

        <Col span={12}>
          <div style={{ marginBottom: 24 }}>
            <div style={{ marginBottom: 8 }}>
              <Text>{t('trainStep.save_every_value', { n: config.saveEpoch })}</Text>
            </div>
            <Slider value={config.saveEpoch} onChange={(value) => onConfigChange({ saveEpoch: value })} min={5} max={100} step={5} disabled={running} />
          </div>
        </Col>
      </Row>

      {usage && (
        <Text type="secondary" style={{ fontSize: 12 }}>
          {t('trainStep.step_output', { size: formatBytes(usage.train) })}
        </Text>
      )}

      <Collapse ghost style={{ marginTop: 8 }}>
        <Collapse.Panel header={t('trainStep.advanced_settings')} key="advanced">
          <Row gutter={[24, 16]}>
            <Col span={12}>
              <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
                <Text>{t('trainStep.save_latest_only')}</Text>
                <Switch checked={config.ifSaveLatest} onChange={(checked) => onConfigChange({ ifSaveLatest: checked })} disabled={featuresStatus !== 'done' || running} />
              </div>
            </Col>
            <Col span={12}>
              <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
                <Text>{t('trainStep.save_every_weights')}</Text>
                <Switch checked={config.ifSaveEveryWeights} onChange={(checked) => onConfigChange({ ifSaveEveryWeights: checked })} disabled={featuresStatus !== 'done' || running} />
              </div>
            </Col>
          </Row>
        </Collapse.Panel>
      </Collapse>
      <Divider style={{ margin: '12px 0' }} />
      <Row justify="space-between" align="middle">
        <Col>
          <Space>
            <ApartmentOutlined />
            <Text strong>{t('trainStep.step_title', { n: 4, stage: t('train.build_index') })}</Text>
            <Tag color={statusColorMap[indexStatus]}>{t(statusTextKeyMap[indexStatus])}</Tag>
          </Space>
          <br />
          <Text type="secondary" style={{ fontSize: 12 }}>
            {t('trainStep.index_tip_manual', { tip: t('train.index_tip') })}
          </Text>
          {usage && (
            <>
              <br />
              <Text type="secondary" style={{ fontSize: 12 }}>
                {t('trainStep.step_output', { size: formatBytes(usage.index) })}
              </Text>
            </>
          )}
        </Col>
        <Col>
          <Button icon={<ApartmentOutlined />} onClick={onBuildIndex} loading={indexStatus === 'running'} disabled={featuresStatus !== 'done' || running || indexStatus === 'running'}>
            {getIndexButtonText()}
          </Button>
        </Col>
      </Row>
    </Card>
  )
}
