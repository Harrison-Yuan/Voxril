import { useState } from 'react'
import { Modal, Input, List, Button, message, Typography, Space } from 'antd'
import { SearchOutlined, DownloadOutlined, LoadingOutlined } from '@ant-design/icons'
import { useLiveSingingStore } from '../store/liveSingingStore'
import { useI18n } from '../i18n'

interface SongResult {
  id: number
  name: string
  artist: string
}

interface Props {
  audioPath?: string
  songName?: string
  onDownloaded?: (lrcPath: string) => void
  children?: (open: () => void) => React.ReactNode
}

export default function LyricsSearchModal({ audioPath, songName: defaultQuery, onDownloaded, children }: Props) {
  const { t } = useI18n()
  const [open, setOpen] = useState(false)
  const [query, setQuery] = useState(defaultQuery || '')
  const [results, setResults] = useState<SongResult[]>([])
  const [searching, setSearching] = useState(false)
  const [downloadingId, setDownloadingId] = useState<number | null>(null)

  const searchLyrics = useLiveSingingStore(s => s.searchLyrics)
  const downloadLyrics = useLiveSingingStore(s => s.downloadLyrics)

  const handleSearch = async () => {
    if (!query.trim()) return
    setSearching(true)
    setResults([])
    try {
      const res = await searchLyrics(query.trim())
      setResults(res)
    } catch (err) {
      message.error(`${t('lyrics_search.search_failed')}: ${err}`)
    } finally {
      setSearching(false)
    }
  }

  const handleDownload = async (song: SongResult) => {
    if (!audioPath) {
      message.error(t('lyrics_search.no_song'))
      return
    }
    setDownloadingId(song.id)
    try {
      const result = await downloadLyrics(song.id, audioPath)
      message.success(`${t('lyrics_search.downloaded')} ${song.name} - ${song.artist} (${result.lineCount} ${t('lyrics_search.lines')})`)
      onDownloaded?.(result.lrcPath)
      setOpen(false)
    } catch (err) {
      message.error(`${t('lyrics_search.download_failed')}: ${err}`)
    } finally {
      setDownloadingId(null)
    }
  }

  return (
    <>
      {children?.(() => {
        setQuery(defaultQuery || '')
        setResults([])
        setOpen(true)
        // 自动搜索
        setTimeout(() => {
          if (defaultQuery) {
            setSearching(true)
            searchLyrics(defaultQuery).then(setResults).catch(() => {}).finally(() => setSearching(false))
          }
        }, 100)
      })}
      <Modal
        title={
          <Space>
            <SearchOutlined />
            {t('lyrics_search.title')}
          </Space>
        }
        open={open}
        onCancel={() => setOpen(false)}
        footer={null}
        width={520}
        destroyOnClose
      >
        <div style={{ display: 'flex', gap: 8, marginBottom: 16 }}>
          <Input
            value={query}
            onChange={e => setQuery(e.target.value)}
            onPressEnter={handleSearch}
            placeholder={t('lyrics_search.placeholder')}
            allowClear
          />
          <Button type="primary" icon={<SearchOutlined />} onClick={handleSearch} loading={searching}>
            {t('lyrics_search.search')}
          </Button>
        </div>

        {searching && (
          <div style={{ textAlign: 'center', padding: '40px 0', color: 'var(--color-text-tertiary)' }}>
            <LoadingOutlined style={{ fontSize: 24 }} />
            <div style={{ marginTop: 8 }}>{t('lyrics_search.searching')}</div>
          </div>
        )}

        {!searching && results.length > 0 && (
          <List
            dataSource={results}
            renderItem={(song) => (
              <List.Item
                style={{
                  cursor: 'default',
                  padding: '10px 12px',
                  borderRadius: 8,
                  transition: 'background 0.15s',
                }}
                onMouseEnter={e => (e.currentTarget.style.background = 'var(--color-bg-hover)')}
                onMouseLeave={e => (e.currentTarget.style.background = 'transparent')}
              >
                <div style={{ flex: 1, minWidth: 0 }}>
                  <Typography.Text strong style={{ fontSize: 14 }}>{song.name}</Typography.Text>
                  <Typography.Text type="secondary" style={{ marginLeft: 8, fontSize: 12 }}>
                    {song.artist}
                  </Typography.Text>
                </div>
                <Button
                  type="link"
                  size="small"
                  icon={downloadingId === song.id ? <LoadingOutlined /> : <DownloadOutlined />}
                  onClick={() => handleDownload(song)}
                  disabled={!audioPath}
                >
                  {t('lyrics_search.download')}
                </Button>
              </List.Item>
            )}
          />
        )}

        {!searching && results.length === 0 && !searching && (
          <div style={{ textAlign: 'center', padding: '20px 0', color: 'var(--color-text-quaternary)', fontSize: 13 }}>
            {defaultQuery ? t('lyrics_search.auto_searching') : t('lyrics_search.initial')}
          </div>
        )}
      </Modal>
    </>
  )
}
