import type { DashboardSnapshot } from '../../api/types'
import { StatCard } from '../shared/StatCard'

interface PortfolioProps {
  snapshot: DashboardSnapshot | null
}

export function Portfolio({ snapshot }: PortfolioProps) {
  const counters = snapshot?.counters
  return (
    <>
      <StatCard
        label="Opportunities Found"
        value={counters?.opportunities_found?.toLocaleString() ?? '0'}
      />
      <StatCard
        label="Above Threshold"
        value={counters?.opportunities_above_threshold?.toLocaleString() ?? '0'}
        variant={
          (counters?.opportunities_above_threshold ?? 0) > 0 ? 'positive' : 'default'
        }
      />
      <StatCard
        label="Batches Evaluated"
        value={counters?.batches_evaluated?.toLocaleString() ?? '0'}
      />
      <StatCard
        label="Transactions"
        value={counters?.transactions_processed?.toLocaleString() ?? '0'}
      />
    </>
  )
}
