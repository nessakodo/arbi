import type { PnlRecord } from '../../api/types'
import { Table } from '../shared/Table'
import { formatTime, truncateHash } from '../../util'

interface TransactionLogProps {
  pnl: PnlRecord[]
}

export function TransactionLog({ pnl }: TransactionLogProps) {
  const columns = [
    {
      key: 'time',
      label: 'Time',
      render: (r: PnlRecord) => formatTime(r.timestamp_ms),
    },
    {
      key: 'block',
      label: 'Block',
      render: (r: PnlRecord) => `#${r.block}`,
    },
    {
      key: 'profit',
      label: 'Profit',
      render: (r: PnlRecord) => (
        <span style={{ color: r.profit_hbip >= 0 ? 'var(--color-positive)' : 'var(--color-negative)' }}>
          {r.profit_hbip >= 0 ? '+' : ''}{r.profit_hbip} hbip
        </span>
      ),
    },
    {
      key: 'tx',
      label: 'Tx Hash',
      render: (r: PnlRecord) => (
        <a href={`https://starkscan.co/tx/${r.tx_hash}`} target="_blank" rel="noopener">
          {truncateHash(r.tx_hash)}
        </a>
      ),
    },
  ]

  return (
    <div class="card">
      <div class="card__title">Transactions</div>
      <Table columns={columns} data={pnl} emptyMessage="No transactions yet" />
    </div>
  )
}
