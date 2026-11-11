import { useEffect, useRef } from 'react'

interface Props {
  content: string
  height?: number
}

const epochPat = /^.*====> *Epoch:/

function getLineColor(line: string): string {
  if (line.includes('Error') || line.includes('error') || line.startsWith('Traceback') || line.startsWith('  File ')) return '#f87171'
  if (epochPat.test(line)) return '#4ade80'
  if (line.startsWith('✓') || line.includes('完成') || line.includes('done')) return '#86efac'
  if (line.startsWith('[') && line.includes(']')) return '#a78bfa'
  return '#94a3b8'
}

export default function LogViewer({ content, height = 300 }: Props) {
  const ref = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (ref.current) ref.current.scrollTop = ref.current.scrollHeight
  }, [content])

  const lines = content ? content.split('\n') : []

  return (
    <div
      ref={ref}
      style={{
        height,
        overflowY: 'auto',
        background: '#07090f',
        borderRadius: 8,
        padding: '10px 14px',
        fontFamily: "var(--font-mono, 'JetBrains Mono', 'Fira Code', Menlo, monospace)",
        fontSize: 11.5,
        lineHeight: 1.65,
        border: '1px solid rgba(255,255,255,0.06)',
      }}
    >
      {lines.length > 0 ? lines.map((line, i) => {
        const isEpoch = epochPat.test(line)
        return (
          <div key={i}>
            {isEpoch && i > 0 && (
              <div style={{ borderTop: '1px solid rgba(255,255,255,0.05)', margin: '5px 0 3px' }} />
            )}
            <div style={{
              color: getLineColor(line),
              fontWeight: isEpoch ? 600 : 400,
              background: isEpoch ? 'rgba(34,197,94,0.05)' : 'transparent',
              paddingLeft: isEpoch ? 6 : 0,
              borderRadius: 3,
            }}>
              {line || '\u00a0'}
            </div>
          </div>
        )
      }) : (
        <div style={{ color: '#475569', fontSize: 12, fontStyle: 'italic' }}>暂无日志输出</div>
      )}
    </div>
  )
}
