import { useEffect, useRef, useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { useParams } from 'react-router'
import { Eye, EyeOff, KeyRound, Plus, RotateCw, Search, ShieldCheck, Trash2 } from 'lucide-react'
import { toast } from 'sonner'
import { useDeleteSecret, useSecrets, useUpsertSecret } from '@/api/hooks'
import type { SecretMetadata } from '@/api/types'
import { Button, Input, Label } from '@sdlc/ui/ui'
import { ConfirmDialog } from '@/shared/ui/confirm-dialog'
import { QueryState } from '@/shared/ui/query-state'

const pageSize = 20
const emptyForm = { key: '', value: '' }

export function SecretsPage() {
  const { t } = useTranslation()
  const { projectId } = useParams()
  const { data: secrets = [], isLoading, error: listError, refetch } = useSecrets(projectId)
  const upsert = useUpsertSecret(projectId)
  const remove = useDeleteSecret()
  const [showForm, setShowForm] = useState(false)
  const [editingKey, setEditingKey] = useState<string | null>(null)
  const [form, setForm] = useState(emptyForm)
  const [showValue, setShowValue] = useState(false)
  const [saveError, setSaveError] = useState('')
  const [pendingDelete, setPendingDelete] = useState<SecretMetadata | null>(null)
  const [deleteError, setDeleteError] = useState('')
  const [search, setSearch] = useState('')
  const [page, setPage] = useState(1)
  const keyRef = useRef<HTMLInputElement>(null)
  const valueRef = useRef<HTMLInputElement>(null)

  const normalizedSearch = search.trim().toLocaleLowerCase()
  const filteredSecrets = secrets.filter((secret) =>
    secret.key.toLocaleLowerCase().includes(normalizedSearch),
  )
  const totalPages = Math.max(1, Math.ceil(filteredSecrets.length / pageSize))
  const currentPage = Math.min(page, totalPages)
  const visibleSecrets = filteredSecrets.slice((currentPage - 1) * pageSize, currentPage * pageSize)
  const isReplacing = editingKey !== null || secrets.some((secret) => secret.key === form.key.trim())

  useEffect(() => {
    if (showForm) (editingKey === null ? keyRef : valueRef).current?.focus()
  }, [showForm, editingKey])

  function closeForm() {
    setShowForm(false)
    setEditingKey(null)
    setForm(emptyForm)
    setShowValue(false)
    setSaveError('')
  }

  function openNewForm() {
    if (showForm && editingKey === null) {
      closeForm()
      return
    }
    setForm(emptyForm)
    setEditingKey(null)
    setShowValue(false)
    setSaveError('')
    setShowForm(true)
  }

  function openReplaceForm(secret: SecretMetadata) {
    setForm({ key: secret.key, value: '' })
    setEditingKey(secret.key)
    setShowValue(false)
    setSaveError('')
    setShowForm(true)
  }

  function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const key = form.key.trim()
    if (!key || !form.value || upsert.isPending) return
    setSaveError('')
    upsert.mutate(
      { key, value: form.value },
      {
        onSuccess: () => {
          closeForm()
          setSearch(key)
          setPage(1)
          toast.success(t(isReplacing ? 'secrets.replaced' : 'secrets.saved'))
        },
        onError: () => setSaveError(t('secrets.saveError')),
      },
    )
  }

  return (
    <div className="min-w-0 space-y-5">
      <header className="flex flex-wrap items-center justify-between gap-3">
        <h1 className="text-xl font-bold sm:text-2xl">{t('secrets.title')}</h1>
        <Button
          type="button"
          size="sm"
          className="min-h-10 sm:min-h-10"
          aria-expanded={showForm && editingKey === null}
          disabled={upsert.isPending}
          onClick={openNewForm}
        >
          <Plus className="h-4 w-4" aria-hidden />
          {t('secrets.add')}
        </Button>
      </header>

      <div className="flex items-start gap-2 border-y border-border py-3 text-xs text-text-secondary">
        <ShieldCheck className="h-4 w-4 shrink-0 text-accent" aria-hidden />
        <p>{t('secrets.encryptedNotice')}</p>
      </div>

      {showForm && (
        <form
          onSubmit={handleSubmit}
          aria-label={t(editingKey !== null ? 'secrets.replace' : 'secrets.add')}
          aria-busy={upsert.isPending}
          className="grid gap-3 border-b border-border pb-4 sm:grid-cols-2"
        >
          <div className="min-w-0 space-y-1.5">
            {editingKey !== null ? (
              <>
                <span className="text-sm font-medium">{t('secrets.key')}</span>
                <p className="flex min-h-10 min-w-0 items-center break-all border-b border-border py-2 font-mono text-sm">{form.key}</p>
              </>
            ) : (
              <>
                <Label htmlFor="secret-key">{t('secrets.key')}</Label>
                <Input
                  id="secret-key"
                  ref={keyRef}
                  className="min-h-10 min-w-0"
                  required
                  disabled={upsert.isPending}
                  autoComplete="off"
                  placeholder="DATABASE_PASSWORD"
                  value={form.key}
                  onChange={(event) => { setForm({ ...form, key: event.target.value }); setSaveError('') }}
                />
              </>
            )}
          </div>
          <div className="min-w-0 space-y-1.5">
            <Label htmlFor="secret-value">{t('secrets.value')}</Label>
            <div className="relative">
              <Input
                id="secret-value"
                ref={valueRef}
                type={showValue ? 'text' : 'password'}
                className="min-h-10 min-w-0 pr-12"
                required
                disabled={upsert.isPending}
                autoComplete="new-password"
                value={form.value}
                onChange={(event) => { setForm({ ...form, value: event.target.value }); setSaveError('') }}
              />
              <button
                type="button"
                aria-label={t(showValue ? 'secrets.hideValue' : 'secrets.showValue')}
                title={t(showValue ? 'secrets.hideValue' : 'secrets.showValue')}
                aria-pressed={showValue}
                disabled={upsert.isPending}
                className="absolute inset-y-0 right-0 inline-flex w-10 items-center justify-center rounded-md text-text-secondary hover:text-text-primary focus-visible:outline-2 focus-visible:outline-accent"
                onClick={() => setShowValue(!showValue)}
              >
                {showValue ? <EyeOff className="h-4 w-4" aria-hidden /> : <Eye className="h-4 w-4" aria-hidden />}
              </button>
            </div>
          </div>
          {isReplacing && <p className="text-xs text-text-secondary sm:col-span-2">{t('secrets.replaceWarning')}</p>}
          {saveError && <p role="alert" className="text-sm text-danger sm:col-span-2">{saveError}</p>}
          <div className="flex flex-wrap justify-end gap-2 sm:col-span-2">
            <Button type="button" variant="outline" className="min-h-10 sm:min-h-10" disabled={upsert.isPending} onClick={closeForm}>
              {t('common.cancel')}
            </Button>
            <Button type="submit" className="min-h-10 sm:min-h-10" disabled={upsert.isPending || !form.key.trim() || !form.value}>
              {t(isReplacing ? 'secrets.replace' : 'secrets.save')}
            </Button>
          </div>
        </form>
      )}

      <QueryState
        data={secrets}
        isLoading={isLoading}
        error={listError}
        errorMessage={t('secrets.loadError')}
        isEmpty={(list) => list.length === 0}
        empty={{ title: t('secrets.empty') }}
        onRetry={() => void refetch()}
      >
        {() => (
          <section aria-label={t('secrets.title')} className="space-y-3">
            <div className="relative max-w-xl">
              <Search className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-text-muted" aria-hidden />
              <Input
                type="search"
                aria-label={t('secrets.search')}
                placeholder={t('secrets.search')}
                className="min-h-10 pl-9"
                value={search}
                onChange={(event) => { setSearch(event.target.value); setPage(1) }}
              />
            </div>
            <p className="text-xs text-text-muted">
              {t('secrets.shown', { count: visibleSecrets.length, total: filteredSecrets.length })}
            </p>
            {filteredSecrets.length === 0 ? (
              <p role="status" className="border-y border-border py-6 text-sm text-text-muted">{t('secrets.noMatches')}</p>
            ) : (
              <ul className="divide-y divide-border border-y border-border">
                {visibleSecrets.map((secret) => (
                  <li key={secret.id} className="flex min-w-0 items-center gap-2 py-2">
                    <KeyRound className="h-4 w-4 shrink-0 text-accent" aria-hidden />
                    <div className="min-w-0 flex-1">
                      <p className="break-all font-mono text-sm font-medium">{secret.key}</p>
                      <time
                        dateTime={secret.updated_at}
                        title={new Date(secret.updated_at).toLocaleString()}
                        className="text-xs text-text-muted"
                      >
                        <span className="sr-only sm:not-sr-only">{t('secrets.updated')}: </span>
                        {new Date(secret.updated_at).toLocaleString(undefined, { dateStyle: 'short', timeStyle: 'short' })}
                      </time>
                    </div>
                    <button
                      type="button"
                      aria-label={t('secrets.replaceFor', { name: secret.key })}
                      title={t('secrets.replaceFor', { name: secret.key })}
                      disabled={upsert.isPending}
                      className="inline-flex h-10 w-10 shrink-0 items-center justify-center rounded-md text-text-secondary hover:bg-surface-raised hover:text-text-primary focus-visible:outline-2 focus-visible:outline-accent disabled:opacity-50"
                      onClick={() => openReplaceForm(secret)}
                    >
                      <RotateCw className="h-4 w-4" aria-hidden />
                    </button>
                    <button
                      type="button"
                      aria-label={t('secrets.deleteFor', { name: secret.key })}
                      title={t('secrets.deleteFor', { name: secret.key })}
                      disabled={remove.isPending}
                      className="inline-flex h-10 w-10 shrink-0 items-center justify-center rounded-md text-danger hover:bg-surface-raised focus-visible:outline-2 focus-visible:outline-accent disabled:opacity-50"
                      onClick={() => { setPendingDelete(secret); setDeleteError('') }}
                    >
                      <Trash2 className="h-4 w-4" aria-hidden />
                    </button>
                  </li>
                ))}
              </ul>
            )}
            {filteredSecrets.length > pageSize && (
              <nav aria-label={t('secrets.pages')} className="flex items-center justify-end gap-2">
                <Button type="button" size="sm" variant="outline" className="min-h-10 sm:min-h-10" disabled={currentPage === 1} onClick={() => setPage(currentPage - 1)}>
                  {t('secrets.previous')}
                </Button>
                <span className="text-sm text-text-muted">{currentPage} / {totalPages}</span>
                <Button type="button" size="sm" variant="outline" className="min-h-10 sm:min-h-10" disabled={currentPage === totalPages} onClick={() => setPage(currentPage + 1)}>
                  {t('secrets.next')}
                </Button>
              </nav>
            )}
          </section>
        )}
      </QueryState>

      <ConfirmDialog
        open={pendingDelete !== null}
        title={pendingDelete ? `${t('secrets.deleteConfirm')} "${pendingDelete.key}"?` : ''}
        description={t('secrets.deleteWarning')}
        error={deleteError || undefined}
        pending={remove.isPending}
        closeOnConfirm={false}
        onCancel={() => { setPendingDelete(null); setDeleteError('') }}
        onConfirm={() => {
          if (!pendingDelete || remove.isPending) return
          setDeleteError('')
          remove.mutate(pendingDelete.id, {
            onSuccess: () => { setPendingDelete(null); toast.success(t('secrets.deleted')) },
            onError: () => setDeleteError(t('secrets.deleteError')),
          })
        }}
      />
    </div>
  )
}
