import type { PnlRecord } from '../../api/types'
import { Table } from '../shared/Table'
import { formatTime, truncateHash } from '../../util'

interface ExecutionLogProps {
  pnl: PnlRecord[]
}

export function ExecutionLog({ pnl }: ExecutionLogProps) {
  const columns = [
    {
      key: 'time',
      label: 'Time',
      render: (r: PnlRecord) => formatTime(r.timestamp_ms),
    },
    {
      key: 'token',
      label: 'Token',
      render: (r: PnlRecord) => r.token,
    },
    {
      key: 'profit',
      label: 'Profit (hbip)',
      render: (r: PnlRecord) => (
        <span style={{ color: r.profit_hbip >= 0 ? 'var(--color-positive)' : 'var(--color-negative)' }}>
          {r.profit_hbip >= 0 ? '+' : ''}{r.profit_hbip}
        </span>
      ),
    },
    {
      key: 'status',
      label: 'Status',
      render: (r: PnlRecord) => (
        <span style={{ color: r.success ? 'var(--color-positive)' : 'var(--color-negative)' }}>
          {r.success ? 'OK' : 'FAIL'}
        </span>
      ),
    },
    {
      key: 'tx',
      label: 'Tx',
      render: (r: PnlRecord) => (
        <a
          href={`https://starkscan.co/tx/${r.tx_hash}`}
          target="_blank"
          rel="noopener"
        >
          {truncateHash(r.tx_hash)}
        </a>
      ),
    },
  ]

  return (
    <div class="card">
      <div class="card__title">Execution Log</div>
      <Table columns={columns} data={pnl} emptyMessage="No executions yet" />
    </div>
  )
}
