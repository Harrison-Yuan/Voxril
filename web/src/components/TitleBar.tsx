import { useState, useEffect } from 'react'
import { Link, useLocation } from 'react-router-dom'
import { getCurrentWindow } from '@tauri-apps/api/window'
import {
  ThunderboltOutlined,
  SoundOutlined,
  ScissorOutlined,
  SettingOutlined,
  AppstoreOutlined,
  CustomerServiceOutlined,
} from '@ant-design/icons'

import { useI18n } from '../i18n'

const NAV_ITEMS = [
  { key: '/train', navKey: 'nav.train' as const, icon: <ThunderboltOutlined /> },
  { key: '/uvr',   navKey: 'nav.uvr' as const, icon: <ScissorOutlined /> },
  { key: '/infer', navKey: 'nav.infer' as const, icon: <SoundOutlined /> },
  { key: '/live', navKey: 'nav.live' as const, icon: <CustomerServiceOutlined /> },
  { key: '/models', navKey: 'nav.models' as const, icon: <AppstoreOutlined /> },
  { key: '/settings', navKey: 'nav.settings' as const, icon: <SettingOutlined /> },
]

const isMac = navigator.platform.toUpperCase().indexOf('MAC') >= 0

export default function TitleBar() {
  const location = useLocation()
  const { t } = useI18n()
  const active = location.pathname
  const [maximized, setMaximized] = useState(false)

  useEffect(() => {
    const win = getCurrentWindow()
    win.isMaximized().then(setMaximized).catch(() => {})
    const unsub = win.onResized(() => {
      win.isMaximized().then(setMaximized).catch(() => {})
    })
    return () => { unsub.then(fn => fn()).catch(() => {}) }
  }, [])

  const handleMinimize = () => { getCurrentWindow().minimize() }
  const handleMaximize = () => { getCurrentWindow().toggleMaximize() }
  const handleClose = () => { getCurrentWindow().close() }

  return (
    <div data-tauri-drag-region style={{
      position: 'sticky',
      top: 0,
      zIndex: 100,
      display: 'flex',
      alignItems: 'center',
      height: 48,
      padding: isMac ? '0 12px 0 0' : '0 12px',
      background: 'rgba(13,17,23,0.92)',
      backdropFilter: 'blur(16px)',
      WebkitBackdropFilter: 'blur(16px)',
      borderBottom: '1px solid var(--color-border-subtle)',
      userSelect: 'none',
    }}>
      {/* macOS traffic lights */}
      {isMac && (
        <div style={{
          display: 'flex',
          gap: 8,
          paddingLeft: 14,
          paddingRight: 16,
          flexShrink: 0,
          alignItems: 'center',
        }}>
          <WindowButton color="#ff5f57" onClick={handleClose} label="关闭" />
          <WindowButton color="#febc2e" onClick={handleMinimize} label="最小化" />
          <WindowButton color="#28c840" onClick={handleMaximize} label={maximized ? '还原' : '最大化'} />
        </div>
      )}

      {/* Logo */}
      <Link to="/train" style={{
        display: 'flex', alignItems: 'center', gap: 8,
        textDecoration: 'none', flexShrink: 0, marginRight: 28,
      }}>
        <div style={{
          width: 26, height: 26, borderRadius: 7,
          background: 'linear-gradient(135deg, #7c3aed 0%, #a78bfa 100%)',
          display: 'flex', alignItems: 'center', justifyContent: 'center',
          fontSize: 12, fontWeight: 700, color: '#fff',
          boxShadow: '0 2px 8px rgba(124,58,237,0.4)',
        }}>
          R
        </div>
        <span style={{
          fontSize: 14, fontWeight: 600,
          color: 'var(--color-text-primary)',
          letterSpacing: '-0.01em',
        }}>
          Voxril
        </span>
      </Link>

      {/* Nav */}
      <nav style={{
        display: 'flex', alignItems: 'center', gap: 2, flex: 1,
      }}>
        {NAV_ITEMS.map(item => {
          const isActive = active === item.key
          return (
            <Link
              key={item.key}
              to={item.key}
              style={{
                display: 'flex', alignItems: 'center', gap: 6,
                padding: '5px 10px', borderRadius: 'var(--radius-md)',
                fontSize: 12.5, fontWeight: isActive ? 600 : 400,
                color: isActive ? 'var(--color-primary-hover)' : 'var(--color-text-secondary)',
                background: isActive ? 'var(--color-primary-subtle)' : 'transparent',
                textDecoration: 'none',
                transition: 'var(--transition-fast)',
                border: isActive ? '1px solid var(--color-primary-border)' : '1px solid transparent',
              }}
            >
              <span style={{ fontSize: 12, opacity: isActive ? 1 : 0.65 }}>{item.icon}</span>
              {t(item.navKey)}
            </Link>
          )
        })}
      </nav>

      {/* Windows/Linux window controls */}
      {!isMac && (
        <div style={{ display: 'flex', gap: 0, flexShrink: 0, marginLeft: 8 }}>
          <WinButton onClick={handleMinimize}>─</WinButton>
          <WinButton onClick={handleMaximize}>{maximized ? '❐' : '□'}</WinButton>
          <WinButton onClick={handleClose} hoverColor="#e81123">✕</WinButton>
        </div>
      )}
    </div>
  )
}

function WindowButton({ color, onClick, label }: { color: string; onClick: () => void; label: string }) {
  const [hover, setHover] = useState(false)
  return (
    <div
      role="button"
      aria-label={label}
      onClick={onClick}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      style={{
        width: 12, height: 12, borderRadius: '50%',
        background: color,
        cursor: 'pointer',
        display: 'flex', alignItems: 'center', justifyContent: 'center',
        fontSize: 8, lineHeight: 1, color: 'rgba(0,0,0,0.5)',
        fontWeight: 700,
        transition: 'opacity 0.1s',
      }}
    >
      {hover && (label === '关闭' ? '✕' : label === '最小化' ? '─' : '▢')}
    </div>
  )
}

function WinButton({ onClick, children, hoverColor }: {
  onClick: () => void
  children: React.ReactNode
  hoverColor?: string
}) {
  const [hover, setHover] = useState(false)
  return (
    <div
      role="button"
      onClick={onClick}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      style={{
        width: 44, height: 48,
        display: 'flex', alignItems: 'center', justifyContent: 'center',
        cursor: 'pointer',
        fontSize: 11, color: hover ? '#fff' : 'var(--color-text-secondary)',
        background: hover ? (hoverColor || 'var(--color-bg-hover)') : 'transparent',
        transition: 'var(--transition-fast)',
      }}
    >
      {children}
    </div>
  )
}
