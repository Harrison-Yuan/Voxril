import { BrowserRouter, Routes, Route, Navigate, useNavigate, useLocation } from 'react-router-dom'
import { ConfigProvider, theme } from 'antd'
import zhCN from 'antd/locale/zh_CN'
import enUS from 'antd/locale/en_US'
import { useEffect } from 'react'
import { listen } from '@tauri-apps/api/event'
import './styles/antd-overrides.css'
import AppLayout from './components/AppLayout'
import TrainPage from './pages/Train'
import InferPage from './pages/Infer'
import UVRPage from './pages/UVR'
import LiveSingingPage from './pages/LiveSinging'
import DesktopLyricsPage from './pages/DesktopLyrics'
import SettingsPage from './pages/Settings'
import ModelsPage from './pages/Models'
import { antdTokens, antdComponentTokens } from './styles/tokens'
import { LocaleProvider, useI18n, type Lang } from './i18n'

const antdLocales: Record<Lang, any> = { zh: zhCN, en: enUS }

/** 监听 Tauri 后端事件（navigate / dock-focused）并执行路由跳转
 *  仅在主窗口生效，桌面歌词窗口不响应 */
function TauriEventRouter() {
  const navigate = useNavigate()
  const location = useLocation()
  useEffect(() => {
    // ⚠️ 桌面歌词窗口不响应路由跳转事件
    if (location.pathname === '/desktop-lyrics') return
    const unlistens: Array<() => void> = []
    listen<string>('navigate', e => navigate(e.payload))
      .then(fn => unlistens.push(fn))
    return () => unlistens.forEach(fn => fn())
  }, [location.pathname])
  return null
}

function AppInner() {
  const { lang } = useI18n()

  return (
    <ConfigProvider
      locale={antdLocales[lang]}
      theme={{
        algorithm: theme.darkAlgorithm,
        token: antdTokens,
        components: antdComponentTokens,
      }}
    >
      <BrowserRouter>
        <TauriEventRouter />
        <Routes>
          {/* ⚠️ 桌面歌词路由必须排第一，不共享任何 catch-all
               否则 URL 匹配失败时落到 * 渲染整个 AppLayout（侧边栏+顶栏） */}
          <Route path="/desktop-lyrics" element={<DesktopLyricsPage />} />
          <Route path="*" element={
            <AppLayout>
              <Routes>
                <Route path="/" element={<Navigate to="/live" replace />} />
                <Route path="/train" element={<TrainPage />} />
                <Route path="/infer" element={<InferPage />} />
                <Route path="/uvr" element={<UVRPage />} />
                <Route path="/live" element={<LiveSingingPage />} />
                <Route path="/models" element={<ModelsPage />} />
                <Route path="/settings" element={<SettingsPage />} />
                <Route path="*" element={<Navigate to="/live" replace />} />
              </Routes>
            </AppLayout>
          } />
        </Routes>
      </BrowserRouter>
    </ConfigProvider>
  )
}

export default function App() {
  return (
    <LocaleProvider>
      <AppInner />
    </LocaleProvider>
  )
}
