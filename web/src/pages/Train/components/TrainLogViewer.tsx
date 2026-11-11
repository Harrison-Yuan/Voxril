import { Card, Space, Typography, Badge, Alert } from 'antd'
import LogViewer from '../../../components/LogViewer'
import type { StreamStatus } from '../hooks/useTrainProgress'

const { Text } = Typography

interface TrainLogViewerProps {
  log: string
  status: 'wait' | 'process' | 'finish' | 'error'
  logStreamStatus: StreamStatus
  running: boolean
}

export const TrainLogViewer = ({ log, status, logStreamStatus, running }: TrainLogViewerProps) => {
  if (!log) return null

  const getStreamStatusText = () => {
    const hints = {
      connecting: '日志流连接中，等待首条输出...',
      reconnecting: '日志流中断，正在自动重连...',
      connected: '日志流已连接',
      idle: '日志流未连接',
    }
    return hints[logStreamStatus]
  }

  return (
    <Card
      size="small"
      title={
        <Space>
          <Text>运行日志</Text>
          {running && (
            <Badge
              status="processing"
              text={<Text type="secondary" style={{ fontSize: 12 }}>{getStreamStatusText()}</Text>}
            />
          )}
        </Space>
      }
    >
      <LogViewer content={log} height={280} />
      {status === 'error' && (
        <Alert type="error" message="执行失败，请查看日志" style={{ marginTop: 8 }} />
      )}
      {status === 'finish' && (
        <Alert type="success" message="执行成功" style={{ marginTop: 8 }} />
      )}
    </Card>
  )
}
