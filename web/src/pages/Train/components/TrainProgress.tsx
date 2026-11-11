import { Card, Row, Col, Progress, Statistic, Typography } from 'antd'
import type { TrainProgressState, StreamStatus } from '../hooks/useTrainProgress'
import type { WorkflowStage } from '../hooks/useTrainWorkflow'

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

export const TrainProgress = ({
  trainProgress,
  trainStatus,
  indexStatus,
  trainEpoch,
  totalEpoch,
  checkpointPath,
  trainProgressStreamStatus,
}: TrainProgressProps) => {
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
      return `约 ${hours} 小时 ${minutes} 分钟`
    }
    return `约 ${minutes} 分钟`
  }

  const getProgressText = () => {
    if (indexStatus === 'running') return '正在训练特征索引...'
    if (indexStatus === 'done') return '训练流程全部完成！'
    
    if (trainStatus === 'running') {
      const parts: string[] = []
      
      if (trainProgress.latestLoss) {
        parts.push(trainProgress.latestLoss)
      }
      
      if (trainProgress.estimatedSeconds && trainProgress.estimatedSeconds > 0) {
        const timeStr = formatTime(trainProgress.estimatedSeconds)
        if (timeStr) {
          parts.push(`预计剩余时间: ${timeStr}`)
        }
      }
      
      if (trainProgress.avgEpochTime && trainProgress.avgEpochTime > 0) {
        const avgMinutes = Math.floor(trainProgress.avgEpochTime / 60)
        const avgSeconds = trainProgress.avgEpochTime % 60
        parts.push(`平均每轮: ${avgMinutes}分${avgSeconds}秒`)
      }
      
      if (parts.length > 0) {
        return parts.join(' | ')
      }
      
      const hints = {
        connecting: '进度流连接中，等待训练进度...',
        reconnecting: '进度流中断，正在自动重连...',
        connected: '进度流已连接',
        idle: '进度流未连接',
      }
      return hints[trainProgressStreamStatus]
    }

    if (trainStatus === 'done') return '模型训练完成，准备训练索引'
    return '等待训练开始'
  }

  const getHintText = () => {
    if (indexStatus === 'running' || indexStatus === 'done') return null

    if (trainEpoch > 0) {
      if (trainStatus === 'running') {
        return '训练完成后将自动训练特征索引。'
      }
      if (trainStatus === 'done') {
        return '训练已完成，可以训练特征索引。'
      }
      return '已有训练产物，可以继续训练或训练索引。'
    }

    return null
  }

  const currentPercent = Math.min(
    100,
    trainProgress.currentPercent || (trainEpoch / Math.max(totalEpoch, 1)) * 100 || 0
  )

  return (
    <Card size="small" title="训练进度">
      <Row gutter={16} align="middle">
        <Col span={8}>
          <Statistic
            title="当前轮次"
            value={`${trainProgress.currentEpoch || trainEpoch || 0} / ${totalEpoch}`}
          />
          {checkpointPath && (
            <Text type="secondary" style={{ fontSize: 12, display: 'block', marginTop: 8 }}>
              检查点: {checkpointPath.split('/').pop() || checkpointPath}
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
