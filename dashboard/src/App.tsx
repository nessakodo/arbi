import { Shell } from './components/layout/Shell'
import { Header } from './components/layout/Header'
import { Portfolio } from './components/left/Portfolio'
import { GasPanel } from './components/left/GasPanel'
import { ConfigPanel } from './components/left/ConfigPanel'
import { ProfitChart } from './components/center/ProfitChart'
import { ExecutionLog } from './components/center/ExecutionLog'
import { OpportunityFeed } from './components/right/OpportunityFeed'
import { TransactionLog } from './components/right/TransactionLog'
import { AlertBar } from './components/right/AlertBar'
import { useSnapshot } from './hooks/useSnapshot'
import { useApi } from './hooks/useApi'
import { fetchOpportunities, fetchPnl } from './api/client'

export function App() {
  const { snapshot, connected } = useSnapshot()
  const { data: opportunities } = useApi(() => fetchOpportunities(100), [])
  const { data: pnl } = useApi(() => fetchPnl(100), [])

  const opps = opportunities ?? []
  const pnlData = pnl ?? []

  return (
    <Shell
      header={<Header snapshot={snapshot} sseConnected={connected} />}
      left={
        <>
          <Portfolio snapshot={snapshot} />
          <GasPanel snapshot={snapshot} />
          <ConfigPanel snapshot={snapshot} />
        </>
      }
      center={
        <>
          <ProfitChart opportunities={opps} />
          <ExecutionLog pnl={pnlData} />
        </>
      }
      right={
        <>
          <AlertBar snapshot={snapshot} sseConnected={connected} />
          <OpportunityFeed opportunities={opps} />
          <TransactionLog pnl={pnlData} />
        </>
      }
    />
  )
}
