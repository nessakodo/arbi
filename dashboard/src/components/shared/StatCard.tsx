interface StatCardProps {
  label: string
  value: string | number
  variant?: 'default' | 'positive' | 'negative'
}

export function StatCard({ label, value, variant = 'default' }: StatCardProps) {
  const valueClass =
    variant === 'positive'
      ? 'stat-card__value stat-card__value--positive'
      : variant === 'negative'
        ? 'stat-card__value stat-card__value--negative'
        : 'stat-card__value'

  return (
    <div class="stat-card">
      <div class={valueClass}>{value}</div>
      <div class="stat-card__label">{label}</div>
    </div>
  )
}
