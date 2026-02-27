import type { DashboardSnapshot } from '../../api/types'

interface ConfigPanelProps {
  snapshot: DashboardSnapshot | null
}

export function ConfigPanel({ snapshot }: ConfigPanelProps) {
  const config = snapshot?.config

  return (
    <div class="card">
      <div class="card__title">Configuration</div>
      <div class="config-row">
        <span class="config-row__label">Min Profit</span>
        <span class="config-row__value">
          {config ? `${config.min_profit_hbip} hbip` : '--'}
        </span>
      </div>
      <div class="config-row">
        <span class="config-row__label">Tip</span>
        <span class="config-row__value">
          {config ? `${config.tip_percentage}%` : '--'}
        </span>
      </div>
      <div class="config-row">
        <span class="config-row__label">Max Hops</span>
        <span class="config-row__value">{config?.max_hops ?? '--'}</span>
      </div>
      <div class="config-row">
        <span class="config-row__label">Broadcast</span>
        <span class="config-row__value">
          {config ? (config.broadcast ? 'ON' : 'OFF') : '--'}
        </span>
      </div>
      <div class="config-row">
        <span class="config-row__label">Pools</span>
        <span class="config-row__value">
          {snapshot?.pool_count?.toLocaleString() ?? '--'}
        </span>
      </div>
      <div class="config-row">
        <span class="config-row__label">Paths</span>
        <span class="config-row__value">
          {snapshot?.path_count?.toLocaleString() ?? '--'}
        </span>
      </div>
      <div class="config-row">
        <span class="config-row__label">Cycle Tokens</span>
        <span class="config-row__value">
          {snapshot?.cycle_token_count ?? '--'}
        </span>
      </div>
    </div>
  )
}
