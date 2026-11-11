import { useEffect, useRef, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { WebviewWindow } from '@tauri-apps/api/webviewWindow'
import { listen } from '@tauri-apps/api/event'
import { message } from 'antd'

interface LrcLine {
  time_secs: number
  text: string
}

interface LiveLyrics {
  currentLine: string
  currentLineIndex: number
  totalLines: number
  nextLine: string
  progressSecs: number
  totalDurationSecs: number
}

interface LyricsDisplayProps {
  allLines?: LrcLine[]
  live?: LiveLyrics
}

export default function LyricsDisplay({ allLines, live }: LyricsDisplayProps) {
  const containerRef = useRef<HTMLDivElement>(null)
  const [expanded, setExpanded] = useState(true)

  const lines = allLines ?? []
  const hasLines = lines.length > 0

  useEffect(() => {
    if (live && containerRef.current && live.currentLineIndex >= 0) {
      const container = containerRef.current
      const items = container.querySelectorAll('.lyrics-line')
      const active = items[live.currentLineIndex]
      if (active) {
        const containerRect = container.getBoundingClientRect()
        const elRect = active.getBoundingClientRect()
        const offset = elRect.top - containerRect.top - containerRect.height / 2 + elRect.height / 2
        container.scrollBy({ top: offset, behavior: 'smooth' })
      }
    }
  }, [live?.currentLineIndex])

  if (!hasLines && !live) return null

  const progressWidth = live && live.totalDurationSecs > 0
    ? Math.min((live.progressSecs / live.totalDurationSecs) * 100, 100)
    : 0

  return (
    <div style={{
      borderRadius: 12,
      background: 'var(--color-bg-elevated)',
      border: '1px solid var(--color-border-subtle)',
      overflow: 'hidden',
      position: 'relative',
    }}>
      {/* 进度条 */}
      <div style={{
        height: 3,
        background: 'var(--color-bg-hover)',
        position: 'absolute',
        top: 0,
        left: 0,
        right: 0,
        zIndex: 1,
      }}>
        <div style={{
          height: '100%',
          width: `${progressWidth}%`,
          background: 'linear-gradient(90deg, #7c3aed, #a78bfa)',
          transition: 'width 0.3s ease',
          borderRadius: '0 2px 2px 0',
        }} />
      </div>

      {/* 头部 */}
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          padding: '10px 16px 6px',
          fontSize: 12,
          color: 'var(--color-text-tertiary)',
          userSelect: 'none',
        }}
      >
        <div
          style={{ cursor: 'pointer', flex: 1 }}
          onClick={() => setExpanded(v => !v)}
        >
          <span>
            🎤 歌词
            {live && (
              <span style={{ marginLeft: 8, opacity: 0.6 }}>
                {Math.min(live.currentLineIndex + 1, Math.max(lines.length, 1))}/{Math.max(lines.length, 1)}
                <span style={{ marginLeft: 6, fontSize: 10, opacity: 0.5 }}>
                  {live.progressSecs.toFixed(1)}s
                </span>
              </span>
            )}
          </span>
        </div>
        <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
          <span
            style={{ cursor: 'pointer', fontSize: 11, opacity: 0.6 }}
            onClick={async (e) => {
              e.stopPropagation()
              try {
                const existing = await WebviewWindow.getByLabel('desktop-lyrics')
                if (existing) {
                  // 窗口存在且未销毁
                  try {
                    const visible = await existing.isVisible()
                    if (visible) { await existing.setFocus() }
                    else { await existing.show(); await existing.setFocus() }
                    return
                  } catch {
                    // 窗口已销毁，走下面重建
                  }
                }
                // 创建新窗口
                const win = new WebviewWindow('desktop-lyrics', {
                  url: '/desktop-lyrics',
                  width: 800,
                  height: 160,
                  center: true,
                  alwaysOnTop: true,
                  transparent: true,
                  decorations: false,
                  skipTaskbar: true,
                })
                win.once('tauri://error', () => {
                  message.error('打开桌面歌词失败')
                })
              } catch (err) {
                message.error(`打开桌面歌词失败: ${err}`)
              }
            }}
          >
            🖥 桌面歌词
          </span>
          <span
            style={{ fontSize: 10, cursor: 'pointer' }}
            onClick={() => setExpanded(v => !v)}
          >
            {expanded ? '收起 ▲' : '展开 ▼'}
          </span>
        </div>
      </div>

      {/* 歌词区域 */}
      {expanded && (
        <div
          ref={containerRef}
          style={{
            height: 280,
            overflowY: 'auto',
            padding: '8px 16px 16px',
            scrollBehavior: 'smooth',
          }}
        >
          {!hasLines && live ? (
            <div style={{ textAlign: 'center', color: 'var(--color-text-tertiary)', paddingTop: 100, fontSize: 14 }}>
              {live.currentLineIndex >= 0 ? (
                <div style={{ fontSize: 20, fontWeight: 700, color: '#c084fc' }}>
                  {live.currentLine || '...'}
                </div>
              ) : (
                '等待歌词同步中...'
              )}
            </div>
          ) : hasLines ? (
            lines.map((line, i) => {
              const isActive = !!(live && i === live.currentLineIndex)
              return (
                <div
                  key={i}
                  className="lyrics-line"
                  style={{
                    textAlign: 'center',
                    fontSize: isActive ? 20 : 13,
                    fontWeight: isActive ? 700 : 400,
                    color: isActive ? '#fff' : 'var(--color-text-secondary)',
                    padding: isActive ? '10px 12px' : '6px 12px',
                    lineHeight: 1.6,
                    transition: 'all 0.15s ease',
                    borderRadius: isActive ? 8 : 0,
                    background: isActive ? 'linear-gradient(135deg, rgba(124,58,237,0.25), rgba(167,139,250,0.10))' : 'transparent',
                    borderBottom: i < lines.length - 1 ? '1px solid var(--color-border-subtle)' : 'none',
                    ...(isActive ? { textShadow: '0 0 20px rgba(192,132,252,0.5)', margin: '4px 0' } : {}),
                  }}
                >
                  {line.text}
                </div>
              )
            })
          ) : null}
        </div>
      )}
    </div>
  )
}

