import { useEffect, useRef } from 'react'

/** 与品牌图标一致的渐变停靠色 */
const GRADIENT: Array<[number, string]> = [
  [0, '#7C3AED'],
  [0.55, '#6366F1'],
  [1, '#22D3EE'],
]

function roundedRectPath(
  ctx: CanvasRenderingContext2D,
  x: number, y: number, w: number, h: number, r: number,
) {
  ctx.beginPath()
  ctx.moveTo(x + r, y)
  ctx.arcTo(x + w, y, x + w, y + h, r)
  ctx.arcTo(x + w, y + h, x, y + h, r)
  ctx.arcTo(x, y + h, x, y, r)
  ctx.arcTo(x, y, x + w, y, r)
  ctx.closePath()
}

/**
 * 品牌标记：圆角渐变底 + canvas 绘制的「呼吸的嘴」。
 *
 * 打包图标是静态首字母 V（见 tools/gen_icon.py），此处用动态嘴强化
 * 「AI 演唱」的产品语义。reduce-motion 或页面不可见时自动降级/暂停。
 */
export function VoxMark({ size = 26 }: { size?: number }) {
  const ref = useRef<HTMLCanvasElement>(null)

  useEffect(() => {
    const canvas = ref.current
    const ctx = canvas?.getContext('2d')
    if (!canvas || !ctx) return

    const dpr = Math.min(window.devicePixelRatio || 1, 3)
    canvas.width = Math.round(size * dpr)
    canvas.height = Math.round(size * dpr)
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0)

    const reduceMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches

    const paint = (open: number) => {
      ctx.clearRect(0, 0, size, size)

      // 圆角渐变底
      const grad = ctx.createLinearGradient(0, 0, size, size)
      for (const [offset, color] of GRADIENT) grad.addColorStop(offset, color)
      roundedRectPath(ctx, 0, 0, size, size, size * 0.225)
      ctx.fillStyle = grad
      ctx.fill()

      // 顶部柔光，与静态图标同一套质感
      const sheen = ctx.createLinearGradient(0, 0, 0, size)
      sheen.addColorStop(0, 'rgba(255,255,255,0.20)')
      sheen.addColorStop(0.55, 'rgba(255,255,255,0)')
      ctx.fillStyle = sheen
      ctx.fill()

      // 嘴：上下两条二次曲线围成的透镜形，随 open 张开
      const cx = size / 2
      const cy = size * 0.55
      const w = size * 0.46
      const h = size * (0.06 + open * 0.26)
      ctx.beginPath()
      ctx.moveTo(cx - w / 2, cy)
      ctx.quadraticCurveTo(cx, cy - h, cx + w / 2, cy)
      ctx.quadraticCurveTo(cx, cy + h, cx - w / 2, cy)
      ctx.closePath()
      ctx.fillStyle = '#FFFFFF'
      ctx.fill()
    }

    if (reduceMotion) {
      paint(0.45)
      return
    }

    let raf = 0
    let last = 0
    const start = performance.now()

    const loop = (now: number) => {
      // 限帧 30fps：常驻标题栏，避免不必要的高频重绘
      if (now - last >= 1000 / 30) {
        last = now
        const t = (now - start) / 1000
        paint(0.32 + 0.30 * (0.5 + 0.5 * Math.sin(t * 2.4)))
      }
      raf = requestAnimationFrame(loop)
    }
    raf = requestAnimationFrame(loop)

    const onVisibility = () => {
      cancelAnimationFrame(raf)
      if (!document.hidden) raf = requestAnimationFrame(loop)
    }
    document.addEventListener('visibilitychange', onVisibility)

    return () => {
      cancelAnimationFrame(raf)
      document.removeEventListener('visibilitychange', onVisibility)
    }
  }, [size])

  return <canvas ref={ref} style={{ width: size, height: size, display: 'block' }} />
}
