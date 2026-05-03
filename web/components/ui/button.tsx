'use client'

import * as React from 'react'
import { cn } from '@/lib/utils'

/**
 * Button — verodesign tokens, Tailwind-free.
 * Backward-compat surface with Radix-era veronex (variant + size + asChild).
 */
type Variant = 'default' | 'destructive' | 'outline' | 'secondary' | 'ghost' | 'link'
type Size = 'default' | 'sm' | 'lg' | 'icon'

export interface ButtonProps
  extends Omit<React.ButtonHTMLAttributes<HTMLButtonElement>, 'children'> {
  variant?: Variant
  size?: Size
  asChild?: boolean
  children?: React.ReactNode
}

function variantClass(v: Variant = 'default'): string {
  switch (v) {
    case 'default':
      return 'vds-bg-primary vds-text-primary-fg vds-border-1 vds-border-primary vds-hover:bg-primary-ring'
    case 'destructive':
      return 'vds-bg-destructive vds-text-destructive-fg vds-border-1 vds-border-destructive vds-hover:opacity-90'
    case 'outline':
      return 'vds-bg-card vds-text-primary vds-border-1 vds-border-default vds-hover:bg-elevated'
    case 'secondary':
      return 'vds-bg-elevated vds-text-primary vds-border-1 vds-border-subtle vds-hover:bg-hover'
    case 'ghost':
      return 'vds-bg-transparent vds-text-primary vds-hover:bg-hover vds-border-0'
    case 'link':
      return 'vds-bg-transparent vds-text-primary vds-underline vds-border-0'
  }
}

function sizeClass(s: Size = 'default'): string {
  switch (s) {
    case 'default': return 'vds-h-9 vds-px-4 vds-py-2 vds-text-sm'
    case 'sm':      return 'vds-h-8 vds-px-3 vds-text-xs'
    case 'lg':      return 'vds-h-10 vds-px-8 vds-text-sm'
    case 'icon':    return 'vds-h-9 vds-w-9 vds-p-0'
  }
}

const BASE =
  'vds-inline-flex vds-items-center vds-justify-center vds-gap-2 ' +
  'vds-rounded-md vds-font-500 vds-transition-colors vds-cursor-pointer ' +
  'vds-disabled:cursor-not-allowed vds-disabled:opacity-50'

export const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant = 'default', size = 'default', asChild, type = 'button', ...props }, ref) => {
    // asChild: forward className/ref to the single child element (replaces Radix Slot)
    if (asChild && React.isValidElement(props.children)) {
      const child = props.children as React.ReactElement<{ className?: string }>
      const merged = cn(BASE, variantClass(variant), sizeClass(size), child.props.className, className)
      return React.cloneElement(child, { className: merged, ref } as any)
    }
    return (
      <button
        ref={ref}
        type={type}
        className={cn(BASE, variantClass(variant), sizeClass(size), className)}
        {...props}
      />
    )
  },
)
Button.displayName = 'Button'

export const buttonVariants = ({
  variant = 'default',
  size = 'default',
  className,
}: { variant?: Variant; size?: Size; className?: string } = {}) =>
  cn(BASE, variantClass(variant), sizeClass(size), className)
