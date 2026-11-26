import { Layout } from 'antd'
import TitleBar from './TitleBar'
import UpdateBanner from './UpdateBanner'
import OnboardingBanner from './OnboardingBanner'

const { Content } = Layout

interface Props {
  children: React.ReactNode
}

export default function AppLayout({ children }: Props) {
  return (
    <Layout style={{ minHeight: '100vh', background: 'var(--color-bg-app)' }}>
      <TitleBar />

      <Content style={{
        padding: '28px 32px',
        maxWidth: 1200,
        margin: '0 auto',
        width: '100%',
      }}>
        <UpdateBanner />
        <OnboardingBanner />
        {children}
      </Content>
    </Layout>
  )
}
