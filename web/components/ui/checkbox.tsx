'use client'

import * as React from 'react'
import { Check } from 'lucide-react'
import { cn } from '@/lib/utils'

/**
 * Compatible with the prior Radix checkbox API:
 *   <Checkbox checked={...} onCheckedChange={...} />
 *   - boolean | "indeterminate"
 */
export interface CheckboxProps
  extends Omit<React.InputHTMLAttributes<HTMLInputElement>, 'type' | 'checked' | 'onChange'> {
  checked?: boolean | 'indeterminate'
  defaultChecked?: boolean
  onCheckedChange?: (checked: boolean) => void
}

export const Checkbox = React.forwardRef<HTMLInputElement, CheckboxProps>(
  ({ className, checked, defaultChecked, onCheckedChange, disabled, ...rest }, ref) => {
    const inputRef = React.useRef<HTMLInputElement | null>(null)
    React.useImperativeHandle(ref, () => inputRef.current as HTMLInputElement)
    const isChecked = checked === true || (checked === undefined && !!defaultChecked)
    const isIndeterminate = checked === 'indeterminate'

    React.useEffect(() => {
      if (inputRef.current) inputRef.current.indeterminate = isIndeterminate
    }, [isIndeterminate])

    return (
      <span className={cn('vds-relative vds-inline-flex vds-items-center vds-justify-center', className)}>
        <input
          ref={inputRef}
          type="checkbox"
          disabled={disabled}
          checked={checked === undefined ? undefined : isChecked}
          defaultChecked={checked === undefined ? defaultChecked : undefined}
          onChange={(e) => onCheckedChange?.(e.target.checked)}
          className={cn(
            'vds-h-4 vds-w-4 vds-rounded vds-border-1 vds-border-default vds-bg-card',
            'vds-cursor-pointer vds-appearance-none',
            'checked:vds-bg-primary checked:vds-border-primary',
            'vds-disabled:cursor-not-allowed vds-disabled:opacity-50',
          )}
          {...rest}
        />
        {(isChecked || isIndeterminate) && (
          <Check
            aria-hidden
            className="vds-absolute vds-pointer-events-none vds-h-3 vds-w-3 vds-text-primary-fg"
          />
        )}
      </span>
    )
  },
)
Checkbox.displayName = 'Checkbox'
