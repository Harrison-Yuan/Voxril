import { Card, Slider, Switch, Button, Space, Typography, Row, Col, Alert, Tag, Divider, Collapse } from 'antd'
import { DeleteOutlined, ThunderboltOutlined, ApartmentOutlined } from '@ant-design/icons'
import type { WorkflowStage } from '../hooks/useTrainWorkflow'
import type { ExperimentConfig } from '../hooks/useExperimentConfig'

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
  onConfigChange: (values: Partial<ExperimentConfig>) => void
  onRun: () => void
  onStop: () => void
  onBuildIndex: () => void
  onClearTraining: () => void
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

export const TrainModelStep = ({
  config,
  trainStatus,
  featuresStatus,
  indexStatus,
  trainEpoch,
  totalEpoch,
  running,
  canRun,
  onConfigChange,
  onRun,
  onStop,
  onBuildIndex,
  onClearTraining,
}: TrainModelStepProps) => {
  const getButtonText = () => {
    if (trainEpoch > 0 && trainEpoch < totalEpoch) return '继续训练'
    if (trainStatus === 'done') return '重新训练模型'
    return '开始训练模型'
  }

  const getIndexButtonText = () => {
    if (indexStatus === 'running') return '正在构建索引...'
    if (indexStatus === 'done') return '重新构建索引'
    return '训练特征索引'
  }

  return (
    <Card
      title={
        <Space>
          <Text>步骤 2：训练模型</Text>
          <Tag color={statusColorMap[trainStatus]}>{statusTextMap[trainStatus]}</Tag>
        </Space>
      }
      size="small"
      extra={
        <Space>
          {(trainEpoch > 0 || trainStatus === 'done') && (
            <Button danger onClick={onClearTraining} icon={<DeleteOutlined />}>
              清理训练结果
            </Button>
          )}
          {trainStatus === 'running' && (
            <Button danger onClick={onStop}>
              停止训练
            </Button>
          )}
          <Button
            type="primary"
            onClick={onRun}
            loading={running}
            disabled={!canRun}
            icon={<ThunderboltOutlined />}
          >
            {getButtonText()}
          </Button>
        </Space>
      }
    >
      <Alert
        type={featuresStatus === 'done' ? 'success' : 'warning'}
        showIcon
        style={{ marginBottom: 16 }}
        message={
          featuresStatus === 'done'
            ? '数据预处理已完成，可以开始训练。'
            : '请先完成步骤1：数据预处理。'
        }
      />

      <Row gutter={16}>
        <Col span={12}>
          <div style={{ marginBottom: 24 }}>
            <div style={{ marginBottom: 8 }}>
              <Text>总训练轮数：{config.totalEpoch}</Text>
            </div>
            <Slider
              value={config.totalEpoch}
              onChange={(value) => onConfigChange({ totalEpoch: value })}
              min={10}
              max={1000}
              step={10}
              disabled={running}
            />
          </div>
        </Col>

        <Col span={12}>
          <div style={{ marginBottom: 24 }}>
            <div style={{ marginBottom: 8 }}>
              <Text>保存频率：每 {config.saveEpoch} 轮</Text>
            </div>
            <Slider
              value={config.saveEpoch}
              onChange={(value) => onConfigChange({ saveEpoch: value })}
              min={5}
              max={100}
              step={5}
              disabled={running}
            />
          </div>
        </Col>

      </Row>

      <Collapse ghost style={{ marginTop: 8 }}>
        <Collapse.Panel header="高级设置" key="advanced">
          <Row gutter={[24, 16]}>
            <Col span={12}>
              <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
                <Text>仅保留最新 ckpt</Text>
                <Switch
                  checked={config.ifSaveLatest}
                  onChange={(checked) => onConfigChange({ ifSaveLatest: checked })}
                  disabled={featuresStatus !== 'done' || running}
                />
              </div>
            </Col>
            <Col span={12}>
              <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
                <Text>每次保存到 weights</Text>
                <Switch
                  checked={config.ifSaveEveryWeights}
                  onChange={(checked) => onConfigChange({ ifSaveEveryWeights: checked })}
                  disabled={featuresStatus !== 'done' || running}
                />
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
            <Text strong>特征索引</Text>
            <Tag color={statusColorMap[indexStatus]}>{statusTextMap[indexStatus]}</Tag>
          </Space>
          <br />
          <Text type="secondary" style={{ fontSize: 12 }}>
            训练完成后需构建特征索引才能用于声音转换。可手动点击触发。
          </Text>
        </Col>
        <Col>
          <Button
            icon={<ApartmentOutlined />}
            onClick={onBuildIndex}
            loading={indexStatus === 'running'}
            disabled={featuresStatus !== 'done' || running || indexStatus === 'running'}
          >
            {getIndexButtonText()}
          </Button>
        </Col>
      </Row>
    </Card>
  )
}
