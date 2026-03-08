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
      <div className="overflow-x-auto">
        <Table style={{ minWidth }}>
          {children}
        </Table>
      </div>
      {footer && (
        <div className="border-t border-border">
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
      <div className="py-12 text-center text-sm text-muted-foreground">
        {children}
      </div>
    </Card>
  )
}
