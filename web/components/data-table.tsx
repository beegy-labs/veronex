'use client'

import { Card } from '@/components/ui/card'
import { Table } from '@/components/ui/table'

/**
 * DataTable — SSOT wrapper for all data tables in the app.
 *
 * Encapsulates the Card + horizontal scroll + min-width pattern so each
 * page only declares columns and rows.
 *
 * Usage:
 *   <DataTable minWidth="700px">
 *     <TableHeader>...</TableHeader>
 *     <TableBody>...</TableBody>
 *   </DataTable>
 */
export function DataTable({
  children,
  minWidth = '600px',
  footer,
}: {
  children: React.ReactNode
  minWidth?: string
  /** Optional footer row rendered below the table inside the same Card (e.g. pagination). */
  footer?: React.ReactNode
}) {
  return (
    <Card>
      <div className="vds-overflow-x-auto">
        <Table style={{ minWidth }}>
          {children}
        </Table>
      </div>
      {footer && (
        <div className="vds-border-t-1 vds-border-subtle">
          {footer}
        </div>
      )}
    </Card>
  )
}

/**
 * DataTableEmpty — empty-state placeholder, same Card shell as DataTable.
 */
export function DataTableEmpty({ children }: { children: React.ReactNode }) {
  return (
    <Card>
      <div className="vds-py-12 vds-text-center vds-text-sm vds-text-dim">
        {children}
      </div>
    </Card>
  )
}
