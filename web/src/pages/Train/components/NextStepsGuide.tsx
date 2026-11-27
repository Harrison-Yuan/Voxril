import { Card, Space, Typography, Button } from 'antd'
import { Link } from 'react-router-dom'
import { useI18n } from '@/i18n'

const { Text } = Typography

export const NextStepsGuide = () => {
  const { t } = useI18n()
  return (
    <Card size="small" title={t('trainStep.next_steps_title')}>
      <Space direction="vertical" size="small">
        <Text>{t('trainStep.next_step1')}</Text>
        <Text>{t('trainStep.next_step2')}</Text>
        <Text>{t('trainStep.next_step3')}</Text>
        <Space wrap>
          <Button>
            <Link to="/uvr">{t('trainStep.go_uvr')}</Link>
          </Button>
          <Button type="link">
            <Link to="/infer">{t('uvr.go_infer')}</Link>
          </Button>
        </Space>
      </Space>
    </Card>
  )
}
