import { useId } from 'react'

/**
 * 品牌标记：圆角渐变底 + 居中白色首字母 V。
 *
 * 与打包图标（tools/gen_icon.py）同源——同一组渐变停靠色、圆角比例、
 * V 顶点与笔画宽度，界面里的 mark 与 Dock / 桌面图标是同一个设计。
 * 用 SVG 而不是位图/canvas，任意 DPI 下都清晰。
 */
const VIEW = 64
const CORNER = 0.2255 * VIEW     // 圆角半径（与生成脚本 CORNER_RATIO 一致）
const STROKE = 0.088 * VIEW      // 笔画宽度（与生成脚本 STROKE_RATIO 一致）
const CENTER = VIEW / 2

const V_LEFT = [CENTER - 0.255 * VIEW, CENTER - 0.205 * VIEW] as const
const V_RIGHT = [CENTER + 0.255 * VIEW, CENTER - 0.205 * VIEW] as const
const V_BOTTOM = [CENTER, CENTER + 0.235 * VIEW] as const

const V_PATH = `M${V_LEFT[0]} ${V_LEFT[1]} L${V_BOTTOM[0]} ${V_BOTTOM[1]} L${V_RIGHT[0]} ${V_RIGHT[1]}`

/** 与品牌图标一致的渐变停靠色 */
const GRADIENT: Array<[number, string]> = [
  [0, '#7C3AED'],
  [0.55, '#6366F1'],
  [1, '#22D3EE'],
]

export function VoxMark({ size = 26 }: { size?: number }) {
  // useId 可能含冒号等字符，放进 url(#id) 前先净化
  const gid = `vox-grad-${useId().replace(/[^a-zA-Z0-9_-]/g, '')}`

  return (
    <svg
      width={size}
      height={size}
      viewBox={`0 0 ${VIEW} ${VIEW}`}
      aria-hidden="true"
      style={{ display: 'block' }}
    >
      <defs>
        <linearGradient id={gid} x1="0" y1="0" x2={VIEW} y2={VIEW} gradientUnits="userSpaceOnUse">
          {GRADIENT.map(([offset, color]) => (
            <stop key={offset} offset={offset} stopColor={color} />
          ))}
        </linearGradient>
      </defs>
      <rect width={VIEW} height={VIEW} rx={CORNER} fill={`url(#${gid})`} />
      <path
        d={V_PATH}
        stroke="#FFFFFF"
        strokeWidth={STROKE}
        strokeLinecap="round"
        strokeLinejoin="round"
        fill="none"
      />
    </svg>
  )
}