export function useLyrics() {
  const [live, setLive] = useState<LiveLyrics>({
    currentLine: '',
    currentLineIndex: -1,
    totalLines: 0,
    nextLine: '',
    progressSecs: 0,
    totalDurationSecs: 0,
  })
  const [allLines, setAllLines] = useState<LrcLine[] | undefined>(undefined)

  useEffect(() => {
    let unlisten: (() => void) | undefined
    listen<LiveLyrics>('live-lyrics', (event) => {
      const data = typeof event.payload === 'string'
        ? JSON.parse(event.payload)
        : event.payload
      setLive({
        currentLine: data.current_line || '',
        currentLineIndex: data.current_line_index ?? -1,
        totalLines: data.total_lines ?? 0,
        nextLine: data.next_line || '',
        progressSecs: data.progress_secs ?? 0,
        totalDurationSecs: data.total_duration_secs ?? 0,
      })
    }).then((fn) => { unlisten = fn })
    return () => { unlisten?.() }
  }, [])

  const loadLrcFile = async (lrcPath: string) => {
    try {
      const lines = await invoke<LrcLine[]>('read_lrc_file', { lrcPath })
      setAllLines(lines)
      return lines
    } catch {
      setAllLines(undefined)
      return null
    }
  }

  const clearLyrics = () => {
    setAllLines(undefined)
    setLive({
      currentLine: '',
      currentLineIndex: -1,
      totalLines: 0,
      nextLine: '',
      progressSecs: 0,
      totalDurationSecs: 0,
    })
  }

  return { live, allLines, loadLrcFile, clearLyrics }
}
