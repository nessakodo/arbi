export function formatUptime(secs: number): string {
  if (secs < 60) return `${secs}s`
  if (secs < 3600) return `${Math.floor(secs / 60)}m ${secs % 60}s`
  const h = Math.floor(secs / 3600)
  const m = Math.floor((secs % 3600) / 60)
  return `${h}h ${m}m`
}

export function formatTime(ms: number): string {
  if (ms === 0) return '--'
  return new Date(ms).toLocaleTimeString([], {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  })
}

export function formatGwei(wei: number): string {
  if (wei === 0) return '0'
  const gwei = wei / 1e9
  if (gwei < 0.01) return `${(wei / 1e6).toFixed(2)} Mwei`
  if (gwei > 1000) return `${(gwei / 1000).toFixed(2)}k Gwei`
  return `${gwei.toFixed(2)} Gwei`
}

export function truncateHash(hash: string): string {
  if (hash.length <= 12) return hash
  return `${hash.slice(0, 6)}...${hash.slice(-4)}`
}
