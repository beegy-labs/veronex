'use client'

import * as React from 'react'
import { cn } from '@/lib/utils'

export interface SwitchProps
  extends Omit<React.ButtonHTMLAttributes<HTMLButtonElement>, 'onChange'> {
  checked?: boolean
  defaultChecked?: boolean
  onCheckedChange?: (checked: boolean) => void
  size?: 'sm' | 'default'
}

export const Switch = React.forwardRef<HTMLButtonElement, SwitchProps>(
  ({ className, checked, defaultChecked, onCheckedChange, disabled, size = 'default', id, ...rest }, ref) => {
    const [internal, setInternal] = React.useState(!!defaultChecked)
    const isControlled = checked !== undefined
    const value = isControlled ? !!checked : internal

    function toggle() {
      if (disabled) return
      const next = !value
      if (!isControlled) setInternal(next)
      onCheckedChange?.(next)
    }

    return (
      <button
        ref={ref}
        type="button"
        role="switch"
        aria-checked={value}
        data-state={value ? 'checked' : 'unchecked'}
        data-size={size}
        disabled={disabled}
        onClick={toggle}
        onKeyDown={(e) => {
          if (e.key === ' ' || e.key === 'Enter') {
            e.preventDefault()
            toggle()
          }
        }}
        id={id}
        className={cn(
          'vds-relative vds-inline-flex vds-items-center vds-rounded-full vds-cursor-pointer',
          'vds-transition-colors vds-border-1 vds-border-transparent',
          size === 'sm' ? 'vds-h-4 vds-w-7' : 'vds-h-5 vds-w-9',
          value ? 'vds-bg-primary' : 'vds-bg-elevated',
          'vds-disabled:cursor-not-allowed vds-disabled:opacity-50',
          className,
        )}
        {...rest}
      >
        <span
          aria-hidden
          className={cn(
            'vds-inline-block vds-rounded-full vds-bg-card vds-transition-transform',
            size === 'sm' ? 'vds-h-3 vds-w-3' : 'vds-h-4 vds-w-4',
          )}
          style={{
            transform: value
              ? `translateX(${size === 'sm' ? '14px' : '18px'})`
              : 'translateX(2px)',
          }}
        />
      </button>
    )
  },
)
Switch.displayName = 'Switch'
