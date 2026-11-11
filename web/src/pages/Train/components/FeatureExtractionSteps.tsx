import { Card, Button, Space, Typography, Tag, Alert, Progress } from 'antd'
import { SoundOutlined, ApartmentOutlined } from '@ant-design/icons'
import type { WorkflowStage } from '../hooks/useTrainWorkflow'

const { Text } = Typography

interface FeatureExtractionStepsProps {
  preprocessStatus: WorkflowStage
  f0Status: WorkflowStage
  audioStatus: WorkflowStage
  ifF0: boolean
  running: boolean
  currentExtracting: 'none' | 'f0' | 'audio'
  f0Percent?: number
  audioPercent?: number
  onExtractF0: () => void
  onExtractAudio: () => void
}

const statusTextMap: Record<WorkflowStage, string> = {
  pending: '未开始',
  running: '进行中',
  done: '已完成',
  error: '失败',
}

const statusColorMap: Record<WorkflowStage, string> = {
  pending: 'default',
  running: 'processing',
  done: 'success',
  error: 'error',
}

export const FeatureExtractionSteps = ({
  preprocessStatus,
  f0Status,
  audioStatus,
  ifF0,
  running,
  currentExtracting,
  f0Percent,
  audioPercent,
  onExtractF0,
  onExtractAudio,
}: FeatureExtractionStepsProps) => {
  const preprocessDone = preprocessStatus === 'done'
  const f0Done = !ifF0 || f0Status === 'done'

  return (
    <Card
      title={
        <Space>
          <Text>步骤 2：特征提取</Text>
          {preprocessDone && (
            <Tag color={f0Done && audioStatus === 'done' ? 'success' : 'processing'}>
              {f0Done && audioStatus === 'done' ? '已完成' : '进行中'}
            </Tag>
          )}
          {!preprocessDone && <Tag>等待预处理</Tag>}
        </Space>
      }
      size="small"
    >
      {!preprocessDone && (
        <Alert type="warning" showIcon style={{ marginBottom: 16 }}
          message="请先完成步骤 1：数据预处理"
        />
      )}

      {/* F0 Extraction */}
      {ifF0 && (
        <div style={{
          display: 'flex', alignItems: 'center', justifyContent: 'space-between',
          padding: '12px 0',
          borderBottom: '1px solid var(--color-border-subtle)',
        }}>
          <Space>
            <SoundOutlined style={{ color: f0Status === 'done' ? 'var(--color-success)' : 'var(--color-text-tertiary)' }} />
            <div>
              <Text strong>F0 特征提取</Text>
              <Tag color={statusColorMap[f0Status]} style={{ marginLeft: 8 }}>
                {statusTextMap[f0Status]}
              </Tag>
              <br />
              <Text type="secondary" style={{ fontSize: 12 }}>
                提取音频的音高信息，RMVPE 算法。没有 F0 模型将无法正确转换。
              </Text>
              {f0Status === 'running' && f0Percent !== undefined && f0Percent > 0 && (
                <Progress percent={f0Percent} size="small" style={{ width: 200, marginTop: 4 }} />
              )}
            </div>
          </Space>
          <Button
            type="primary"
            onClick={onExtractF0}
            loading={currentExtracting === 'f0'}
            disabled={!preprocessDone || running}
          >
            {f0Status === 'done' ? '重新提取' : f0Status === 'error' ? '重试 F0 提取' : '提取 F0 特征'}
          </Button>
        </div>
      )}

      {/* Audio Feature Extraction */}
      <div style={{
        display: 'flex', alignItems: 'center', justifyContent: 'space-between',
        padding: '12px 0',
      }}>
        <Space>
          <ApartmentOutlined style={{ color: audioStatus === 'done' ? 'var(--color-success)' : 'var(--color-text-tertiary)' }} />
          <div>
            <Text strong>音频特征提取</Text>
            <Tag color={statusColorMap[audioStatus]} style={{ marginLeft: 8 }}>
              {statusTextMap[audioStatus]}
            </Tag>
            <br />
            <Text type="secondary" style={{ fontSize: 12 }}>
                ContentVec 提取 768 维语音特征，是 RVC 的核心。必须等 F0 完成后执行。
              </Text>
              {audioStatus === 'running' && audioPercent !== undefined && audioPercent > 0 && (
                <Progress percent={audioPercent} size="small" style={{ width: 200, marginTop: 4 }} />
              )}
            </div>
          </Space>
          <Button
            type="primary"
            onClick={onExtractAudio}
            loading={currentExtracting === 'audio'}
            disabled={!preprocessDone || !f0Done || running}
          >
            {audioStatus === 'done' ? '重新提取' : audioStatus === 'error' ? '重试音频提取' : '提取音频特征'}
          </Button>
        </div>

      {f0Done && audioStatus === 'done' && (
        <Alert type="success" showIcon style={{ marginTop: 12 }}
          message="特征提取完成，可以开始训练模型"
        />
      )}
    </Card>
  )
}
