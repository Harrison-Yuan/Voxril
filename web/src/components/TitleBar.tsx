import { useState, useEffect } from 'react'
import { Link, useLocation, useNavigate } from 'react-router-dom'
import { Dropdown } from 'antd'
import { getCurrentWindow } from '@tauri-apps/api/window'
import {
  ThunderboltOutlined,
  SoundOutlined,
  ScissorOutlined,
  SettingOutlined,
  AppstoreOutlined,
  CustomerServiceOutlined,
  PlayCircleOutlined,
  DownOutlined,
  ExperimentOutlined,
} from '@ant-design/icons'

import { useI18n } from '../i18n'
import { VoxMark } from './VoxMark'

/** 主流程入口：一键翻唱打头，训练/直播/模型/设置并列。 */
const PRIMARY_NAV = [
  { key: '/cover', navKey: 'nav.cover', icon: <PlayCircleOutlined /> },
  { key: '/train', navKey: 'nav.train', icon: <ThunderboltOutlined /> },
  { key: '/live',   navKey: 'nav.live', icon: <CustomerServiceOutlined /> },
  { key: '/models', navKey: 'nav.models', icon: <AppstoreOutlined /> },
  { key: '/settings', navKey: 'nav.settings', icon: <SettingOutlined /> },
] as const

/** 手动分步的老流程，收进「高级模式」。 */
const ADVANCED_NAV = [
  { key: '/uvr',   navKey: 'nav.uvr', icon: <ScissorOutlined /> },
  { key: '/infer', navKey: 'nav.infer', icon: <SoundOutlined /> },
] as const

const navItemStyle = (isActive: boolean): React.CSSProperties => ({
  display: 'flex', alignItems: 'center', gap: 6,
  padding: '5px 10px', borderRadius: 'var(--radius-md)',
  fontSize: 12.5, fontWeight: isActive ? 600 : 400,
  color: isActive ? 'var(--color-primary-hover)' : 'var(--color-text-secondary)',
  background: isActive ? 'var(--color-primary-subtle)' : 'transparent',
  textDecoration: 'none',
  transition: 'var(--transition-fast)',
  border: isActive ? '1px solid var(--color-primary-border)' : '1px solid transparent',
  cursor: 'pointer',
})

const isMac = navigator.platform.toUpperCase().indexOf('MAC') >= 0

export default function TitleBar() {
  const location = useLocation()
  const navigate = useNavigate()
  const { t } = useI18n()
  const active = location.pathname
  /** 首页与 `/cover` 是同一个页面，高亮一并处理。 */
  const coverActive = active === '/' || active === '/cover'
  const advancedActive = ADVANCED_NAV.some(item => item.key === active)
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
      <Link to="/" style={{
        display: 'flex', alignItems: 'center', gap: 8,
        textDecoration: 'none', flexShrink: 0, marginRight: 28,
      }}>
        <div style={{ filter: 'drop-shadow(0 2px 6px rgba(99,102,241,0.45))' }}>
          <VoxMark size={26} />
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
        {PRIMARY_NAV.map(item => {
          const isActive = item.key === '/cover' ? coverActive : active === item.key
          return (
            <Link key={item.key} to={item.key} style={navItemStyle(isActive)}>
              <span style={{ fontSize: 12, opacity: isActive ? 1 : 0.65 }}>{item.icon}</span>
              {t(item.navKey)}
            </Link>
          )
        })}
        <Dropdown
          trigger={['click']}
          placement="bottomLeft"
          menu={{
            items: ADVANCED_NAV.map(item => ({
              key: item.key,
              icon: item.icon,
              label: t(item.navKey),
            })),
            selectedKeys: advancedActive ? [active] : [],
            onClick: ({ key }) => navigate(key),
          }}
        >
          <div style={navItemStyle(advancedActive)}>
            <span style={{ fontSize: 12, opacity: advancedActive ? 1 : 0.65 }}><ExperimentOutlined /></span>
            {t('nav.advanced')}
            <DownOutlined style={{ fontSize: 9, opacity: 0.6 }} />
          </div>
        </Dropdown>
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
