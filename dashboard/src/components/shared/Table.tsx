import type { ComponentChildren } from 'preact'

interface Column<T> {
  key: string
  label: string
  render: (row: T) => ComponentChildren
}

interface TableProps<T> {
  columns: Column<T>[]
  data: T[]
  emptyMessage?: string
}

export function Table<T>({ columns, data, emptyMessage = 'No data' }: TableProps<T>) {
  if (data.length === 0) {
    return <div class="empty-state">{emptyMessage}</div>
  }

  return (
    <div class="table-wrap">
      <table class="table">
        <thead>
          <tr>
            {columns.map((col) => (
              <th key={col.key}>{col.label}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {data.map((row, i) => (
            <tr key={i}>
              {columns.map((col) => (
                <td key={col.key}>{col.render(row)}</td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}
