import type { DashboardSnapshot, OpportunityRecord, PnlRecord } from './types'

const BASE = ''

export async function fetchSnapshot(): Promise<DashboardSnapshot> {
  const res = await fetch(`${BASE}/api/snapshot`)
  if (!res.ok) throw new Error(`snapshot: ${res.status}`)
  return res.json()
}

export async function fetchOpportunities(limit = 50): Promise<OpportunityRecord[]> {
  const res = await fetch(`${BASE}/api/opportunities?limit=${limit}`)
  if (!res.ok) throw new Error(`opportunities: ${res.status}`)
  return res.json()
}

export async function fetchPnl(limit = 100): Promise<PnlRecord[]> {
  const res = await fetch(`${BASE}/api/pnl?limit=${limit}`)
  if (!res.ok) throw new Error(`pnl: ${res.status}`)
  return res.json()
}

export function createEventSource(onSnapshot: (s: DashboardSnapshot) => void): EventSource {
  const es = new EventSource(`${BASE}/api/events`)

  es.addEventListener('snapshot', (e: MessageEvent) => {
    try {
      const data = JSON.parse(e.data) as DashboardSnapshot
      onSnapshot(data)
    } catch {
      // skip malformed events
    }
  })

  return es
}
