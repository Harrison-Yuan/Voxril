import { Card, Form, Input, Select, Slider, Button, Space, Typography, Row, Col, Alert, Tag, AutoComplete } from 'antd'
import { DatabaseOutlined } from '@ant-design/icons'
import type { WorkflowStage } from '../hooks/useTrainWorkflow'
import type { ExperimentConfig } from '../hooks/useExperimentConfig'

const { Text } = Typography

interface PreprocessStepProps {
  config: ExperimentConfig
  status: WorkflowStage
  running: boolean
  canRun: boolean
  experimentOptions: string[]
  hasStartedWorkflow: boolean
  onConfigChange: (values: Partial<ExperimentConfig>) => void
  onExpDirChange: (expDir: string) => void
  onSelectDirectory: () => void
  onRun: () => void
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

export const PreprocessStep = ({
  config,
  status,
  running,
  canRun,
  experimentOptions: _experimentOptions,
  hasStartedWorkflow,
  onConfigChange,
  onExpDirChange,
  onSelectDirectory,
  onRun,
}: PreprocessStepProps) => {
  return (
    <Card
      title={
        <Space>
          <Text>步骤 1：数据预处理</Text>
          <Tag color={statusColorMap[status]}>{statusTextMap[status]}</Tag>
        </Space>
      }
      size="small"
      extra={
        <Button
          type="primary"
          onClick={onRun}
          loading={running}
          disabled={!canRun}
          icon={<DatabaseOutlined />}
        >
          {status === 'done' ? '重新执行数据预处理' : '开始数据预处理'}
        </Button>
      }
    >
      {!config.trainsetDir && (
        <Alert
          type="warning"
          showIcon
          style={{ marginBottom: 16 }}
          message="请先选择训练集路径"
          description="点击「训练集路径」输入框右侧的「选择文件夹」按钮，选择包含训练音频文件的文件夹。"
        />
      )}
      
      {config.trainsetDir && (
        <Alert
          type="success"
          showIcon
          style={{ marginBottom: 16 }}
          message="训练集路径已配置"
          description={`当前路径：${config.trainsetDir}`}
        />
      )}

      <Row gutter={16}>
        <Col span={12}>
          <Form.Item
            label="实验名称"
            required
            extra={
              hasStartedWorkflow
                ? "切换实验将放弃当前进度，加载新实验的配置"
                : "可下拉切换已有实验，也可以直接输入新名称"
            }
          >
            <AutoComplete
              value={config.expDir}
              onChange={(value) => {
                onConfigChange({ expDir: value })
              }}
              onSelect={(value) => {
                onExpDirChange(value)
              }}
              options={_experimentOptions.map((name) => ({ value: name }))}
              placeholder="选择已有实验或输入新名称"
              disabled={running}
              filterOption={(inputValue, option) =>
                String(option?.value ?? '').toUpperCase().includes(String(inputValue).toUpperCase())
              }
            >
              <Input />
            </AutoComplete>
          </Form.Item>
        </Col>

        <Col span={12}>
          <Form.Item 
            label="训练集路径" 
            required 
            extra="点击右侧按钮选择包含训练音频的文件夹。"
            validateStatus={!config.trainsetDir ? 'warning' : 'success'}
          >
            <Input
              value={config.trainsetDir}
              onChange={(e) => onConfigChange({ trainsetDir: e.target.value })}
              disabled={running}
              placeholder="请点击右侧按钮选择文件夹"
              status={!config.trainsetDir ? 'warning' : undefined}
              addonAfter={
                <Button type="link" size="small" disabled={running} onClick={onSelectDirectory}>
                  选择文件夹
                </Button>
              }
            />
          </Form.Item>
        </Col>

        <Col span={12}>
          <Form.Item label="目标采样率" extra="采样率越高音质越好，但训练时间更长。40k适合大多数场景。">
            <Select
              value={config.sr}
              onChange={(value) => onConfigChange({ sr: value })}
              disabled={running}
              options={[
                { value: '32k', label: '32k（说话/播客）' },
                { value: '40k', label: '40k（推荐/歌唱）' },
                { value: '48k', label: '48k（高质量）' },
              ]}
            />
          </Form.Item>
        </Col>

        <Col span={12}>
          <Form.Item label="音高提取算法" extra="RMVPE 效果最佳（ONNX 加速）；Harvest 低音更好但慢；PM 最快。">
            <Select
              value={config.f0Method}
              onChange={(value) => onConfigChange({ f0Method: value })}
              disabled={running}
              options={[
                { value: 'rmvpe', label: 'RMVPE（推荐，ONNX）' },
                { value: 'harvest', label: 'Harvest（低音好）' },
                { value: 'pm', label: 'PM（最快）' },
              ]}
            />
          </Form.Item>
        </Col>

        <Col span={12}>
          <Form.Item
            label={`并行进程数：${config.npProcess}`}
            extra="增加进程数可加快处理速度。"
          >
            <Slider
              value={config.npProcess}
              onChange={(value) => onConfigChange({ npProcess: value })}
              min={1}
              max={8}
              step={1}
              disabled={running}
            />
          </Form.Item>
        </Col>
      </Row>
    </Card>
  )
}
