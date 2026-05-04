'use client'

import * as React from 'react'
import {
  useFloating, autoUpdate, offset, flip, size,
  useDismiss, useRole, useListNavigation, useTypeahead, useClick, useInteractions,
  FloatingPortal, FloatingFocusManager,
} from '@floating-ui/react'
import { Check, ChevronDown } from 'lucide-react'
import { cn } from '@/lib/utils'

/**
 * WAI-ARIA Listbox-as-Combobox (select-only, AP 1.2). Compatible API:
 *
 *   <Select value={v} onValueChange={setV} disabled>
 *     <SelectTrigger><SelectValue placeholder="…" /></SelectTrigger>
 *     <SelectContent>
 *       <SelectGroup>
 *         <SelectLabel>Group</SelectLabel>
 *         <SelectItem value="a">Apple</SelectItem>
 *         <SelectSeparator />
 *       </SelectGroup>
 *     </SelectContent>
 *   </Select>
 */

interface SelectContextValue {
  value: string
  setValue: (v: string) => void
  open: boolean
  setOpen: (b: boolean) => void
  disabled?: boolean
  refs: ReturnType<typeof useFloating>['refs']
  floatingStyles: React.CSSProperties
  context: ReturnType<typeof useFloating>['context']
  getReferenceProps: (props?: Record<string, unknown>) => Record<string, unknown>
  getFloatingProps: (props?: Record<string, unknown>) => Record<string, unknown>
  getItemProps: (extra?: Record<string, unknown>) => Record<string, unknown>
  activeIndex: number | null
  setActiveIndex: (i: number | null) => void
  registerItem: (label: string) => number
  resetItems: () => void
  itemRefs: React.MutableRefObject<Array<HTMLElement | null>>
  triggerLabel: string
  setTriggerLabel: (l: string) => void
  placeholder?: string
}

const SelectCtx = React.createContext<SelectContextValue | null>(null)

export interface SelectProps {
  value?: string
  defaultValue?: string
  onValueChange?: (v: string) => void
  disabled?: boolean
  children?: React.ReactNode
}

export function Select({ value: vProp, defaultValue, onValueChange, disabled, children }: SelectProps) {
  const [internal, setInternal] = React.useState(defaultValue ?? '')
  const isControlled = vProp !== undefined
  const value = isControlled ? (vProp as string) : internal
  const setValue = React.useCallback((next: string) => {
    if (!isControlled) setInternal(next)
    onValueChange?.(next)
  }, [isControlled, onValueChange])

  const [open, setOpen] = React.useState(false)
  const [activeIndex, setActiveIndex] = React.useState<number | null>(null)
  const [triggerLabel, setTriggerLabel] = React.useState('')

  const labelsRef = React.useRef<string[]>([])
  const itemRefs = React.useRef<Array<HTMLElement | null>>([])

  const { refs, floatingStyles, context } = useFloating({
    open,
    onOpenChange: setOpen,
    placement: 'bottom-start',
    whileElementsMounted: autoUpdate,
    middleware: [
      offset(4),
      flip(),
      size({
        apply({ rects, availableHeight, elements }) {
          Object.assign(elements.floating.style, {
            maxHeight: `${Math.min(availableHeight, 320)}px`,
            minWidth: `${rects.reference.width}px`,
          })
        },
        padding: 8,
      }),
    ],
  })

  const click = useClick(context, { event: 'click' })
  const dismiss = useDismiss(context)
  const role = useRole(context, { role: 'listbox' })
  const list = useListNavigation(context, {
    listRef: itemRefs,
    activeIndex,
    onNavigate: setActiveIndex,
    loop: true,
  })
  const type = useTypeahead(context, {
    listRef: labelsRef,
    activeIndex,
    onMatch: open ? setActiveIndex : (i) => i !== null && commitIndex(i),
  })

  function commitIndex(i: number) {
    const node = itemRefs.current[i]
    const v = node?.dataset.value
    if (v != null) setValue(v)
  }

  const { getReferenceProps, getFloatingProps, getItemProps } = useInteractions([
    click, dismiss, role, list, type,
  ])

  const registerItem = React.useCallback((label: string) => {
    labelsRef.current.push(label)
    return labelsRef.current.length - 1
  }, [])
  const resetItems = React.useCallback(() => {
    labelsRef.current = []
    itemRefs.current = []
  }, [])

  // Reset registry on each render pass (children may be conditional)
  resetItems()

  return (
    <SelectCtx.Provider
      value={{
        value, setValue, open, setOpen, disabled,
        refs, floatingStyles, context,
        getReferenceProps, getFloatingProps, getItemProps,
        activeIndex, setActiveIndex,
        registerItem, resetItems, itemRefs,
        triggerLabel, setTriggerLabel,
      }}
    >
      {children}
    </SelectCtx.Provider>
  )
}

export interface SelectTriggerProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  children?: React.ReactNode
}

