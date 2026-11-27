import { Space, Table, Tag, Button, Tooltip, Typography } from 'antd'
import type { ColumnsType } from 'antd/es/table'
import { DeleteOutlined } from '@ant-design/icons'
import type { ExperimentSummary } from '../services/trainApi'
import { formatBytes } from '@/lib/formatBytes'
import { useI18n } from '@/i18n'

const { Text } = Typography

/** 批次号 `260304-1530` → `2026-03-04 15:30`；无法解析时原样返回。 */
export function formatRunDate(runId: string): string {
  const matched = /^(\d{2})(\d{2})(\d{2})-(\d{2})(\d{2})/.exec(runId)
  if (!matched) return runId
  return `20${matched[1]}-${matched[2]}-${matched[3]} ${matched[4]}:${matched[5]}`
}

interface BatchTableProps {
  /** 已按展示顺序（最新在前）排好的批次 */
  batches: ExperimentSummary[]
  /** 当前工作位限定名 `<voice>/<run_id>`；命中时打「当前」标记 */
  currentRunKey?: string
  /** 训练进行中：禁用操作，避免删到正在写入的批次 */
  running?: boolean
  /** 传入才显示「加载」按钮（把该批次设为当前工作位） */
  onLoad?: (batch: ExperimentSummary) => void
  onDelete: (batch: ExperimentSummary) => void
  /** 是否展示「数据集指纹」列 */
  showFingerprint?: boolean
  emptyText?: string
}

/**
 * 批次表格：训练页与模型页共用。
 * 训练页额外需要「加载」「当前」；模型页只看历史批次，故两者均由 props 控制。
 */
export function BatchTable({ batches, currentRunKey, running = false, onLoad, onDelete, showFingerprint = false, emptyText }: BatchTableProps) {
  const { t } = useI18n()
  const columns: ColumnsType<ExperimentSummary> = [
    {
      title: t('trainStep.col_batch'),
      dataIndex: 'run_id',
      render: (runId: string, record) => (
        <Space size={4}>
          <Text>{formatRunDate(runId)}</Text>
          {currentRunKey === `${record.voice}/${record.run_id}` && <Tag color="processing">{t('trainStep.current_tag')}</Tag>}
        </Space>
      ),
    },
    {
      title: t('train.sr'),
      dataIndex: 'sr',
      width: 90,
      render: (sr: string | null) => sr ?? '—',
    },
    {
      title: 'Epoch',
      dataIndex: 'epoch',
      width: 80,
      render: (epoch: number | null) => (epoch === null ? '—' : epoch),
    },
  ]

  if (showFingerprint) {
    columns.push({
      title: t('trainStep.col_fingerprint'),
      dataIndex: 'fingerprint',
      width: 120,
      render: (fingerprint: string | null) =>
        fingerprint ? (
          <Tooltip title={fingerprint}>
            <Text code style={{ fontSize: 11 }}>
              {fingerprint.slice(0, 8)}
            </Text>
          </Tooltip>
        ) : (
          '—'
        ),
    })
  }

  columns.push(
    {
      title: t('trainStep.col_weights'),
      dataIndex: 'has_model',
      width: 80,
      render: (has: boolean) => (has ? t('trainStep.weights_ready') : '—'),
    },
    {
      title: t('trainStep.col_index'),
      dataIndex: 'has_index',
      width: 80,
      render: (has: boolean) => (has ? t('trainStep.index_ready') : '—'),
    },
    {
      title: t('trainStep.col_size'),
      dataIndex: 'bytes',
      width: 100,
      render: (bytes: number) => formatBytes(bytes),
    },
    {
      title: t('trainStep.col_actions'),
      width: onLoad ? 140 : 80,
      render: (_: unknown, record) => (
        <Space size={4}>
          {onLoad && (
            <Button size="small" disabled={running || currentRunKey === `${record.voice}/${record.run_id}`} onClick={() => onLoad(record)}>
              {t('trainStep.load')}
            </Button>
          )}
          <Button size="small" danger icon={<DeleteOutlined />} disabled={running} onClick={() => onDelete(record)} />
        </Space>
      ),
    },
  )

  return (
    <Table<ExperimentSummary>
      size="small"
      rowKey={(record) => `${record.voice}/${record.run_id}`}
      dataSource={batches}
      pagination={false}
      locale={{ emptyText: emptyText ?? t('trainStep.no_batches') }}
      columns={columns}
    />
  )
}
