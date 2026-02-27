import { useRef, useEffect } from 'preact/hooks'
import type { OpportunityRecord } from '../../api/types'

interface ProfitChartProps {
  opportunities: OpportunityRecord[]
}

export function ProfitChart({ opportunities }: ProfitChartProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null)

  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas || opportunities.length === 0) return

    const ctx = canvas.getContext('2d')
    if (!ctx) return

    const dpr = window.devicePixelRatio || 1
    const rect = canvas.getBoundingClientRect()
    canvas.width = rect.width * dpr
    canvas.height = rect.height * dpr
    ctx.scale(dpr, dpr)

    const w = rect.width
    const h = rect.height
    const pad = { top: 20, right: 20, bottom: 30, left: 60 }
    const plotW = w - pad.left - pad.right
    const plotH = h - pad.top - pad.bottom

    // Data sorted by time ascending
    const sorted = [...opportunities].sort((a, b) => a.timestamp_ms - b.timestamp_ms)
    const profits = sorted.map((o) => o.profit_hbip)
    const times = sorted.map((o) => o.timestamp_ms)

    const minP = Math.min(0, ...profits)
    const maxP = Math.max(1, ...profits)
    const minT = times[0]
    const maxT = times[times.length - 1]
    const rangeT = maxT - minT || 1
    const rangeP = maxP - minP || 1

    // Clear
    ctx.clearRect(0, 0, w, h)

    // Grid lines
    ctx.strokeStyle = 'rgba(100, 200, 255, 0.06)'
    ctx.lineWidth = 1
    for (let i = 0; i <= 4; i++) {
      const y = pad.top + (plotH / 4) * i
      ctx.beginPath()
      ctx.moveTo(pad.left, y)
      ctx.lineTo(w - pad.right, y)
      ctx.stroke()
    }

    // Zero line
    const zeroY = pad.top + plotH - ((0 - minP) / rangeP) * plotH
    ctx.strokeStyle = 'rgba(100, 200, 255, 0.15)'
    ctx.setLineDash([4, 4])
    ctx.beginPath()
    ctx.moveTo(pad.left, zeroY)
    ctx.lineTo(w - pad.right, zeroY)
    ctx.stroke()
    ctx.setLineDash([])

    // Plot points and line
    ctx.strokeStyle = '#7dd3fc'
    ctx.lineWidth = 1.5
    ctx.beginPath()
    sorted.forEach((_, i) => {
      const x = pad.left + ((times[i] - minT) / rangeT) * plotW
      const y = pad.top + plotH - ((profits[i] - minP) / rangeP) * plotH
      if (i === 0) ctx.moveTo(x, y)
      else ctx.lineTo(x, y)
    })
    ctx.stroke()

    // Dots
    sorted.forEach((opp, i) => {
      const x = pad.left + ((times[i] - minT) / rangeT) * plotW
      const y = pad.top + plotH - ((profits[i] - minP) / rangeP) * plotH
      ctx.fillStyle = opp.profit_hbip >= 0 ? '#4ade80' : '#f87171'
      ctx.beginPath()
      ctx.arc(x, y, 3, 0, Math.PI * 2)
      ctx.fill()
    })

    // Y-axis labels
    ctx.fillStyle = '#64748b'
    ctx.font = '11px "Space Mono", monospace'
    ctx.textAlign = 'right'
    for (let i = 0; i <= 4; i++) {
      const val = maxP - (rangeP / 4) * i
      const y = pad.top + (plotH / 4) * i
      ctx.fillText(`${val.toFixed(0)}`, pad.left - 8, y + 4)
    }

    // X-axis label
    ctx.fillStyle = '#64748b'
    ctx.textAlign = 'center'
    ctx.fillText('hbip', pad.left - 8, pad.top - 6)
  }, [opportunities])

  if (opportunities.length === 0) {
    return (
      <div class="card">
        <div class="card__title">Profit Over Time</div>
        <div class="empty-state">Waiting for opportunities...</div>
      </div>
    )
  }

  return (
    <div class="card">
      <div class="card__title">Profit Over Time (hbip)</div>
      <div class="chart-container">
        <canvas
          ref={canvasRef}
          style={{ width: '100%', height: '100%' }}
        />
      </div>
    </div>
  )
}
