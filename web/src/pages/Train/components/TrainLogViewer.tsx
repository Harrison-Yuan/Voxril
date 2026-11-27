import { Card, Space, Typography, Badge, Alert } from 'antd'
import LogViewer from '../../../components/LogViewer'
import type { StreamStatus } from '../hooks/useTrainProgress'
import { useI18n } from '@/i18n'

const { Text } = Typography

interface TrainLogViewerProps {
  log: string
  status: 'wait' | 'process' | 'finish' | 'error'
  logStreamStatus: StreamStatus
  running: boolean
}

export const TrainLogViewer = ({ log, status, logStreamStatus, running }: TrainLogViewerProps) => {
  const { t } = useI18n()
  if (!log) return null

  const getStreamStatusText = () => {
    const hints = {
      connecting: t('trainStep.log_stream_connecting'),
      reconnecting: t('trainStep.log_stream_reconnecting'),
      connected: t('trainStep.log_stream_connected'),
      idle: t('trainStep.log_stream_idle'),
    }
    return hints[logStreamStatus]
  }

  return (
    <Card
      size="small"
      title={
        <Space>
          <Text>{t('trainStep.log_title')}</Text>
          {running && (
            <Badge
              status="processing"
              text={
                <Text type="secondary" style={{ fontSize: 12 }}>
                  {getStreamStatusText()}
                </Text>
              }
            />
          )}
        </Space>
      }
    >
      <LogViewer content={log} height={280} />
      {status === 'error' && <Alert type="error" message={t('trainStep.exec_failed')} style={{ marginTop: 8 }} />}
      {status === 'finish' && <Alert type="success" message={t('status.success')} style={{ marginTop: 8 }} />}
    </Card>
  )
}
