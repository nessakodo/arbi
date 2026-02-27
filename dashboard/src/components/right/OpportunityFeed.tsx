import type { OpportunityRecord } from '../../api/types'
import { formatTime } from '../../util'

interface OpportunityFeedProps {
  opportunities: OpportunityRecord[]
}

export function OpportunityFeed({ opportunities }: OpportunityFeedProps) {
  if (opportunities.length === 0) {
    return (
      <div class="card">
        <div class="card__title">Live Opportunities</div>
        <div class="empty-state">Scanning for opportunities...</div>
      </div>
    )
  }

  return (
    <div class="card">
      <div class="card__title">Live Opportunities</div>
      {opportunities.slice(0, 30).map((opp, i) => (
        <div class="feed-item" key={i}>
          <div style={{ flex: 1 }}>
            <div class="feed-item__profit" style={{
              color: opp.profit_hbip >= 0 ? 'var(--color-positive)' : 'var(--color-negative)'
            }}>
              {opp.profit_hbip >= 0 ? '+' : ''}{opp.profit_hbip} hbip
            </div>
            <div class="feed-item__path">{opp.path_display}</div>
            <div class="feed-item__meta">
              {formatTime(opp.timestamp_ms)} &middot; {opp.token} &middot; {opp.hop_count}h
              {opp.executed && opp.tx_hash && (
                <> &middot; <a href={`https://starkscan.co/tx/${opp.tx_hash}`} target="_blank" rel="noopener">tx</a></>
              )}
            </div>
          </div>
          {opp.executed && (
            <span class="chip chip--positive" style={{ fontSize: 10, padding: '2px 6px' }}>EXEC</span>
          )}
        </div>
      ))}
    </div>
  )
}
