import { useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { Plus, Search, Server, Trash2 } from 'lucide-react'
import { toast } from 'sonner'
import { useDeleteRunner, useRegisterRunner, useRunners } from '@/api/hooks'
import type { Runner, RunnerStatus } from '@/api/types'
import { Button, Input, Label } from '@sdlc/ui/ui'
import { ConfirmDialog } from '@/shared/ui/confirm-dialog'
import { QueryState } from '@/shared/ui/query-state'

const pageSize = 20

function statusDotClass(status: RunnerStatus): string {
  if (status === 'online') return 'bg-success'
  if (status === 'paused') return 'bg-warning'
  return 'bg-text-muted'
}

export function RunnersPage() {
  const { t } = useTranslation()
  const { data: runners = [], isLoading, error: listError, refetch } = useRunners()
  const registerRunner = useRegisterRunner()
  const deleteRunner = useDeleteRunner()
  const [showForm, setShowForm] = useState(false)
  const [form, setForm] = useState({ name: '', tags: '' })
  const [pendingDelete, setPendingDelete] = useState<Runner | null>(null)
  const [search, setSearch] = useState('')
  const [statusFilter, setStatusFilter] = useState<RunnerStatus | 'all'>('all')
  const [page, setPage] = useState(1)

  const normalizedSearch = search.trim().toLocaleLowerCase()
  const filteredRunners = runners
    .filter((runner) =>
      (statusFilter === 'all' || runner.status === statusFilter) &&
      `${runner.name} ${runner.tags.join(' ')}`.toLocaleLowerCase().includes(normalizedSearch),
    )
    .sort((a, b) => a.name.localeCompare(b.name))
  const totalPages = Math.max(1, Math.ceil(filteredRunners.length / pageSize))
  const currentPage = Math.min(page, totalPages)
  const visibleRunners = filteredRunners.slice((currentPage - 1) * pageSize, currentPage * pageSize)

  function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const name = form.name.trim()
    if (!name) return
    registerRunner.mutate(
      { name, tags: form.tags.split(',').map((tag) => tag.trim()).filter(Boolean) },
      {
        onSuccess: () => {
          setShowForm(false)
          setForm({ name: '', tags: '' })
          toast.success(t('runners.registered'))
        },
        onError: () => toast.error(t('runners.registerError')),
      },
    )
  }

  return (
    <div className="space-y-5">
      <header className="flex flex-wrap items-center justify-between gap-3">
        <h1 className="text-2xl font-bold">{t('runners.title')}</h1>
        <Button
          size="sm"
          className="min-h-10 sm:min-h-10"
          aria-expanded={showForm}
          disabled={registerRunner.isPending}
          onClick={() => {
            if (showForm) setForm({ name: '', tags: '' })
            setShowForm(!showForm)
          }}
        >
          <Plus className="h-4 w-4" aria-hidden />
          {t('runners.register')}
        </Button>
      </header>

      {showForm && (
        <form onSubmit={handleSubmit} aria-label={t('runners.register')} className="grid gap-3 border-y border-border py-4 sm:grid-cols-2">
          <div className="space-y-1.5">
            <Label htmlFor="runner-name">{t('runners.name')}</Label>
            <Input id="runner-name" className="min-h-10" required autoFocus value={form.name} onChange={(event) => setForm({ ...form, name: event.target.value })} />
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="runner-tags">{t('runners.tags')}</Label>
            <Input id="runner-tags" className="min-h-10" placeholder="linux, docker" value={form.tags} onChange={(event) => setForm({ ...form, tags: event.target.value })} />
          </div>
          <p className="text-xs text-text-secondary sm:col-span-2">{t('runners.manualNotice')}</p>
          <div className="flex justify-end gap-2 sm:col-span-2">
            <Button type="button" variant="outline" className="min-h-10 sm:min-h-10" disabled={registerRunner.isPending} onClick={() => { setShowForm(false); setForm({ name: '', tags: '' }) }}>
              {t('common.cancel')}
            </Button>
            <Button type="submit" className="min-h-10 sm:min-h-10" disabled={registerRunner.isPending || !form.name.trim()}>
              {t('runners.register')}
            </Button>
          </div>
        </form>
      )}

      <QueryState
        data={runners}
        isLoading={isLoading}
        error={listError}
        errorMessage={t('runners.loadError')}
        isEmpty={(list) => list.length === 0}
        empty={{ title: t('runners.empty') }}
        onRetry={() => void refetch()}
      >
        {() => (
          <section aria-label={t('runners.title')} className="space-y-3">
            <div className="grid gap-3 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-end">
              <div className="relative">
                <Search className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-text-muted" aria-hidden />
                <Input
                  type="search"
                  aria-label={t('runners.search')}
                  placeholder={t('runners.search')}
                  className="min-h-10 pl-9"
                  value={search}
                  onChange={(event) => { setSearch(event.target.value); setPage(1) }}
                />
              </div>
              <div className="flex min-w-0 items-center gap-2">
                <Label htmlFor="runner-status" className="shrink-0">{t('runners.status')}</Label>
                <select
                  id="runner-status"
                  value={statusFilter}
                  onChange={(event) => { setStatusFilter(event.target.value as RunnerStatus | 'all'); setPage(1) }}
                  className="min-h-10 min-w-0 flex-1 rounded-md border border-border bg-surface px-2 text-sm text-text-primary outline-none focus-visible:border-accent sm:w-40"
                >
                  <option value="all">{t('runners.allStatuses')}</option>
                  <option value="online">{t('runners.status_online')}</option>
                  <option value="offline">{t('runners.status_offline')}</option>
                  <option value="paused">{t('runners.status_paused')}</option>
                </select>
              </div>
            </div>
            <p className="text-xs text-text-muted">{t('runners.shown', { count: visibleRunners.length, total: filteredRunners.length })}</p>
            {filteredRunners.length === 0 ? (
              <p role="status" className="border-y border-border py-6 text-sm text-text-muted">{t('runners.noMatches')}</p>
            ) : (
              <ul className="divide-y divide-border border-y border-border">
                {visibleRunners.map((runner) => {
                  const tags = runner.tags.join(', ')
                  return (
                    <li key={runner.id} className="flex min-w-0 items-center gap-2 py-2">
                      <Server className="h-4 w-4 shrink-0 text-accent" aria-hidden />
                      <div className="min-w-0 flex-1">
                        <p className="truncate text-sm font-medium" title={runner.name}>{runner.name}</p>
                        <p className="truncate text-xs text-text-muted" title={tags || undefined}>{tags || t('runners.noTags')}</p>
                        <p className="truncate text-xs text-text-muted">
                          {t('runners.lastSeen')}: {runner.last_seen_at ? new Date(runner.last_seen_at).toLocaleString(undefined, { dateStyle: 'short', timeStyle: 'short' }) : t('runners.neverSeen')}
                        </p>
                      </div>
                      <span className="inline-flex max-w-24 shrink-0 items-center gap-1.5 truncate rounded bg-surface-raised px-2 py-1 text-xs text-text-primary">
                        <span aria-hidden className={`h-1.5 w-1.5 shrink-0 rounded-full ${statusDotClass(runner.status)}`} />
                        {t(`runners.status_${runner.status}`)}
                      </span>
                      <button
                        type="button"
                        aria-label={t('runners.deleteRunner', { name: runner.name })}
                        title={t('runners.deleteRunner', { name: runner.name })}
                        disabled={deleteRunner.isPending}
                        className="inline-flex h-10 w-10 shrink-0 items-center justify-center rounded-md text-danger hover:bg-surface-raised focus-visible:outline-2 focus-visible:outline-accent disabled:opacity-50"
                        onClick={() => setPendingDelete(runner)}
                      >
                        <Trash2 className="h-4 w-4" aria-hidden />
                      </button>
                    </li>
                  )
                })}
              </ul>
            )}
            {filteredRunners.length > pageSize && (
              <nav aria-label={t('runners.pages')} className="flex items-center justify-end gap-2">
                <Button type="button" size="sm" variant="outline" className="min-h-10 sm:min-h-10" disabled={currentPage === 1} onClick={() => setPage(currentPage - 1)}>
                  {t('runners.previous')}
                </Button>
                <span className="text-sm text-text-muted">{currentPage} / {totalPages}</span>
                <Button type="button" size="sm" variant="outline" className="min-h-10 sm:min-h-10" disabled={currentPage === totalPages} onClick={() => setPage(currentPage + 1)}>
                  {t('runners.next')}
                </Button>
              </nav>
            )}
          </section>
        )}
      </QueryState>

      <ConfirmDialog
        open={pendingDelete !== null}
        title={pendingDelete ? `${t('runners.deleteConfirm')} "${pendingDelete.name}"?` : ''}
        description={t('runners.deleteWarning')}
        pending={deleteRunner.isPending}
        closeOnConfirm={false}
        onCancel={() => setPendingDelete(null)}
        onConfirm={() => {
          if (!pendingDelete) return
          deleteRunner.mutate(pendingDelete.id, {
            onSuccess: () => { setPendingDelete(null); toast.success(t('runners.deleted')) },
            onError: () => toast.error(t('runners.deleteError')),
          })
        }}
      />
    </div>
  )
}
