'use client'

import * as React from 'react'
import { X } from 'lucide-react'
import { useTranslation } from '@/i18n'
import { cn } from '@/lib/utils'
import { tokens } from '@/lib/design-tokens'

/**
 * Compatible Radix-style compound API. Built on the native HTMLDialogElement,
 * which provides focus trap, ESC dismissal, and inert background — no Radix.
 *
 * Usage:
 *   <Dialog open={x} onOpenChange={setX}>
 *     <DialogTrigger>...</DialogTrigger>     // optional; for uncontrolled use
 *     <DialogContent>
 *       <DialogHeader><DialogTitle>...</DialogTitle></DialogHeader>
 *       …
 *       <DialogFooter>…</DialogFooter>
 *     </DialogContent>
 *   </Dialog>
 */

interface DialogContextValue {
  open: boolean
  setOpen: (b: boolean) => void
  dialogRef: React.RefObject<HTMLDialogElement | null>
}
const DialogCtx = React.createContext<DialogContextValue | null>(null)

export interface DialogProps {
  open?: boolean
  defaultOpen?: boolean
  onOpenChange?: (open: boolean) => void
  children?: React.ReactNode
}

export function Dialog({ open: openProp, defaultOpen, onOpenChange, children }: DialogProps) {
  const [internal, setInternal] = React.useState(!!defaultOpen)
  const isControlled = openProp !== undefined
  const open = isControlled ? !!openProp : internal
  const dialogRef = React.useRef<HTMLDialogElement | null>(null)

  const setOpen = React.useCallback((next: boolean) => {
    if (!isControlled) setInternal(next)
    onOpenChange?.(next)
  }, [isControlled, onOpenChange])

  React.useEffect(() => {
    const el = dialogRef.current
    if (!el) return
    if (open && !el.open) {
      try { el.showModal() } catch { /* already open in some race */ }
    } else if (!open && el.open) {
      el.close()
    }
  }, [open])

  return (
    <DialogCtx.Provider value={{ open, setOpen, dialogRef }}>
      {children}
    </DialogCtx.Provider>
  )
}

export const DialogPortal: React.FC<{ children?: React.ReactNode }> = ({ children }) => <>{children}</>
export const DialogOverlay: React.FC<React.HTMLAttributes<HTMLDivElement>> = () => null

export interface DialogTriggerProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  asChild?: boolean
  children?: React.ReactNode
}

export function DialogTrigger({ asChild, children, onClick, ...rest }: DialogTriggerProps) {
  const ctx = React.useContext(DialogCtx)
  function open(e: React.MouseEvent<HTMLButtonElement>) {
    onClick?.(e as React.MouseEvent<HTMLButtonElement>)
    ctx?.setOpen(true)
  }
  if (asChild && React.isValidElement(children)) {
    const child = children as React.ReactElement<{ onClick?: React.MouseEventHandler }>
    return React.cloneElement(child, {
      onClick: (e: React.MouseEvent) => {
        child.props.onClick?.(e)
        ctx?.setOpen(true)
      },
    } as Record<string, unknown>)
  }
  return <button type="button" onClick={open} {...rest}>{children}</button>
}

export function DialogClose({
  asChild,
  children,
  onClick,
  ...rest
}: React.ButtonHTMLAttributes<HTMLButtonElement> & { asChild?: boolean }) {
  const ctx = React.useContext(DialogCtx)
  function close(e: React.MouseEvent<HTMLButtonElement>) {
    onClick?.(e as React.MouseEvent<HTMLButtonElement>)
    ctx?.setOpen(false)
  }
  if (asChild && React.isValidElement(children)) {
    const child = children as React.ReactElement<{ onClick?: React.MouseEventHandler }>
    return React.cloneElement(child, {
      onClick: (e: React.MouseEvent) => {
        child.props.onClick?.(e)
        ctx?.setOpen(false)
      },
    } as Record<string, unknown>)
  }
  return <button type="button" onClick={close} {...rest}>{children}</button>
}

