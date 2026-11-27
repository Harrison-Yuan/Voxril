import { Card, Row, Col, Progress, Statistic, Typography } from 'antd'
import type { TrainProgressState, StreamStatus } from '../hooks/useTrainProgress'
import type { WorkflowStage } from '@/store/trainStore'
import { useI18n } from '@/i18n'

const { Text } = Typography

interface TrainProgressProps {
  trainProgress: TrainProgressState
  trainStatus: WorkflowStage
  indexStatus: WorkflowStage
  trainEpoch: number
  totalEpoch: number
  checkpointPath: string
  trainProgressStreamStatus: StreamStatus
}

export const TrainProgress = ({ trainProgress, trainStatus, indexStatus, trainEpoch, totalEpoch, checkpointPath, trainProgressStreamStatus }: TrainProgressProps) => {
  const { t } = useI18n()
  const getProgressStatus = () => {
    if (indexStatus === 'running') return 'active'
    if (trainStatus === 'error' || indexStatus === 'error') return 'exception'
    if (indexStatus === 'done') return 'success'
    if (trainStatus === 'done') return 'active'
    return 'active'
  }

  const formatTime = (seconds: number) => {
    if (seconds <= 0) return ''
    const hours = Math.floor(seconds / 3600)
    const minutes = Math.floor((seconds % 3600) / 60)
    if (hours > 0) {
      return t('trainStep.eta_hm', { hours, minutes })
    }
    return t('trainStep.eta_m', { minutes })
  }

  const getProgressText = () => {
    if (indexStatus === 'running') return t('trainStep.index_building')
    if (indexStatus === 'done') return t('trainStep.flow_done')

    if (trainStatus === 'running') {
      const parts: string[] = []

      if (trainProgress.latestLoss) {
        parts.push(trainProgress.latestLoss)
      }

      if (trainProgress.estimatedSeconds && trainProgress.estimatedSeconds > 0) {
        const timeStr = formatTime(trainProgress.estimatedSeconds)
        if (timeStr) {
          parts.push(`${t('train.eta')}: ${timeStr}`)
        }
      }

      if (trainProgress.avgEpochTime && trainProgress.avgEpochTime > 0) {
        const avgMinutes = Math.floor(trainProgress.avgEpochTime / 60)
        const avgSeconds = trainProgress.avgEpochTime % 60
        parts.push(t('trainStep.avg_epoch_time', { minutes: avgMinutes, seconds: avgSeconds }))
      }

      if (parts.length > 0) {
        return parts.join(' | ')
      }

      const hints = {
        connecting: t('trainStep.progress_stream_connecting'),
        reconnecting: t('trainStep.progress_stream_reconnecting'),
        connected: t('trainStep.progress_stream_connected'),
        idle: t('trainStep.progress_stream_idle'),
      }
      return hints[trainProgressStreamStatus]
    }

    if (trainStatus === 'done') return t('trainStep.train_done_index_pending')
    return t('trainStep.wait_train_start')
  }

  const getHintText = () => {
    if (indexStatus === 'running' || indexStatus === 'done') return null

    if (trainEpoch > 0) {
      if (trainStatus === 'running') {
        return t('trainStep.hint_running')
      }
      if (trainStatus === 'done') {
        return t('trainStep.hint_train_done')
      }
      return t('trainStep.hint_existing_artifacts')
    }

    return null
  }

  const currentPercent = Math.min(100, trainProgress.currentPercent || (trainEpoch / Math.max(totalEpoch, 1)) * 100 || 0)

  return (
    <Card size="small" title={t('trainStep.progress_title')}>
      <Row gutter={16} align="middle">
        <Col span={8}>
          <Statistic title={t('train.current_epoch')} value={`${trainProgress.currentEpoch || trainEpoch || 0} / ${totalEpoch}`} />
          {checkpointPath && (
            <Text type="secondary" style={{ fontSize: 12, display: 'block', marginTop: 8 }}>
              {t('trainStep.checkpoint', { name: checkpointPath.split('/').pop() || checkpointPath })}
            </Text>
          )}
        </Col>

        <Col span={16}>
          <Progress percent={Number(currentPercent.toFixed(1))} status={getProgressStatus()} />

          <Text type="secondary" style={{ fontSize: 12 }}>
            {getProgressText()}
          </Text>

          {getHintText() && (
            <Text type="secondary" style={{ fontSize: 12, display: 'block', marginTop: 4 }}>
              {getHintText()}
            </Text>
          )}
        </Col>
      </Row>
    </Card>
  )
}
