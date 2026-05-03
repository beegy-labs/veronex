'use client'

import * as React from 'react'
import { cn } from '@/lib/utils'

/**
 * WAI-ARIA Tabs pattern (AP 1.2). Compatible API with prior Radix usage:
 *   <Tabs value onValueChange defaultValue>
 *     <TabsList>
 *       <TabsTrigger value="a">A</TabsTrigger>
 *     </TabsList>
 *     <TabsContent value="a">…</TabsContent>
 *   </Tabs>
 */

interface TabsContextValue {
  value: string
  setValue: (v: string) => void
  baseId: string
  orientation: 'horizontal' | 'vertical'
}

const TabsCtx = React.createContext<TabsContextValue | null>(null)

export interface TabsProps extends React.HTMLAttributes<HTMLDivElement> {
  value?: string
  defaultValue?: string
  onValueChange?: (value: string) => void
  orientation?: 'horizontal' | 'vertical'
}

let tabIdSeq = 0

export const Tabs = React.forwardRef<HTMLDivElement, TabsProps>(
  ({ className, value: vProp, defaultValue, onValueChange, orientation = 'horizontal', children, ...rest }, ref) => {
    const [internal, setInternal] = React.useState(defaultValue ?? '')
    const isControlled = vProp !== undefined
    const value = isControlled ? (vProp as string) : internal
    const baseId = React.useMemo(() => `vds-tabs-${++tabIdSeq}`, [])

    const setValue = React.useCallback((next: string) => {
      if (!isControlled) setInternal(next)
      onValueChange?.(next)
    }, [isControlled, onValueChange])

    return (
      <TabsCtx.Provider value={{ value, setValue, baseId, orientation }}>
        <div ref={ref} className={cn(className)} data-orientation={orientation} {...rest}>
          {children}
        </div>
      </TabsCtx.Provider>
    )
  },
)
Tabs.displayName = 'Tabs'

export const TabsList = React.forwardRef<HTMLDivElement, React.HTMLAttributes<HTMLDivElement>>(
  ({ className, ...props }, ref) => {
    const ctx = React.useContext(TabsCtx)
    return (
      <div
        ref={ref}
        role="tablist"
        aria-orientation={ctx?.orientation}
        className={cn(
          'vds-inline-flex vds-h-9 vds-items-center vds-justify-center',
          'vds-rounded-lg vds-bg-elevated vds-p-1 vds-text-dim',
          className,
        )}
        {...props}
      />
    )
  },
)
TabsList.displayName = 'TabsList'

export interface TabsTriggerProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  value: string
}

export const TabsTrigger = React.forwardRef<HTMLButtonElement, TabsTriggerProps>(
  ({ className, value, onClick, onKeyDown, ...rest }, ref) => {
    const ctx = React.useContext(TabsCtx)
    if (!ctx) return null
    const active = ctx.value === value
    const id = `${ctx.baseId}-tab-${value}`
    const panelId = `${ctx.baseId}-panel-${value}`
    return (
      <button
        ref={ref}
        type="button"
        role="tab"
        id={id}
        aria-selected={active}
        aria-controls={panelId}
        tabIndex={active ? 0 : -1}
        data-state={active ? 'active' : 'inactive'}
        onClick={(e) => {
          ctx.setValue(value)
          onClick?.(e)
        }}
        onKeyDown={(e) => {
          if (e.key === 'Enter' || e.key === ' ') {
            e.preventDefault()
            ctx.setValue(value)
          }
          onKeyDown?.(e)
        }}
        className={cn(
          'vds-inline-flex vds-items-center vds-justify-center vds-rounded-md',
          'vds-px-3 vds-py-1 vds-text-sm vds-font-500 vds-transition-colors vds-cursor-pointer',
          active
            ? 'vds-bg-card vds-text-primary vds-shadow-1'
            : 'vds-text-dim vds-hover:text-primary',
          'vds-disabled:cursor-not-allowed vds-disabled:opacity-50',
          className,
        )}
        {...rest}
      />
    )
  },
)
TabsTrigger.displayName = 'TabsTrigger'

export interface TabsContentProps extends React.HTMLAttributes<HTMLDivElement> {
  value: string
}

export const TabsContent = React.forwardRef<HTMLDivElement, TabsContentProps>(
  ({ className, value, ...rest }, ref) => {
    const ctx = React.useContext(TabsCtx)
    if (!ctx) return null
    const active = ctx.value === value
    const id = `${ctx.baseId}-panel-${value}`
    const tabId = `${ctx.baseId}-tab-${value}`
    if (!active) return null
    return (
      <div
        ref={ref}
        role="tabpanel"
        id={id}
        aria-labelledby={tabId}
        tabIndex={0}
        className={cn('vds-mt-2', className)}
        {...rest}
      />
    )
  },
)
TabsContent.displayName = 'TabsContent'
