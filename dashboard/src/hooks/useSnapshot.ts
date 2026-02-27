import { useState, useEffect, useRef } from 'preact/hooks'
import type { DashboardSnapshot } from '../api/types'
import { createEventSource, fetchSnapshot } from '../api/client'

export function useSnapshot() {
  const [snapshot, setSnapshot] = useState<DashboardSnapshot | null>(null)
  const [connected, setConnected] = useState(false)
  const esRef = useRef<EventSource | null>(null)

  useEffect(() => {
    // Load initial snapshot
    fetchSnapshot().then(setSnapshot).catch(() => {})

    let reconnectTimer: ReturnType<typeof setTimeout>

    function connect() {
      if (esRef.current) {
        esRef.current.close()
      }

      const es = createEventSource((s) => {
        setSnapshot(s)
        setConnected(true)
      })

      es.onopen = () => setConnected(true)

      es.onerror = () => {
        setConnected(false)
        es.close()
        reconnectTimer = setTimeout(connect, 3000)
      }

      esRef.current = es
    }

    connect()

    return () => {
      clearTimeout(reconnectTimer)
      if (esRef.current) {
        esRef.current.close()
      }
    }
  }, [])

  return { snapshot, connected }
}
