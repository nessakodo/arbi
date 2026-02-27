export interface GasPriceSnapshot {
  l1_gas_price: number
  l2_gas_price: number
  l1_data_gas_price: number
  block_number: number
}

export interface CounterSnapshot {
  uptime_secs: number
  transactions_processed: number
  reactions_sent: number
  batches_evaluated: number
  opportunities_found: number
  opportunities_above_threshold: number
}

export interface ConfigSnapshot {
  min_profit_hbip: number
  tip_percentage: number
  max_hops: number
  broadcast: boolean
  tokens: string[]
}

export interface DashboardSnapshot {
  timestamp_ms: number
  current_block: number
  ws_connected: boolean
  broadcast_enabled: boolean
  pool_count: number
  path_count: number
  cycle_token_count: number
  gas_prices: GasPriceSnapshot
  counters: CounterSnapshot
  config: ConfigSnapshot
}

export interface OpportunityRecord {
  timestamp_ms: number
  block: number
  token: string
  amount_in: string
  amount_out: string
  profit: number
  profit_hbip: number
  hop_count: number
  path_display: string
  executed: boolean
  tx_hash: string | null
}

export interface PnlRecord {
  timestamp_ms: number
  block: number
  token: string
  profit: number
  profit_hbip: number
  tx_hash: string
  success: boolean
}
