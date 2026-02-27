import type { DashboardSnapshot } from '../../api/types'
import { Chip } from '../shared/Chip'
import { formatUptime } from '../../util'

interface HeaderProps {
  snapshot: DashboardSnapshot | null
  sseConnected: boolean
}

export function Header({ snapshot, sseConnected }: HeaderProps) {
  const broadcast = snapshot?.broadcast_enabled ?? false
  const wsConnected = snapshot?.ws_connected ?? false
  const block = snapshot?.current_block ?? 0
  const uptime = snapshot?.counters.uptime_secs ?? 0

  return (
    <header class="header">
      <div class="header__left">
        <span class="header__logo">arbi</span>
        <Chip
          label={broadcast ? 'LIVE' : 'PAPER'}
          variant={broadcast ? 'negative' : 'ice'}
          pulse={broadcast}
        />
        <Chip
          label={wsConnected ? 'WS' : 'WS OFF'}
          variant={wsConnected ? 'positive' : 'muted'}
          pulse={wsConnected}
        />
        {!sseConnected && <Chip label="SSE OFF" variant="warning" />}
      </div>
      <div class="header__right">
        {block > 0 && <Chip label={`#${block.toLocaleString()}`} variant="muted" />}
        <Chip label={formatUptime(uptime)} variant="muted" />
      </div>
    </header>
  )
}
