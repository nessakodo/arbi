interface ChipProps {
  label: string
  variant?: 'ice' | 'positive' | 'negative' | 'warning' | 'muted'
  pulse?: boolean
}

export function Chip({ label, variant = 'ice', pulse = false }: ChipProps) {
  return (
    <span class={`chip chip--${variant}`}>
      <span class={`chip__dot${pulse ? ' chip__dot--pulse' : ''}`} />
      {label}
    </span>
  )
}
