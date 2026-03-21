'use client'

import { useState, Suspense } from 'react'
import { useSearchParams } from 'next/navigation'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { providersQuery, serversQuery } from '@/lib/queries'
import { api } from '@/lib/api'
import type { Provider } from '@/lib/types'
import { useTranslation } from '@/i18n'
import { useLabSettings } from '@/components/lab-settings-provider'
import { PROVIDER_OLLAMA } from '@/lib/constants'
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

  // Servers needed for RegisterModal/EditModal dropdowns
  const { data: serversData } = useQuery(serversQuery())
  const servers = serversData?.servers

  const { data: providersData, isLoading: providersLoading, error: providersError } = useQuery(providersQuery())
  const providers = providersData?.providers

  const deleteMutation = useMutation({
    mutationFn: (id: string) => api.deleteProvider(id),
    onSettled: () => queryClient.invalidateQueries({ queryKey: ['providers'] }),
  })

  const toggleActiveMutation = useMutation({
    mutationFn: (b: Provider) =>
      api.updateProvider(b.id, {
        name: b.name,
        is_active: !b.is_active,
        ...(b.provider_type === PROVIDER_OLLAMA && { url: b.url, total_vram_mb: b.total_vram_mb, gpu_index: b.gpu_index, server_id: b.server_id }),
      }),
    onSettled: () => queryClient.invalidateQueries({ queryKey: ['providers'] }),
  })

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
          providers={providers}
          servers={servers ?? []}
          isLoading={providersLoading}
          error={providersError as Error | null}
          onRegister={() => setRegisterProviderType('ollama')}
          onEdit={(b) => setEditingProvider(b)}
          onSync={(id) => syncProviderMutation.mutate(id)}
          syncPending={syncProviderMutation.isPending}
          syncVars={syncProviderMutation.variables}
          onDelete={(id, name) => { if (confirm(t('providers.deleteConfirm', { name }))) deleteMutation.mutate(id) }}
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
          onToggleActive={(b) => toggleActiveMutation.mutate(b)}
          toggleActivePending={toggleActiveMutation.isPending}
          onDelete={(id, name) => { if (confirm(t('providers.deleteConfirm', { name }))) deleteMutation.mutate(id) }}
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
    </div>
  )
}

function ProvidersSectionReader() {
  const searchParams = useSearchParams()
  const section = searchParams.get('s') ?? 'ollama'
  return <ProvidersContent section={section} />
}

export default function ProvidersPage() {
  const { t } = useTranslation()
  return (
    <Suspense fallback={<div className="p-2 text-sm text-muted-foreground">{t('common.loading')}</div>}>
      <ProvidersSectionReader />
    </Suspense>
  )
}
