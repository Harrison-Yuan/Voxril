import { useEffect, useState } from 'react'
import { listen } from '@tauri-apps/api/event'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { useI18n } from '@/i18n'
import PlaybackControls from '@/components/PlaybackControls'

const win = getCurrentWebviewWindow()

export default function DesktopLyricsPage() {
  const { t } = useI18n()
  const [current, setCurrent] = useState('')
  const [next, setNext] = useState('')
  const [progress, setProgress] = useState(0)
  const [total, setTotal] = useState(0)
  const [hasLyrics, setHasLyrics] = useState(false)

  useEffect(() => {
    // ⚠️ 强制 html/body/root 完全透明
    const htmlEl = document.documentElement
    const bodyEl = document.body
    const rootEl = document.getElementById('root')
    if (htmlEl) htmlEl.style.background = 'transparent'
    if (bodyEl) {
      bodyEl.style.background = 'transparent'
      bodyEl.style.backgroundColor = 'transparent'
    }
    if (rootEl) {
      rootEl.style.background = 'transparent'
      rootEl.style.backgroundColor = 'transparent'
    }

    let unlisten: (() => void) | undefined
    listen<any>('player-state', (event) => {
      const d = typeof event.payload === 'string' ? JSON.parse(event.payload) : event.payload
      setCurrent(d.current_line || '')
      setNext(d.next_line || '')
      setProgress(d.progress_secs || 0)
      setTotal(d.total_duration_secs || 0)
      setHasLyrics(d.total_lines > 0)
    }).then((fn) => { unlisten = fn })

    const handleKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') { win.close() }
    }
    window.addEventListener('keydown', handleKey)
    return () => {
      unlisten?.()
      window.removeEventListener('keydown', handleKey)
    }
  }, [])

  const pct = total > 0 ? Math.min((progress / total) * 100, 100) : 0

  return (
    <div
      style={{
        width: '100vw',
        height: '100vh',
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        justifyContent: 'center',
        padding: '24px 48px 48px',
        background: 'transparent',
        userSelect: 'none',
        fontFamily: '-apple-system, BlinkMacSystemFont, "PingFang SC", "Microsoft YaHei", sans-serif',
        position: 'relative',
        boxShadow: '0 8px 48px rgba(0,0,0,0.4), 0 0 0 1px rgba(255,255,255,0.08)',
      }}
    >
      {/* 拖动条 + 关闭按钮 */}
      <div
        data-tauri-drag-region
        style={{
          position: 'absolute', top: 0, left: 0, right: 0, height: 28,
          display: 'flex', alignItems: 'center', justifyContent: 'flex-end',
          paddingRight: 8, pointerEvents: 'auto', zIndex: 10,
        }}
      >
        <span
          style={{
            display: 'flex', alignItems: 'center', justifyContent: 'center', gap: 4,
            color: 'rgba(255,255,255,0.6)', fontSize: 11, fontWeight: 500,
            cursor: 'pointer', padding: '4px 12px 4px 10px', borderRadius: 20,
            background: 'rgba(255,255,255,0.08)',
            backdropFilter: 'blur(12px)', WebkitBackdropFilter: 'blur(12px)',
            border: '1px solid rgba(255,255,255,0.12)',
            fontFamily: '-apple-system, BlinkMacSystemFont, "SF Pro Text", sans-serif',
            letterSpacing: '0.01em', transition: 'all 0.2s ease',
          }}
          onClick={() => win.close()}
          onMouseEnter={(e) => {
            e.currentTarget.style.color = 'rgba(255,255,255,0.9)'
            e.currentTarget.style.background = 'rgba(255,255,255,0.15)'
            e.currentTarget.style.borderColor = 'rgba(255,255,255,0.25)'
          }}
          onMouseLeave={(e) => {
            e.currentTarget.style.color = 'rgba(255,255,255,0.6)'
            e.currentTarget.style.background = 'rgba(255,255,255,0.08)'
            e.currentTarget.style.borderColor = 'rgba(255,255,255,0.12)'
          }}
        >
          <span style={{ fontSize: 13, lineHeight: 1, marginTop: -1 }}>✕</span>
          <span>{t('desktop_lyrics.close')}</span>
        </span>
      </div>

      {/* 歌词内容 */}
      <div
        style={{
          display: 'flex', flexDirection: 'column', alignItems: 'center',
          justifyContent: 'center', pointerEvents: 'none', maxWidth: '90%', flex: 1,
        }}
      >
        {!hasLyrics && (
          <div style={{ color: 'rgba(255,255,255,0.3)', fontSize: 13, textAlign: 'center' }}>
            <div>{t('desktop_lyrics.waiting')}</div>
          </div>
        )}

        {current && (
          <div
            style={{
              fontSize: 34, fontWeight: 700, color: '#ffffff',
              textAlign: 'center', lineHeight: 1.5,
              textShadow: '0 1px 12px rgba(0,0,0,0.6)',
              marginBottom: next ? 16 : 0, maxWidth: '100%', wordBreak: 'break-word',
              WebkitTextStroke: '0.5px rgba(255,255,255,0.25)',
              background: 'linear-gradient(135deg, rgba(255,255,255,0.95) 0%, rgba(255,255,255,0.6) 45%, rgba(255,255,255,0.9) 55%, rgba(255,255,255,0.4) 100%)',
              WebkitBackgroundClip: 'text', WebkitTextFillColor: 'transparent',
              backgroundClip: 'text', letterSpacing: '0.02em',
            }}
          >
            {current}
          </div>
        )}

        {next && (
          <div
            style={{
              fontSize: 17, fontWeight: 400, color: 'rgba(255,255,255,0.40)',
              textAlign: 'center', lineHeight: 1.5,
              textShadow: '0 1px 8px rgba(0,0,0,0.5)',
              maxWidth: '100%', wordBreak: 'break-word',
              WebkitTextStroke: '0.3px rgba(255,255,255,0.12)',
              background: 'linear-gradient(135deg, rgba(255,255,255,0.5) 0%, rgba(255,255,255,0.25) 45%, rgba(255,255,255,0.4) 55%, rgba(255,255,255,0.15) 100%)',
              WebkitBackgroundClip: 'text', WebkitTextFillColor: 'transparent',
              backgroundClip: 'text', letterSpacing: '0.02em',
            }}
          >
            {next}
          </div>
        )}
      </div>

      {/* 控制栏（紧凑模式） */}
      <div style={{
        width: '100%', padding: '4px 12px',
        borderTop: '1px solid rgba(255,255,255,0.06)',
        display: 'flex', alignItems: 'center', justifyContent: 'center',
        pointerEvents: 'auto',
      }}>
        <PlaybackControls compact />
      </div>

      {/* 进度条 */}
      {hasLyrics && (
        <div
          style={{
            position: 'absolute', bottom: 0, left: 0, right: 0, height: 3,
            background: 'rgba(255,255,255,0.04)', pointerEvents: 'none',
          }}
        >
          <div
            style={{
              height: '100%', width: `${pct}%`,
              background: 'linear-gradient(90deg, rgba(124,58,237,0.5), rgba(167,139,250,0.2))',
              transition: 'width 0.3s ease',
            }}
          />
        </div>
      )}
    </div>
  )
}
