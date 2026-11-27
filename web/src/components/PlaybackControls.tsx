import { useEffect, useRef, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { Button, message, Radio, Slider } from 'antd'
import { PlayCircleOutlined, PauseCircleOutlined, StopOutlined } from '@ant-design/icons'
import { useLiveSingingStore } from '../store/liveSingingStore'
import { useI18n } from '../i18n'
import { type SeparatedSong } from '../api/infer'

// ── Props ──

interface PlaybackControlsProps {
  /** 歌曲列表，用于 Play 首次加载时获取路径；桌面歌词场景可不传 */
  songs?: SeparatedSong[]
  /** 当前选中的歌曲名 */
  selectedSongName?: string
  /** 覆盖的歌曲名文本（桌面歌词场景用） */
  songLabel?: string
  /** 紧凑模式（桌面歌词场景用，按钮文字更少） */
  compact?: boolean
}

// ── 类型 ──

interface PlayerState {
  status: 'playing' | 'paused' | 'stopped'
  audio_source: number
  progress_secs: number
  current_line: string
  current_line_index: number
  total_lines: number
  total_duration_secs: number
  preview_lines: Array<{ time_secs: number; text: string }>
  instrumental_path?: string
  lrc_path?: string
  cover_path?: string
}

const INITIAL_STATE: PlayerState = {
  status: 'stopped',
  audio_source: 0,
  progress_secs: 0,
  current_line: '',
  current_line_index: -1,
  total_lines: 0,
  total_duration_secs: 0,
  preview_lines: [],
}

// ── 组件 ──

export default function PlaybackControls({ songs = [], selectedSongName, songLabel, compact }: PlaybackControlsProps) {
  const { t } = useI18n()
  const isDragging = useRef(false)
  const [draggingValue, setDraggingValue] = useState<number | null>(null)
  const [ps, setPs] = useState<PlayerState>(INITIAL_STATE)

  const selectedSong = selectedSongName ? songs.find((s) => s.name === selectedSongName) : undefined
  const displayLabel = songLabel ?? selectedSong?.name ?? ''

  // ── player-state 事件监听 ──
  useEffect(() => {
    let unlisten: (() => void) | undefined
    listen<PlayerState>('player-state', (event) => {
      const d = typeof event.payload === 'string' ? JSON.parse(event.payload) : event.payload
      if (d.progress_secs !== undefined || d.preview_lines !== undefined) {
        setPs({
          status: d.status ?? 'stopped',
          audio_source: d.audio_source ?? 0,
          progress_secs: d.progress_secs ?? 0,
          current_line: d.current_line ?? '',
          current_line_index: d.current_line_index ?? -1,
          total_lines: d.total_lines ?? 0,
          total_duration_secs: d.total_duration_secs ?? 0,
          preview_lines: d.preview_lines ?? [],
          instrumental_path: d.instrumental_path,
          lrc_path: d.lrc_path,
          cover_path: d.cover_path,
        })
      } else if (d.status !== undefined) {
        setPs((prev) => ({ ...prev, status: d.status }))
      }
    }).then((fn) => {
      unlisten = fn
    })
    return () => {
      unlisten?.()
    }
  }, [])

  // ── 挂载时查询后端当前状态（桌面歌词等子窗口首次打开时获取初始状态） ──
  useEffect(() => {
    invoke<PlayerState>('get_player_state')
      .then((state) => {
        setPs((prev) => ({
          ...prev,
          status: state.status ?? prev.status,
          audio_source: state.audio_source ?? prev.audio_source,
          progress_secs: state.progress_secs ?? prev.progress_secs,
          instrumental_path: state.instrumental_path,
          lrc_path: state.lrc_path,
          cover_path: state.cover_path,
        }))
      })
      .catch(() => {
        // 忽略，事件系统会后续同步
      })
  }, [])

  // ── 播放器运输 ──

  const handlePlay = async () => {
    if (ps.status === 'stopped' && (selectedSong || ps.instrumental_path)) {
      try {
        message.loading({ content: t('lyrics.loading'), key: 'acc-load' })
        const instrumentalPath = selectedSong?.instrumental_path || ps.instrumental_path!
        const lrcPath = selectedSong?.lrc_path || ps.lrc_path || null
        const coverPath = selectedSong?.converted_path || ps.cover_path || null
        await invoke('play_accompaniment', {
          audioPath: instrumentalPath,
          lrcPath,
          coverPath,
        })
        message.success({ content: t('playback.loaded'), key: 'acc-load' })
      } catch (err) {
        message.error({ content: t('playback.load_failed', { err: String(err) }), key: 'acc-load' })
      }
    } else {
      await invoke('play_current_accompaniment')
    }
  }

  const handlePause = async () => {
    await useLiveSingingStore.getState().setAccompanimentPause(true)
  }

  const handleStop = async () => {
    await invoke('stop_accompaniment')
  }

  const handleSourceChange = async (source: 'live' | 'cover') => {
    const src = source === 'live' ? 0 : 1
    const coverPath = selectedSong?.converted_path || ps.cover_path
    if (src === 1 && coverPath) {
      if (ps.status === 'stopped') {
        // 停止状态 → 重新加载伴奏 + 翻唱
        await invoke('play_accompaniment', {
          audioPath: selectedSong?.instrumental_path || ps.instrumental_path,
          lrcPath: selectedSong?.lrc_path || ps.lrc_path || null,
          coverPath,
        })
      } else {
        // 播放中 → 仅预加载翻唱人声（不重启伴奏）
        await invoke('preload_cover', { audioPath: coverPath })
      }
    }
    await invoke('set_audio_source', { source: src })
    if (src === 1) {
      await useLiveSingingStore
        .getState()
        .setAccompanimentPause(false)
        .catch(() => {})
    }
  }

  return (
    <div
      style={{
        padding: compact ? '4px 12px 4px' : '6px 16px 6px',
        display: 'flex',
        alignItems: 'center',
        gap: compact ? 4 : 8,
        flexWrap: 'wrap',
      }}
    >
      {/* 歌曲名 */}
      {displayLabel && (
        <span
          style={{
            fontSize: compact ? 11 : 12,
            fontWeight: 600,
            color: 'var(--color-text-primary)',
            maxWidth: compact ? 120 : 200,
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap',
          }}
        >
          {displayLabel}
        </span>
      )}

      {/* 进度 Slider */}
      <div style={{ flex: 1, minWidth: 60, maxWidth: compact ? 120 : 240, margin: '0 4px' }}>
        <Slider
          disabled={ps.status === 'stopped'}
          min={0}
          max={ps.total_duration_secs > 0 ? ps.total_duration_secs : 100}
          value={draggingValue !== null ? draggingValue : ps.progress_secs}
          step={0.5}
          tooltip={{ formatter: (v) => `${v?.toFixed(1)}s` }}
          onChange={(val) => {
            isDragging.current = true
            setDraggingValue(val)
          }}
          onChangeComplete={async (value) => {
            try {
              await invoke('seek_accompaniment', { positionSecs: value })
            } catch (err) {
              message.error(t('playback.seek_failed', { err: String(err) }))
            } finally {
              setTimeout(() => {
                isDragging.current = false
                setDraggingValue(null)
              }, 300)
            }
          }}
          styles={{
            track: { background: 'linear-gradient(90deg, #7c3aed, #a78bfa)' },
            handle: { borderColor: '#7c3aed', backgroundColor: '#fff' },
          }}
          style={{ margin: 0, padding: 0, width: '100%' }}
        />
      </div>

      {/* 进度时间 */}
      <span style={{ fontSize: compact ? 10 : 11, color: 'var(--color-text-tertiary)', whiteSpace: 'nowrap', fontVariantNumeric: 'tabular-nums' }}>{ps.progress_secs.toFixed(1)}s</span>

      {/* 按钮组 */}
      <Button
        size={compact ? 'small' : 'small'}
        type={ps.status === 'playing' ? 'primary' : 'default'}
        icon={<PlayCircleOutlined />}
        onClick={handlePlay}
        disabled={ps.status === 'playing' || (ps.status === 'stopped' && !selectedSong && !ps.instrumental_path)}
        style={{ borderRadius: 20, fontSize: compact ? 11 : 12 }}
      >
        {!compact && (ps.status === 'stopped' ? t('lyrics.load_play') : t('playback.play'))}
      </Button>
      <Button size={compact ? 'small' : 'small'} icon={<PauseCircleOutlined />} onClick={handlePause} disabled={ps.status !== 'playing'} style={{ borderRadius: 20, fontSize: compact ? 11 : 12 }}>
        {!compact && t('lyrics.pause')}
      </Button>
      <Button size={compact ? 'small' : 'small'} icon={<StopOutlined />} onClick={handleStop} disabled={ps.status === 'stopped'} style={{ borderRadius: 20, fontSize: compact ? 11 : 12 }}>
        {!compact && t('lyrics.stop')}
      </Button>

      {/* 音源切换：compact 模式始终显示（桌面歌词）；非 compact 模式有歌曲文件时显示 */}
      {(compact || (selectedSong?.converted_path && selectedSong.converted_path.length > 0)) && (
        <Radio.Group
          size={compact ? 'small' : 'small'}
          value={ps.audio_source === 0 ? 'live' : 'cover'}
          onChange={(e) => handleSourceChange(e.target.value)}
          optionType="button"
          buttonStyle="solid"
          style={{ marginLeft: compact ? 2 : 4 }}
        >
          <Radio.Button value="live" style={{ fontSize: compact ? 10 : 12, borderRadius: '12px 0 0 12px', padding: compact ? '0 6px' : undefined }}>
            🎤{!compact && ` ${t('playback.source_live')}`}
          </Radio.Button>
          <Radio.Button value="cover" style={{ fontSize: compact ? 10 : 12, borderRadius: '0 12px 12px 0', padding: compact ? '0 6px' : undefined }}>
            🎧{!compact && ` ${t('playback.source_cover')}`}
          </Radio.Button>
        </Radio.Group>
      )}

      {/* 状态 */}
      {!compact && (
        <span style={{ fontSize: 11, color: 'var(--color-text-tertiary)', marginLeft: 4 }}>
          {ps.status === 'stopped' ? t('lyrics.waiting') : ps.status === 'paused' ? t('lyrics.paused') : t('lyrics.playing')}
          {ps.audio_source === 1 && ` · ${t('playback.ai_cover')}`}
        </span>
      )}
    </div>
  )
}
