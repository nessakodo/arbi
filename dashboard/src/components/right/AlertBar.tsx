import type { DashboardSnapshot } from '../../api/types'

interface AlertBarProps {
  snapshot: DashboardSnapshot | null
  sseConnected: boolean
}

export function AlertBar({ snapshot, sseConnected }: AlertBarProps) {
  const alerts: { message: string; variant: 'warning' | 'error' | 'info' }[] = []

  if (!sseConnected) {
    alerts.push({ message: 'SSE disconnected — retrying...', variant: 'error' })
  }

  if (snapshot && !snapshot.ws_connected) {
    alerts.push({ message: 'WebSocket disconnected from Starknet node', variant: 'error' })
  }

  if (snapshot && snapshot.gas_prices.l1_gas_price > 100_000_000_000) {
    alerts.push({ message: 'L1 gas price elevated — profits may be squeezed', variant: 'warning' })
  }

  if (snapshot && snapshot.counters.opportunities_above_threshold === 0 && snapshot.counters.batches_evaluated > 10) {
    alerts.push({ message: 'No opportunities above threshold yet', variant: 'info' })
  }

  if (alerts.length === 0) return null

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 'var(--space-sm)' }}>
      {alerts.map((a, i) => (
        <div key={i} class={`alert alert--${a.variant}`}>
          {a.message}
        </div>
      ))}
    </div>
  )
}
