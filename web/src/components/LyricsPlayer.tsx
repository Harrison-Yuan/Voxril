import { useEffect, useRef, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { WebviewWindow } from '@tauri-apps/api/webviewWindow'
import { listen } from '@tauri-apps/api/event'
import { useI18n } from '../i18n'
import { type SeparatedSong } from '../api/infer'
import PlaybackControls from './PlaybackControls'

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

// ── Props ──

interface LyricsPlayerProps {
  connected: boolean
  songs: SeparatedSong[]
  selectedSongName?: string
}

// ── 组件 ──

export default function LyricsPlayer({ connected, songs, selectedSongName }: LyricsPlayerProps) {
  const { t } = useI18n()
  const containerRef = useRef<HTMLDivElement>(null)
  const [expanded, setExpanded] = useState(true)
  const [ps, setPs] = useState<PlayerState>(INITIAL_STATE)

  const lines = ps.preview_lines ?? []
  const hasLines = lines.length > 0

  // ── player-state 事件监听（仅歌词相关字段） ──
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
        })
      } else if (d.status !== undefined) {
        setPs(prev => ({ ...prev, status: d.status }))
      }
    }).then((fn) => { unlisten = fn })
    return () => { unlisten?.() }
  }, [])

  // ── 换歌时重置 ──
  useEffect(() => {
    invoke('stop_accompaniment').catch(() => {})
    setPs(INITIAL_STATE)
  }, [selectedSongName])

  // ── 自动滚动到当前行 ──
  useEffect(() => {
    if (ps.current_line_index >= 0 && containerRef.current) {
      const container = containerRef.current
      const items = container.querySelectorAll('.lyrics-line')
      const active = items[ps.current_line_index]
      if (active) {
        const cr = container.getBoundingClientRect()
        const er = active.getBoundingClientRect()
        const offset = er.top - cr.top - cr.height / 2 + er.height / 2
        container.scrollBy({ top: offset, behavior: 'smooth' })
      }
    }
  }, [ps.current_line_index])

  const handleDesktopLyrics = async () => {
    try {
      const existing = await WebviewWindow.getByLabel('desktop-lyrics')
      if (existing) {
        try {
          const visible = await existing.isVisible()
          if (visible) { await existing.setFocus() }
          else { await existing.show(); await existing.setFocus() }
          return
        } catch { /* 已销毁 */ }
      }
      const win = new WebviewWindow('desktop-lyrics', {
        url: '/desktop-lyrics',
        width: 800, height: 160, center: true,
        alwaysOnTop: true, transparent: true, decorations: false, skipTaskbar: true,
      })
      win.once('tauri://error', () => { /* 忽略 */ })
    } catch { /* 忽略 */ }
  }

  // ── 渲染 ──

  if (!hasLines && !ps.current_line && !connected && !selectedSongName) return null

  return (
    <div style={{
      borderRadius: 12,
      background: 'var(--color-bg-elevated)',
      border: '1px solid var(--color-border-subtle)',
      overflow: 'hidden',
      position: 'relative',
    }}>
      {/* 头部 */}
      <div style={{
        display: 'flex', alignItems: 'center', justifyContent: 'space-between',
        padding: '10px 16px 6px', fontSize: 12, color: 'var(--color-text-tertiary)', userSelect: 'none',
      }}>
        <div style={{ cursor: 'pointer', flex: 1 }} onClick={() => setExpanded(v => !v)}>
          <span>
            🎤 {t('lyrics.title')}
            {ps.status !== 'stopped' && (
              <span style={{ marginLeft: 8, opacity: 0.6 }}>
                {Math.min(ps.current_line_index + 1, Math.max(lines.length, 1))}/{Math.max(lines.length, 1)}
                <span style={{ marginLeft: 6, fontSize: 10, opacity: 0.5 }}>
                  {ps.progress_secs.toFixed(1)}s
                </span>
              </span>
            )}
          </span>
        </div>
        <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
          <span
            style={{ cursor: 'pointer', fontSize: 11, opacity: 0.6 }}
            onClick={handleDesktopLyrics}
          >
            🖥 {t('lyrics.desktop')}
          </span>
          <span style={{ fontSize: 10, cursor: 'pointer' }} onClick={() => setExpanded(v => !v)}>
            {expanded ? t('lyrics.collapse') : t('lyrics.expand')}
          </span>
        </div>
      </div>

      {expanded && (
        <>
          {/* 控制栏（可复用组件） */}
          {connected && selectedSongName && (
            <div style={{
              borderBottom: '1px solid var(--color-border-subtle)',
            }}>
              <PlaybackControls
                songs={songs}
                selectedSongName={selectedSongName}
              />
            </div>
          )}

          {/* 歌词区域 */}
          <div ref={containerRef} style={{
            height: 280, overflowY: 'auto', padding: '8px 16px 16px', scrollBehavior: 'smooth',
          }}>
            {!hasLines && ps.status !== 'stopped' ? (
              <div style={{ textAlign: 'center', color: 'var(--color-text-tertiary)', paddingTop: 100, fontSize: 14 }}>
                {ps.current_line_index >= 0 && ps.current_line ? (
                  <div style={{ fontSize: 20, fontWeight: 700, color: '#c084fc' }}>
                    {ps.current_line}
                  </div>
                ) : t('lyrics.waiting')}
              </div>
            ) : hasLines ? (
              lines.map((line, i) => {
                const isActive = ps.status !== 'stopped' && i === ps.current_line_index
                return (
                  <div key={i} className="lyrics-line" style={{
                    textAlign: 'center',
                    fontSize: isActive ? 20 : 13,
                    fontWeight: isActive ? 700 : 400,
                    color: isActive ? '#fff' : 'var(--color-text-secondary)',
                    padding: isActive ? '10px 12px' : '6px 12px',
                    lineHeight: 1.6, transition: 'all 0.15s ease',
                    borderRadius: isActive ? 8 : 0,
                    background: isActive ? 'linear-gradient(135deg, rgba(124,58,237,0.25), rgba(167,139,250,0.10))' : 'transparent',
                    borderBottom: i < lines.length - 1 ? '1px solid var(--color-border-subtle)' : 'none',
                    ...(isActive ? { textShadow: '0 0 20px rgba(192,132,252,0.5)', margin: '4px 0' } : {}),
                  }}>
                    {line.text}
                  </div>
                )
              })
            ) : null}
          </div>
        </>
      )}
    </div>
  )
}
