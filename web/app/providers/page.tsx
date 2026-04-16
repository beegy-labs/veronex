'use client'

import { useState, Suspense } from 'react'
import { useSearchParams } from 'next/navigation'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { useApiMutation } from '@/hooks/use-api-mutation'
import { providersQuery, serversQuery } from '@/lib/queries'
import { api } from '@/lib/api'
import type { Provider } from '@/lib/types'
import { useTranslation } from '@/i18n'
import { usePageGuard } from '@/hooks/use-page-guard'
import { useLabSettings } from '@/components/lab-settings-provider'
import { ConfirmDialog } from '@/components/confirm-dialog'
import { EditModal, RegisterModal } from './components/modals'
import { OllamaTab } from './components/ollama-tab'
import { GeminiTab } from './components/gemini-tab'

// ── Page ──────────────────────────────────────────────────────────────────────

function ProvidersContent({ section: sectionParam }: { section: string }) {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const { labSettings } = useLabSettings()
  const geminiEnabled = labSettings?.gemini_function_calling ?? false
  // Fall back to 'ollama' when Gemini is disabled and the URL says ?s=gemini
  const section = (sectionParam === 'gemini' && !geminiEnabled) ? 'ollama' : sectionParam


  const [registerProviderType, setRegisterProviderType] = useState<'ollama' | 'gemini' | null>(null)
  const [editingProvider, setEditingProvider] = useState<Provider | null>(null)
  const [deleteTarget, setDeleteTarget] = useState<{ id: string; name: string } | null>(null)

  // Servers needed for RegisterModal/EditModal dropdowns
  const { data: serversData } = useQuery(serversQuery())
  const servers = serversData?.servers

  const { data: providersData, isLoading: providersLoading, error: providersError } = useQuery(
    providersQuery({ provider_type: 'gemini' })
  )
  const providers = providersData?.providers

  const deleteMutation = useApiMutation(
    (id: string) => api.deleteProvider(id),
    { invalidateKey: ['providers'], onSuccess: () => setDeleteTarget(null) },
  )

  const syncProviderMutation = useMutation({
    mutationFn: (id: string) => api.syncProvider(id),
    onSettled: (_data, _error, id) => {
      queryClient.invalidateQueries({ queryKey: ['providers'] })
      queryClient.invalidateQueries({ queryKey: ['provider-models', id] })
      queryClient.invalidateQueries({ queryKey: ['selected-models', id] })
      queryClient.invalidateQueries({ queryKey: ['ollama-sync-status'] })
      queryClient.invalidateQueries({ queryKey: ['ollama-models'] })
      queryClient.invalidateQueries({ queryKey: ['capacity'] })
    },
  })

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-bold tracking-tight">
          {section === 'gemini' ? t('providers.gemini.title') : t('providers.ollama.title')}
        </h1>
        <p className="text-muted-foreground mt-1 text-sm">
          {section === 'gemini' ? t('providers.gemini.description') : t('providers.ollama.description')}
        </p>
      </div>

      {section === 'ollama' && (
        <OllamaTab
          servers={servers ?? []}
          onRegister={() => setRegisterProviderType('ollama')}
          onEdit={(b) => setEditingProvider(b)}
          onSync={(id) => syncProviderMutation.mutate(id)}
          syncPending={syncProviderMutation.isPending}
          syncVars={syncProviderMutation.variables}
          onDelete={(id, name) => setDeleteTarget({ id, name })}
          deleteIsPending={deleteMutation.isPending}
        />
      )}

      {section === 'gemini' && (
        <GeminiTab
          providers={providers}
          isLoading={providersLoading}
          error={providersError as Error | null}
          onRegister={() => setRegisterProviderType('gemini')}
          onEdit={(b) => setEditingProvider(b)}
          onSync={(id) => syncProviderMutation.mutate(id)}
          syncPending={syncProviderMutation.isPending}
          onDelete={(id, name) => setDeleteTarget({ id, name })}
          deleteIsPending={deleteMutation.isPending}
        />
      )}

      {registerProviderType && (
        <RegisterModal
          servers={servers ?? []}
          initialType={registerProviderType}
          onClose={() => setRegisterProviderType(null)}
        />
      )}
      {editingProvider && (
        <EditModal
          provider={editingProvider}
          servers={servers ?? []}
          onClose={() => setEditingProvider(null)}
        />
      )}
      {deleteTarget && (
        <ConfirmDialog
          open
          title={t('providers.deleteProvider')}
          description={t('providers.deleteConfirm', { name: deleteTarget.name })}
          confirmLabel={deleteMutation.isPending ? t('common.deleting') : t('common.delete')}
          onConfirm={() => deleteMutation.mutate(deleteTarget.id)}
          onClose={() => setDeleteTarget(null)}
          isLoading={deleteMutation.isPending}
        />
      )}
    </div>
  )
}

function ProvidersSectionReader() {
  const searchParams = useSearchParams()
  const section = searchParams.get('s') ?? 'ollama'
  return <ProvidersContent section={section} />
}

export default function ProvidersPage() {
  usePageGuard('providers')
  const { t } = useTranslation()
  return (
    <Suspense fallback={<div className="p-2 text-sm text-muted-foreground">{t('common.loading')}</div>}>
      <ProvidersSectionReader />
    </Suspense>
  )
}
