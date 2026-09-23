import { useEffect, useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { useSearchParams } from 'react-router'
import { ChevronLeft, ChevronRight, History, Search, X } from 'lucide-react'

import { useAuditLog } from '@/api/hooks'
import { QueryState } from '@/shared/ui/query-state'
import { UserAvatar } from '@/shared/ui/user-avatar'
import { Button, Card, Input, Label } from '@sdlc/ui/ui'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@sdlc/ui/ui'

const pageSize = 20
const maxAuditPage = Math.floor(2_147_483_647 / pageSize) + 1

export function AuditLogPage() {
  const { t, i18n } = useTranslation()
  const [searchParams, setSearchParams] = useSearchParams()
  const pageParam = searchParams.get('page')
  const rawPage = Number(pageParam)
  const validPage = pageParam === null || (Number.isSafeInteger(rawPage) && rawPage > 0 && rawPage <= maxAuditPage)
  const page = pageParam !== null && validPage ? rawPage : 1
  const action = searchParams.get('action')?.trim() ?? ''
  const query = searchParams.get('q')?.trim() ?? ''
  const [searchDraft, setSearchDraft] = useState(query)
  const auditQuery = useAuditLog({
    limit: pageSize,
    offset: (page - 1) * pageSize,
    action: action || undefined,
    q: query || undefined,
  })
  const hasFilters = Boolean(action || query)
  const timeFormatter = new Intl.DateTimeFormat(i18n.language, { dateStyle: 'short', timeStyle: 'short' })

  useEffect(() => setSearchDraft(query), [query])

  useEffect(() => {
    if (validPage) return
    setSearchParams((current) => {
      const next = new URLSearchParams(current)
      next.delete('page')
      return next
    }, { replace: true })
  }, [setSearchParams, validPage])

  useEffect(() => {
    if (!auditQuery.data || auditQuery.isPlaceholderData) return
    const lastPage = Math.max(1, Math.ceil(auditQuery.data.total / pageSize))
    if (page <= lastPage) return
    setSearchParams((current) => {
      const next = new URLSearchParams(current)
      if (lastPage === 1) next.delete('page')
      else next.set('page', String(lastPage))
      return next
    }, { replace: true })
  }, [auditQuery.data, auditQuery.isPlaceholderData, page, setSearchParams])

  function changePage(nextPage: number) {
    setSearchParams((current) => {
      const next = new URLSearchParams(current)
      if (nextPage <= 1) next.delete('page')
      else next.set('page', String(nextPage))
      return next
    })
  }

  function changeAction(nextAction: string) {
    setSearchParams((current) => {
      const next = new URLSearchParams(current)
      if (nextAction) next.set('action', nextAction)
      else next.delete('action')
      next.delete('page')
      return next
    }, { replace: true })
  }

  function submitSearch(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const nextQuery = searchDraft.trim()
    setSearchParams((current) => {
      const next = new URLSearchParams(current)
      if (nextQuery) next.set('q', nextQuery)
      else next.delete('q')
      next.delete('page')
      return next
    }, { replace: true })
  }

  function clearSearch() {
    setSearchDraft('')
    setSearchParams((current) => {
      const next = new URLSearchParams(current)
      next.delete('q')
      next.delete('page')
      return next
    }, { replace: true })
  }

  return (
    <div className="space-y-4">
      <header className="flex items-center gap-2">
        <History className="h-5 w-5 text-accent" aria-hidden />
        <h1 className="text-2xl font-bold">{t('auditLog.title')}</h1>
      </header>

      {auditQuery.error && auditQuery.data && (
        <div role="alert" className="flex flex-wrap items-center gap-3 border-y border-border py-3 text-sm text-text-secondary">
          <span>{t('auditLog.refreshFailed')}</span>
          <Button type="button" size="sm" variant="outline" className="min-h-10 sm:min-h-10" onClick={() => void auditQuery.refetch()}>
            {t('common.retry')}
          </Button>
        </div>
      )}

      <QueryState
        data={auditQuery.data}
        isLoading={auditQuery.isLoading}
        error={auditQuery.data ? null : auditQuery.error}
        errorMessage={t('auditLog.loadFailed')}
        onRetry={() => void auditQuery.refetch()}
        isEmpty={(result) => result.total === 0 && !hasFilters}
        empty={{ title: t('auditLog.empty') }}
      >
        {(result) => {
          const pageCount = Math.max(1, Math.ceil(result.total / pageSize))
          const firstItem = result.total === 0 ? 0 : result.offset + 1
          const lastItem = result.offset + result.items.length
          const actionOptions = action && !result.actions.includes(action)
            ? [action, ...result.actions]
            : result.actions

          return (
            <section aria-label={t('auditLog.title')} aria-busy={auditQuery.isFetching} className="space-y-3">
              <div className="grid gap-2 lg:grid-cols-[minmax(18rem,1fr)_minmax(12rem,18rem)]">
                <form onSubmit={submitSearch} role="search" className="flex min-w-0 gap-2">
                  <div className="relative min-w-0 flex-1">
                    <Search className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-text-muted" aria-hidden />
                    <Input
                      type="search"
                      aria-label={t('auditLog.search')}
                      placeholder={t('auditLog.search')}
                      maxLength={128}
                      value={searchDraft}
                      onChange={(event) => setSearchDraft(event.target.value)}
                      className="min-h-10 pl-9 pr-10"
                    />
                    {(searchDraft || query) && (
                      <button
                        type="button"
                        aria-label={t('auditLog.clearSearch')}
                        title={t('auditLog.clearSearch')}
                        className="absolute right-0 top-0 inline-flex h-10 w-10 items-center justify-center text-text-muted hover:text-text-primary focus-visible:outline-2 focus-visible:outline-accent"
                        onClick={clearSearch}
                      >
                        <X className="h-4 w-4" aria-hidden />
                      </button>
                    )}
                  </div>
                  <Button type="submit" size="sm" className="min-h-10 sm:min-h-10" disabled={searchDraft.trim() === query}>
                    <Search className="h-4 w-4" aria-hidden />
                    {t('auditLog.searchButton')}
                  </Button>
                </form>
                <div>
                  <Label htmlFor="audit-action" className="sr-only">{t('auditLog.actionFilter')}</Label>
                  <select
                    id="audit-action"
                    value={action}
                    onChange={(event) => changeAction(event.target.value)}
                    className="min-h-10 w-full min-w-0 rounded-md border border-border bg-surface px-3 py-2 text-sm text-text-primary outline-none focus-visible:border-accent"
                  >
                    <option value="">{t('auditLog.allActions')}</option>
                    {actionOptions.map((value) => (
                      <option key={value} value={value}>
                        {t(`auditLog.actions.${value}`, { defaultValue: value })}
                      </option>
                    ))}
                  </select>
                </div>
              </div>

              <p className="flex flex-wrap gap-2 text-xs text-text-muted" aria-live="polite">
                <span>{t('auditLog.shown', { from: firstItem, to: lastItem, total: result.total })}</span>
                {auditQuery.isFetching && <span>{t('common.loading')}</span>}
              </p>

              {result.total === 0 ? (
                <p role="status" className="border-y border-border py-6 text-sm text-text-muted">{t('auditLog.noMatches')}</p>
              ) : (
                <>
                  <ul className="divide-y divide-border border-y border-border md:hidden">
                    {result.items.map((event) => (
                      <li key={event.id} className="min-w-0 space-y-2 py-3 text-sm">
                        <div className="flex min-w-0 items-start justify-between gap-3">
                          <span className="min-w-0 break-words text-xs font-medium">
                            {t(`auditLog.actions.${event.action}`, { defaultValue: event.action })}
                          </span>
                          <time className="shrink-0 text-xs text-text-muted" dateTime={event.created_at} title={event.created_at}>
                            {timeFormatter.format(new Date(event.created_at))}
                          </time>
                        </div>
                        <dl className="grid min-w-0 grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-xs">
                          <dt className="text-text-muted">{t('auditLog.resource')}</dt>
                          <dd className="min-w-0 break-all">
                            {t(`auditLog.resources.${event.resource_type}`, { defaultValue: event.resource_type })}
                            {event.resource_id && <span className="ml-2 font-mono">{event.resource_id.slice(0, 8)}</span>}
                          </dd>
                          <dt className="text-text-muted">{t('auditLog.actor')}</dt>
                          <dd className="min-w-0 break-words">{event.actor || '—'}</dd>
                        </dl>
                      </li>
                    ))}
                  </ul>
                  <Card className="hidden overflow-hidden md:block">
                    <Table>
                      <TableHeader>
                        <TableRow>
                          <TableHead>{t('auditLog.action')}</TableHead>
                          <TableHead>{t('auditLog.resource')}</TableHead>
                          <TableHead>{t('auditLog.actor')}</TableHead>
                          <TableHead>{t('auditLog.time')}</TableHead>
                        </TableRow>
                      </TableHeader>
                      <TableBody>
                        {result.items.map((event) => (
                          <TableRow key={event.id}>
                            <TableCell className="text-xs">{t(`auditLog.actions.${event.action}`, { defaultValue: event.action })}</TableCell>
                            <TableCell className="text-xs">
                              <span className="text-text-muted">{t(`auditLog.resources.${event.resource_type}`, { defaultValue: event.resource_type })}</span>
                              {event.resource_id && <span className="ml-2 font-mono text-text-muted">{event.resource_id.slice(0, 8)}</span>}
                            </TableCell>
                            <TableCell className="text-xs text-text-muted">
                              {event.actor ? (
                                <span className="inline-flex min-w-0 items-center gap-1.5">
                                  <UserAvatar name={event.actor} size="xs" />
                                  <span className="min-w-0 break-all">{event.actor}</span>
                                </span>
                              ) : ('—')}
                            </TableCell>
                            <TableCell className="text-xs text-text-muted">
                              <time dateTime={event.created_at} title={event.created_at}>{timeFormatter.format(new Date(event.created_at))}</time>
                            </TableCell>
                          </TableRow>
                        ))}
                      </TableBody>
                    </Table>
                  </Card>
                </>
              )}

              {result.total > pageSize && (
                <nav aria-label={t('auditLog.pages')} className="flex items-center justify-end gap-2">
                  <Button type="button" variant="outline" size="sm" className="min-h-10 sm:min-h-10" disabled={page === 1 || auditQuery.isFetching} onClick={() => changePage(page - 1)}>
                    <ChevronLeft className="h-4 w-4" aria-hidden />
                    {t('auditLog.previous')}
                  </Button>
                  <span className="min-w-16 text-center text-sm tabular-nums text-text-muted">{page} / {pageCount}</span>
                  <Button type="button" variant="outline" size="sm" className="min-h-10 sm:min-h-10" disabled={page === pageCount || auditQuery.isFetching} onClick={() => changePage(page + 1)}>
                    {t('auditLog.next')}
                    <ChevronRight className="h-4 w-4" aria-hidden />
                  </Button>
                </nav>
              )}
            </section>
          )
        }}
      </QueryState>
    </div>
  )
}
