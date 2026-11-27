import { useEffect, useMemo, useState } from 'react'
import { Button, Modal, Spin, Tag, message } from 'antd'
import { AppstoreOutlined, PlayCircleOutlined, CheckCircleOutlined, DeleteOutlined, StarOutlined, StarFilled, ReloadOutlined } from '@ant-design/icons'
import { convertFileSrc } from '@tauri-apps/api/core'
import { outputsApi } from '@/api/outputs'
import type { OutputItem } from '@/api/outputs'
import { formatBytes, formatDuration } from '@/lib/formatBytes'
import { useI18n } from '@/i18n'

function SectionHeader({ children }: { children: React.ReactNode }) {
  return <div style={{ padding: '12px 20px', borderBottom: '1px solid var(--color-border-subtle)', fontSize: 13, fontWeight: 600 }}>{children}</div>
}

interface OutputLibraryProps {
  /** 变化时重新拉取列表（如转换/合成产出了新成品） */
  refreshToken: string
  /** 卡片标题，默认「成品库」 */
  title?: string
  /** 仅展示作品（mixed），过滤掉人声轨 */
  onlyWorks?: boolean
}

/** 成品库：按批次分组展示转换/混音结果，支持播放、定稿、星标与删除。 */
export function OutputLibrary({ refreshToken, title, onlyWorks = false }: OutputLibraryProps) {
  const { t } = useI18n()
  const [items, setItems] = useState<OutputItem[]>([])
  const [loading, setLoading] = useState(false)
  const [busyPath, setBusyPath] = useState<string | null>(null)
  const [playing, setPlaying] = useState<string | null>(null)

  const load = async () => {
    setLoading(true)
    try {
      setItems(await outputsApi.list())
    } catch (e: any) {
      message.error(t('library.load_failed', { msg: e?.message || e }))
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    void load()
  }, [refreshToken])

  const visibleItems = useMemo(() => (onlyWorks ? items.filter((i) => i.kind === 'mixed') : items), [items, onlyWorks])

  const groups = useMemo(() => {
    const map = new Map<string, OutputItem[]>()
    for (const item of visibleItems) {
      const key = item.run_id || t('library.unknown_batch')
      const arr = map.get(key) ?? []
      arr.push(item)
      map.set(key, arr)
    }
    return Array.from(map.entries())
      .map(([runId, list]) => {
        const sorted = [...list].sort((a, b) => b.created_at - a.created_at)
        return { runId, list: sorted, voice: sorted.find((i) => i.voice)?.voice || '' }
      })
      .sort((a, b) => (b.list[0]?.created_at ?? 0) - (a.list[0]?.created_at ?? 0))
  }, [visibleItems, t])

  const handleFinalize = async (item: OutputItem) => {
    setBusyPath(item.path)
    try {
      const res = await outputsApi.finalize(item.id)
      message.success(t('library.finalized', { name: res.path.split('/').pop() || '' }))
      await load()
    } catch (e: any) {
      message.error(t('library.finalize_failed', { msg: e?.message || e }))
    } finally {
      setBusyPath(null)
    }
  }

  const handleStar = async (item: OutputItem) => {
    try {
      await outputsApi.setStarred(item.id, !item.starred)
      setItems((prev) => prev.map((i) => (i.id === item.id ? { ...i, starred: !i.starred } : i)))
    } catch (e: any) {
      message.error(t('library.star_failed', { msg: e?.message || e }))
    }
  }

  const handleDelete = (item: OutputItem) => {
    Modal.confirm({
      title: t('library.delete_title'),
      content: t('library.delete_confirm', { name: item.song || item.path.split('/').pop() || '' }),
      okText: t('common.delete'),
      okType: 'danger',
      cancelText: t('common.cancel'),
      onOk: async () => {
        try {
          await outputsApi.remove(item.path)
          message.success(t('library.moved_to_trash'))
          if (playing === item.path) setPlaying(null)
          await load()
        } catch (e: any) {
          message.error(t('library.delete_failed', { msg: e?.message || e }))
        }
      },
    })
  }

  return (
    <div className="ant-card" style={{ padding: 0 }}>
      <SectionHeader>
        <span style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          <AppstoreOutlined style={{ color: 'var(--color-primary-hover)' }} />
          {title ?? t('library.title')}
          <Button size="small" type="text" icon={<ReloadOutlined />} onClick={load} loading={loading} style={{ marginLeft: 'auto' }} />
        </span>
      </SectionHeader>
      <div style={{ padding: 20 }}>
        {loading && visibleItems.length === 0 ? (
          <Spin size="small" />
        ) : visibleItems.length === 0 ? (
          <div style={{ fontSize: 13, color: 'var(--color-text-tertiary)' }}>
            {onlyWorks ? t('library.empty_works') : t('library.empty_outputs')}
          </div>
        ) : (
          groups.map((group) => (
            <div key={group.runId} style={{ marginBottom: 16 }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 8 }}>
                <code style={{ fontSize: 12, color: 'var(--color-primary-hover)' }}>{group.runId}</code>
                {group.voice && <span style={{ fontSize: 12, color: 'var(--color-text-secondary)' }}>· {group.voice}</span>}
                <span style={{ fontSize: 11, color: 'var(--color-text-tertiary)' }}>{t('library.item_count', { count: group.list.length })}</span>
              </div>
              {group.list.map((item) => (
                <div
                  key={item.id}
                  style={{
                    border: '1px solid var(--color-border-subtle)',
                    borderRadius: 'var(--radius-lg)',
                    padding: '8px 12px',
                    marginBottom: 8,
                  }}
                >
                  <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                    <Tag color={item.kind === 'mixed' ? 'purple' : 'blue'} style={{ marginInlineEnd: 0, fontSize: 11 }}>
                      {item.kind === 'mixed' ? t('library.kind_work') : t('library.kind_vocal')}
                    </Tag>
                    <div style={{ flex: 1, minWidth: 0 }}>
                      <div style={{ fontSize: 13, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{item.song || item.path.split('/').pop()}</div>
                      <div style={{ fontSize: 11, color: 'var(--color-text-tertiary)' }}>
                        {formatDuration(item.duration_secs)} · key{item.transpose >= 0 ? `+${item.transpose}` : item.transpose} · {formatBytes(item.bytes)}
                        {item.finalized ? t('library.finalized_tag') : ''}
                      </div>
                    </div>
                    <Button size="small" type="text" icon={<PlayCircleOutlined />} onClick={() => setPlaying(playing === item.path ? null : item.path)} />
                    <Button size="small" type="text" icon={item.starred ? <StarFilled style={{ color: '#fadb14' }} /> : <StarOutlined />} onClick={() => handleStar(item)} />
                    {!item.finalized && (
                      <Button size="small" type="text" icon={<CheckCircleOutlined />} loading={busyPath === item.path} onClick={() => handleFinalize(item)} title={t('library.finalize_tip')} />
                    )}
                    <Button size="small" type="text" danger icon={<DeleteOutlined />} onClick={() => handleDelete(item)} />
                  </div>
                  {playing === item.path && <audio controls autoPlay src={convertFileSrc(item.path)} style={{ width: '100%', marginTop: 8 }} />}
                </div>
              ))}
            </div>
          ))
        )}
      </div>
    </div>
  )
}