export const SelectTrigger = React.forwardRef<HTMLButtonElement, SelectTriggerProps>(
  ({ className, children, disabled, ...rest }, ref) => {
    const ctx = React.useContext(SelectCtx)
    if (!ctx) return null
    const isDisabled = disabled || ctx.disabled
    return (
      <button
        ref={(node) => {
          ctx.refs.setReference(node)
          if (typeof ref === 'function') ref(node)
          else if (ref) (ref as React.MutableRefObject<HTMLButtonElement | null>).current = node
        }}
        type="button"
        disabled={isDisabled}
        aria-haspopup="listbox"
        aria-expanded={ctx.open}
        data-placeholder={!ctx.value || undefined}
        className={cn(
          'vds-flex vds-h-9 vds-w-full vds-items-center vds-justify-between',
          'vds-rounded-md vds-border-1 vds-border-default vds-bg-card vds-px-3 vds-py-2',
          'vds-text-sm vds-text-primary vds-cursor-pointer vds-shadow-1',
          'data-[placeholder]:vds-text-faint',
          'vds-disabled:cursor-not-allowed vds-disabled:opacity-50',
          className,
        )}
        {...ctx.getReferenceProps(rest as Record<string, unknown>)}
      >
        {children}
        <ChevronDown aria-hidden className="vds-h-4 vds-w-4 vds-opacity-50 vds-flex-shrink-0" />
      </button>
    )
  },
)
SelectTrigger.displayName = 'SelectTrigger'

export interface SelectValueProps extends React.HTMLAttributes<HTMLSpanElement> {
  placeholder?: string
}

export function SelectValue({ placeholder, className, ...rest }: SelectValueProps) {
  const ctx = React.useContext(SelectCtx)
  if (!ctx) return null
  const label = ctx.triggerLabel || ctx.value || placeholder || ''
  return (
    <span className={cn('vds-truncate', className)} {...rest}>
      {label}
    </span>
  )
}

export interface SelectContentProps extends React.HTMLAttributes<HTMLDivElement> {
  position?: 'popper' | 'item-aligned'
}

export function SelectContent({ className, children, position, ...rest }: SelectContentProps) {
  const ctx = React.useContext(SelectCtx)
  void position
  if (!ctx || !ctx.open) return null
  return (
    <FloatingPortal>
      <FloatingFocusManager context={ctx.context} modal={false}>
        <div
          ref={ctx.refs.setFloating}
          style={ctx.floatingStyles}
          className={cn(
            'vds-z-popover vds-rounded-md vds-border-1 vds-border-default',
            'vds-bg-card vds-text-primary vds-shadow-2 vds-overflow-y-auto vds-overflow-x-hidden',
            'vds-p-1',
            className,
          )}
          role="listbox"
          {...ctx.getFloatingProps(rest as Record<string, unknown>)}
        >
          {children}
        </div>
      </FloatingFocusManager>
    </FloatingPortal>
  )
}

export const SelectGroup: React.FC<React.HTMLAttributes<HTMLDivElement>> = ({ className, ...props }) => (
  <div role="group" className={cn(className)} {...props} />
)

export const SelectLabel: React.FC<React.HTMLAttributes<HTMLDivElement>> = ({ className, ...props }) => (
  <div className={cn('vds-px-2 vds-py-1 vds-text-xs vds-font-600 vds-text-dim', className)} {...props} />
)

export interface SelectItemProps extends React.HTMLAttributes<HTMLDivElement> {
  value: string
  disabled?: boolean
  children?: React.ReactNode
}

export function SelectItem({ value, disabled, className, children, ...rest }: SelectItemProps) {
  const ctx = React.useContext(SelectCtx)
  if (!ctx) return null
  const label = typeof children === 'string' ? children : value
  const index = ctx.registerItem(label)
  const active = ctx.activeIndex === index
  const selected = ctx.value === value

  // When this item is currently selected, expose its label up to the trigger.
  React.useEffect(() => {
    if (selected) ctx.setTriggerLabel(label)
  }, [selected, label, ctx])

  return (
    <div
      ref={(n) => {
        ctx.itemRefs.current[index] = n
      }}
      role="option"
      aria-selected={selected}
      aria-disabled={disabled || undefined}
      data-value={value}
      tabIndex={active ? 0 : -1}
      className={cn(
        'vds-relative vds-flex vds-w-full vds-cursor-pointer vds-items-center',
        'vds-rounded-sm vds-py-2 vds-pl-2 vds-pr-8 vds-text-sm vds-text-primary',
        active && 'vds-bg-hover',
        disabled && 'vds-opacity-50 vds-cursor-not-allowed',
        className,
      )}
      {...ctx.getItemProps({
        onClick: () => {
          if (disabled) return
          ctx.setValue(value)
          ctx.setOpen(false)
        },
        ...rest,
      })}
    >
      <span className="vds-truncate">{children}</span>
      {selected && (
        <Check aria-hidden className="vds-absolute vds-right-2 vds-h-4 vds-w-4" />
      )}
    </div>
  )
}

export const SelectSeparator: React.FC<React.HTMLAttributes<HTMLDivElement>> = ({ className, ...props }) => (
  <div role="none" className={cn('vds-h-px vds-bg-border-subtle vds-my-1', className)} {...props} />
)

// Legacy aliases for any callers still using them.
export const SelectScrollUpButton: React.FC<React.HTMLAttributes<HTMLDivElement>> = () => null
export const SelectScrollDownButton: React.FC<React.HTMLAttributes<HTMLDivElement>> = () => null
