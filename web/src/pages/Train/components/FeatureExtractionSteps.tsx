import { Card, Button, Space, Typography, Tag, Alert, Progress } from 'antd'
import { SoundOutlined, ApartmentOutlined } from '@ant-design/icons'
import type { WorkflowStage } from '@/store/trainStore'
import type { RunUsage } from '../services/trainApi'
import { formatBytes } from '@/lib/formatBytes'
import { useI18n } from '@/i18n'

const { Text } = Typography

interface FeatureExtractionStepsProps {
  preprocessStatus: WorkflowStage
  f0Status: WorkflowStage
  audioStatus: WorkflowStage
  ifF0: boolean
  running: boolean
  currentExtracting: 'none' | 'f0' | 'audio'
  usage?: RunUsage | null
  f0Percent?: number
  audioPercent?: number
  onExtractF0: () => void
  onExtractAudio: () => void
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

export const FeatureExtractionSteps = ({
  preprocessStatus,
  f0Status,
  audioStatus,
  ifF0,
  running,
  currentExtracting,
  usage,
  f0Percent,
  audioPercent,
  onExtractF0,
  onExtractAudio,
}: FeatureExtractionStepsProps) => {
  const { t } = useI18n()
  const preprocessDone = preprocessStatus === 'done'
  const f0Done = !ifF0 || f0Status === 'done'
  const f0DoneText = f0Status === 'done' || f0Status === 'reclaimed'
  const audioDoneText = audioStatus === 'done' || audioStatus === 'reclaimed'

  return (
    <Card
      title={
        <Space>
          <Text>{t('trainStep.step_title', { n: 2, stage: t('trainStep.feature_extract') })}</Text>
          {preprocessDone && <Tag color={f0Done && audioStatus === 'done' ? 'success' : 'processing'}>{f0Done && audioStatus === 'done' ? t('common.done') : t('common.running')}</Tag>}
          {!preprocessDone && <Tag>{t('trainStep.wait_preprocess')}</Tag>}
        </Space>
      }
      size="small"
    >
      {!preprocessDone && <Alert type="warning" showIcon style={{ marginBottom: 16 }} message={t('trainStep.step_required', { n: 1, stage: t('train.preprocess') })} />}

      {/* F0 Extraction */}
      {ifF0 && (
        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
            padding: '12px 0',
            borderBottom: '1px solid var(--color-border-subtle)',
          }}
        >
          <Space>
            <SoundOutlined style={{ color: f0Status === 'done' ? 'var(--color-success)' : 'var(--color-text-tertiary)' }} />
            <div>
              <Text strong>{t('train.f0_extract')}</Text>
              <Tag color={statusColorMap[f0Status]} style={{ marginLeft: 8 }}>
                {t(statusTextKeyMap[f0Status])}
              </Tag>
              <br />
              <Text type="secondary" style={{ fontSize: 12 }}>
                {t('trainStep.f0_desc')}
              </Text>
              {f0Status === 'running' && f0Percent !== undefined && f0Percent > 0 && <Progress percent={f0Percent} size="small" style={{ width: 200, marginTop: 4 }} />}
              {usage && (
                <>
                  <br />
                  <Text type="secondary" style={{ fontSize: 12 }}>
                    {t('trainStep.step_output', { size: formatBytes(usage.f0) })}
                  </Text>
                </>
              )}
            </div>
          </Space>
          <Button type="primary" onClick={onExtractF0} loading={currentExtracting === 'f0'} disabled={!preprocessDone || running}>
            {f0DoneText ? t('trainStep.re_extract') : f0Status === 'error' ? t('trainStep.retry_f0_extract') : t('trainStep.start_f0_extract')}
          </Button>
        </div>
      )}

      {/* Audio Feature Extraction */}
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          padding: '12px 0',
        }}
      >
        <Space>
          <ApartmentOutlined style={{ color: audioStatus === 'done' ? 'var(--color-success)' : 'var(--color-text-tertiary)' }} />
          <div>
            <Text strong>{t('train.audio_extract')}</Text>
            <Tag color={statusColorMap[audioStatus]} style={{ marginLeft: 8 }}>
              {t(statusTextKeyMap[audioStatus])}
            </Tag>
            <br />
            <Text type="secondary" style={{ fontSize: 12 }}>
              {t('trainStep.audio_desc')}
            </Text>
            {audioStatus === 'running' && audioPercent !== undefined && audioPercent > 0 && <Progress percent={audioPercent} size="small" style={{ width: 200, marginTop: 4 }} />}
            {usage && (
              <>
                <br />
                <Text type="secondary" style={{ fontSize: 12 }}>
                  {t('trainStep.step_output', { size: formatBytes(usage.feature) })}
                </Text>
              </>
            )}
          </div>
        </Space>
        <Button type="primary" onClick={onExtractAudio} loading={currentExtracting === 'audio'} disabled={!preprocessDone || !f0Done || running}>
          {audioDoneText ? t('trainStep.re_extract') : audioStatus === 'error' ? t('trainStep.retry_audio_extract') : t('trainStep.start_audio_extract')}
        </Button>
      </div>

      {f0Done && audioStatus === 'done' && <Alert type="success" showIcon style={{ marginTop: 12 }} message={t('trainStep.features_done_msg')} />}
    </Card>
  )
}
