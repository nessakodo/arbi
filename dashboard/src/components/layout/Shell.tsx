import type { ComponentChildren } from 'preact'

interface ShellProps {
  header: ComponentChildren
  left: ComponentChildren
  center: ComponentChildren
  right: ComponentChildren
}

export function Shell({ header, left, center, right }: ShellProps) {
  return (
    <div class="shell">
      {header}
      <main class="main">
        <div class="column">{left}</div>
        <div class="column column--center">{center}</div>
        <div class="column">{right}</div>
      </main>
    </div>
  )
}
