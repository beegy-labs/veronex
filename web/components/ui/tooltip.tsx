'use client'

import * as React from 'react'
import {
  useFloating, autoUpdate, offset, flip, shift,
  useHover, useFocus, useDismiss, useRole, useInteractions,
  FloatingPortal, useId,
} from '@floating-ui/react'
import { cn } from '@/lib/utils'

/**
 * Compatible compound surface:
 *   <TooltipProvider>
 *     <Tooltip>
 *       <TooltipTrigger>...</TooltipTrigger>
 *       <TooltipContent>...</TooltipContent>
 *     </Tooltip>
 *   </TooltipProvider>
 *
 * WCAG 1.4.13 Content on Hover/Focus:
 *  - hoverable: pointer can move onto the tooltip (we set `move: true` in useHover)
 *  - dismissible: Esc closes (useDismiss)
 *  - persistent: stays until dismissed (until trigger leaves)
 */

interface TooltipContextValue {
  open: boolean
  setOpen: (b: boolean) => void
  refs: ReturnType<typeof useFloating>['refs']
  floatingStyles: React.CSSProperties
  getReferenceProps: (props?: Record<string, unknown>) => Record<string, unknown>
  getFloatingProps: (props?: Record<string, unknown>) => Record<string, unknown>
  id: string | undefined
}

const TooltipCtx = React.createContext<TooltipContextValue | null>(null)

export function TooltipProvider({
  delayDuration = 400,
  children,
}: { delayDuration?: number; children: React.ReactNode }) {
  void delayDuration
  return <>{children}</>
}

export interface TooltipProps {
  children?: React.ReactNode
  open?: boolean
  defaultOpen?: boolean
  onOpenChange?: (open: boolean) => void
  delayDuration?: number
}

export function Tooltip({ children, open: openProp, defaultOpen, onOpenChange, delayDuration = 400 }: TooltipProps) {
  const [internalOpen, setInternalOpen] = React.useState(!!defaultOpen)
  const isControlled = openProp !== undefined
  const open = isControlled ? !!openProp : internalOpen

  const setOpen = React.useCallback(
    (next: boolean) => {
      if (!isControlled) setInternalOpen(next)
      onOpenChange?.(next)
    },
    [isControlled, onOpenChange],
  )

  const id = useId()

  const { refs, floatingStyles, context } = useFloating({
    open,
    onOpenChange: setOpen,
    placement: 'top',
    whileElementsMounted: autoUpdate,
    middleware: [offset(6), flip(), shift({ padding: 8 })],
  })

  const hover = useHover(context, { delay: { open: delayDuration, close: 100 }, move: true })
  const focus = useFocus(context)
  const dismiss = useDismiss(context)
  const role = useRole(context, { role: 'tooltip' })
  const { getReferenceProps, getFloatingProps } = useInteractions([hover, focus, dismiss, role])

  return (
    <TooltipCtx.Provider value={{ open, setOpen, refs, floatingStyles, getReferenceProps, getFloatingProps, id }}>
      {children}
    </TooltipCtx.Provider>
  )
}

export interface TooltipTriggerProps extends React.HTMLAttributes<HTMLElement> {
  asChild?: boolean
  children: React.ReactNode
}

export function TooltipTrigger({ children, asChild, ...rest }: TooltipTriggerProps) {
  const ctx = React.useContext(TooltipCtx)
  if (!ctx) return <>{children}</>
  const refProps = ctx.getReferenceProps({ ref: ctx.refs.setReference, ...rest } as Record<string, unknown>)
  if (asChild && React.isValidElement(children)) {
    const child = children as React.ReactElement<Record<string, unknown>>
    return React.cloneElement(child, { ...refProps, ...child.props })
  }
  return <span {...(refProps as React.HTMLAttributes<HTMLSpanElement>)}>{children}</span>
}

export interface TooltipContentProps extends React.HTMLAttributes<HTMLDivElement> {
  sideOffset?: number
  /** Compatibility — Radix-style placement hint; floating-ui handles it via Tooltip placement. */
  side?: 'top' | 'right' | 'bottom' | 'left'
  align?: 'start' | 'center' | 'end'
}

export function TooltipContent({ className, sideOffset, side, align, children, ...rest }: TooltipContentProps) {
  const ctx = React.useContext(TooltipCtx)
  void sideOffset; void side; void align
  if (!ctx || !ctx.open) return null
  return (
    <FloatingPortal>
      <div
        ref={ctx.refs.setFloating}
        style={ctx.floatingStyles}
        {...ctx.getFloatingProps(rest as Record<string, unknown>)}
        className={cn(
          'vds-z-tooltip vds-rounded-md vds-border-1 vds-border-subtle',
          'vds-bg-card vds-text-primary vds-px-3 vds-py-1 vds-text-xs vds-shadow-2',
          className,
        )}
      >
        {children}
      </div>
    </FloatingPortal>
  )
}
