'use client'

import * as React from 'react'
import { cn } from '@/lib/utils'

export const Table = React.forwardRef<HTMLTableElement, React.HTMLAttributes<HTMLTableElement>>(
  ({ className, ...props }, ref) => (
    <div className="vds-relative vds-w-full vds-overflow-auto">
      <table
        ref={ref}
        className={cn('vds-w-full vds-text-sm vds-text-primary', className)}
        style={{ captionSide: 'bottom' }}
        {...props}
      />
    </div>
  ),
)
Table.displayName = 'Table'

export const TableHeader = React.forwardRef<HTMLTableSectionElement, React.HTMLAttributes<HTMLTableSectionElement>>(
  ({ className, ...props }, ref) => (
    <thead
      ref={ref}
      className={cn('[&_tr]:vds-border-b-1 [&_tr]:vds-border-subtle', className)}
      {...props}
    />
  ),
)
TableHeader.displayName = 'TableHeader'

export const TableBody = React.forwardRef<HTMLTableSectionElement, React.HTMLAttributes<HTMLTableSectionElement>>(
  ({ className, ...props }, ref) => (
    <tbody
      ref={ref}
      className={cn('[&_tr:last-child]:vds-border-b-0', className)}
      {...props}
    />
  ),
)
TableBody.displayName = 'TableBody'

export const TableFooter = React.forwardRef<HTMLTableSectionElement, React.HTMLAttributes<HTMLTableSectionElement>>(
  ({ className, ...props }, ref) => (
    <tfoot
      ref={ref}
      className={cn('vds-border-t-1 vds-border-subtle vds-bg-elevated vds-font-500', className)}
      {...props}
    />
  ),
)
TableFooter.displayName = 'TableFooter'

export const TableRow = React.forwardRef<HTMLTableRowElement, React.HTMLAttributes<HTMLTableRowElement>>(
  ({ className, ...props }, ref) => (
    <tr
      ref={ref}
      className={cn(
        'vds-border-b-1 vds-border-subtle vds-transition-colors',
        'vds-hover:bg-hover',
        'data-[state=selected]:vds-bg-elevated',
        className,
      )}
      {...props}
    />
  ),
)
TableRow.displayName = 'TableRow'

export const TableHead = React.forwardRef<HTMLTableCellElement, React.ThHTMLAttributes<HTMLTableCellElement>>(
  ({ className, scope = 'col', ...props }, ref) => (
    <th
      ref={ref}
      scope={scope}
      className={cn(
        'vds-h-11 vds-px-4 vds-text-left vds-align-middle',
        'vds-font-500 vds-text-dim vds-text-xs vds-uppercase vds-tracking-wide',
        'first:vds-pl-6 last:vds-pr-6',
        className,
      )}
      {...props}
    />
  ),
)
TableHead.displayName = 'TableHead'

export const TableCell = React.forwardRef<HTMLTableCellElement, React.TdHTMLAttributes<HTMLTableCellElement>>(
  ({ className, ...props }, ref) => (
    <td
      ref={ref}
      className={cn(
        'vds-px-4 vds-py-3 vds-align-middle',
        'first:vds-pl-6 last:vds-pr-6',
        className,
      )}
      {...props}
    />
  ),
)
TableCell.displayName = 'TableCell'

export const TableCaption = React.forwardRef<HTMLTableCaptionElement, React.HTMLAttributes<HTMLTableCaptionElement>>(
  ({ className, ...props }, ref) => (
    <caption
      ref={ref}
      className={cn('vds-mt-4 vds-text-sm vds-text-dim', className)}
      {...props}
    />
  ),
)
TableCaption.displayName = 'TableCaption'