export interface DialogContentProps extends React.HTMLAttributes<HTMLDivElement> {
  showClose?: boolean
  /** Compatibility no-op: Radix-only callbacks ignored on native <dialog>. */
  onPointerDownOutside?: (e: unknown) => void
  onInteractOutside?: (e: unknown) => void
  onEscapeKeyDown?: (e: unknown) => void
}

export const DialogContent = React.forwardRef<HTMLDivElement, DialogContentProps>(
  ({ className, children, showClose = true, onPointerDownOutside, onInteractOutside, onEscapeKeyDown, ...rest }, ref) => {
    void onPointerDownOutside; void onInteractOutside; void onEscapeKeyDown
    const ctx = React.useContext(DialogCtx)
    const { t } = useTranslation()
    if (!ctx) return null
    const dialogCtx = ctx

    function onCancel(e: React.SyntheticEvent<HTMLDialogElement>) {
      e.preventDefault()
      dialogCtx.setOpen(false)
    }
    function onClick(e: React.MouseEvent<HTMLDialogElement>) {
      // Backdrop click — native <dialog> reports e.target === dialog
      if (e.target === dialogCtx.dialogRef.current) dialogCtx.setOpen(false)
    }

    return (
      <dialog
        ref={ctx.dialogRef}
        onCancel={onCancel}
        onClick={onClick}
        aria-modal="true"
        className={cn(
          'vds-rounded-lg vds-border-1 vds-border-default',
          'vds-bg-card vds-text-primary vds-shadow-3 vds-p-0',
          'vds-w-full vds-max-w-lg',
        )}
        style={{
          margin: 'auto',
          padding: 0,
          background: tokens.bg.card,
          color: tokens.text.primary,
          border: `1px solid ${tokens.border.default}`,
        }}
      >
        <div ref={ref} className={cn('vds-relative vds-grid vds-gap-4 vds-p-6', className)} {...rest}>
          {children}
          {showClose && (
            <button
              type="button"
              onClick={() => ctx.setOpen(false)}
              aria-label={t('common.close')}
              className={cn(
                'vds-absolute vds-top-4 vds-right-4 vds-rounded-md vds-p-1',
                'vds-text-dim vds-hover:bg-hover vds-hover:text-primary',
                'vds-transition-colors',
              )}
            >
              <X className="vds-h-4 vds-w-4" aria-hidden />
            </button>
          )}
        </div>
      </dialog>
    )
  },
)
DialogContent.displayName = 'DialogContent'

export const DialogHeader: React.FC<React.HTMLAttributes<HTMLDivElement>> = ({ className, ...props }) => (
  <div className={cn('vds-flex vds-flex-col vds-gap-2', className)} {...props} />
)
DialogHeader.displayName = 'DialogHeader'

export const DialogFooter: React.FC<React.HTMLAttributes<HTMLDivElement>> = ({ className, ...props }) => (
  <div
    className={cn('vds-flex vds-flex-col-reverse vds-md:flex-row vds-md:justify-end vds-gap-2', className)}
    {...props}
  />
)
DialogFooter.displayName = 'DialogFooter'

export const DialogTitle = React.forwardRef<HTMLHeadingElement, React.HTMLAttributes<HTMLHeadingElement>>(
  ({ className, ...props }, ref) => (
    <h2
      ref={ref}
      className={cn('vds-text-lg vds-font-600 vds-leading-tight vds-tracking-tight', className)}
      {...props}
    />
  ),
)
DialogTitle.displayName = 'DialogTitle'

export const DialogDescription = React.forwardRef<HTMLParagraphElement, React.HTMLAttributes<HTMLParagraphElement>>(
  ({ className, ...props }, ref) => (
    <p ref={ref} className={cn('vds-text-sm vds-text-dim', className)} {...props} />
  ),
)
DialogDescription.displayName = 'DialogDescription'
