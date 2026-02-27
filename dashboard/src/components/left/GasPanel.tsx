import type { DashboardSnapshot } from '../../api/types'
import { formatGwei } from '../../util'

interface GasPanelProps {
  snapshot: DashboardSnapshot | null
}

export function GasPanel({ snapshot }: GasPanelProps) {
  const gas = snapshot?.gas_prices

  return (
    <div class="card">
      <div class="card__title">Gas Prices</div>
      <div class="gas-row">
        <span class="gas-row__label">L1 Gas</span>
        <span class="gas-row__value">{gas ? formatGwei(gas.l1_gas_price) : '--'}</span>
      </div>
      <div class="gas-row">
        <span class="gas-row__label">L2 Gas</span>
        <span class="gas-row__value">{gas ? formatGwei(gas.l2_gas_price) : '--'}</span>
      </div>
      <div class="gas-row">
        <span class="gas-row__label">L1 Data Gas</span>
        <span class="gas-row__value">
          {gas ? formatGwei(gas.l1_data_gas_price) : '--'}
        </span>
      </div>
      {gas && gas.block_number > 0 && (
        <div class="gas-row">
          <span class="gas-row__label">As of Block</span>
          <span class="gas-row__value">#{gas.block_number.toLocaleString()}</span>
        </div>
      )}
    </div>
  )
}
