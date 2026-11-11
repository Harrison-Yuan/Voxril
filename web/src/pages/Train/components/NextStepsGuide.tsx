import { Card, Space, Typography, Button } from 'antd'
import { Link } from 'react-router-dom'

const { Text } = Typography

export const NextStepsGuide = () => {
  return (
    <Card size="small" title="后续步骤">
      <Space direction="vertical" size="small">
        <Text>1. 训练到足够轮次后，先构建特征索引。</Text>
        <Text>2. 然后去「人声分离」页面，把目标歌曲拆成人声和伴奏。</Text>
        <Text>3. 最后去「语音转换」页面，将分离出的人声转换成目标音色。</Text>
        <Space wrap>
          <Button>
            <Link to="/uvr">前往人声分离</Link>
          </Button>
          <Button type="link">
            <Link to="/infer">直接去语音转换</Link>
          </Button>
        </Space>
      </Space>
    </Card>
  )
}
