'use client'

import * as React from 'react'
import { cn } from '@/lib/utils'

type BadgeVariant = 'default' | 'secondary' | 'destructive' | 'outline'

export interface BadgeProps extends React.HTMLAttributes<HTMLDivElement> {
  variant?: BadgeVariant
}

function variantClass(v: BadgeVariant = 'default'): string {
  switch (v) {
    case 'default':
      return 'vds-bg-primary vds-text-primary-fg vds-border-1 vds-border-primary'
    case 'secondary':
      return 'vds-bg-elevated vds-text-primary vds-border-1 vds-border-subtle'
    case 'destructive':
      return 'vds-bg-destructive vds-text-destructive-fg vds-border-1 vds-border-destructive'
    case 'outline':
      return 'vds-bg-transparent vds-text-primary vds-border-1 vds-border-default'
  }
}

const BASE =
  'vds-inline-flex vds-items-center vds-rounded-md vds-px-2 vds-py-1 ' +
  'vds-text-xs vds-font-600 vds-tracking-tight'

export function Badge({ className, variant = 'default', ...props }: BadgeProps) {
  return <div className={cn(BASE, variantClass(variant), className)} {...props} />
}

export const badgeVariants = ({
  variant = 'default',
  className,
}: { variant?: BadgeVariant; className?: string } = {}) =>
  cn(BASE, variantClass(variant), className)
