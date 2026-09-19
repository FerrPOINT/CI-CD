import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { QueryState } from '@/shared/ui/query-state'
import { useAuditLog } from '@/api/hooks'
import { Button, Card } from '@sdlc/ui/ui'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@sdlc/ui/ui'
import { History } from 'lucide-react'

import { UserAvatar } from '@/shared/ui/user-avatar'

export function AuditLogPage() {
  const { t, i18n } = useTranslation()
  const { data: events = [], isLoading, error: listError } = useAuditLog()
  const [search, setSearch] = useState('')
  const [action, setAction] = useState('')
  const [page, setPage] = useState(1)
  const timeFormatter = new Intl.DateTimeFormat(i18n.language, { dateStyle: 'short', timeStyle: 'short' })
  const actions = [...new Set(events.map(event => event.action))].sort()
  const query = search.trim().toLocaleLowerCase()
  const filteredEvents = events.filter(event => {
    if (action && event.action !== action) return false
    if (!query) return true
    return [
      t(`auditLog.actions.${event.action}`, { defaultValue: event.action }),
      t(`auditLog.resources.${event.resource_type}`, { defaultValue: event.resource_type }),
      event.actor ?? '',
      event.resource_id ?? '',
    ].some(value => value.toLocaleLowerCase().includes(query))
  })
  const pageCount = Math.max(1, Math.ceil(filteredEvents.length / 20))
  const currentPage = Math.min(page, pageCount)
  const visibleEvents = filteredEvents.slice((currentPage - 1) * 20, currentPage * 20)

  return (
    <div className="space-y-4">
      <div className="flex items-center gap-2">
        <History className="h-5 w-5 text-accent" />
        <h1 className="text-2xl font-bold">{t('auditLog.title')}</h1>
      </div>

      {events.length > 0 && (
        <div className="flex flex-wrap items-center gap-2">
          <input
            type="search"
            aria-label={t('auditLog.search')}
            placeholder={t('auditLog.search')}
            value={search}
            onChange={event => { setSearch(event.target.value); setPage(1) }}
            className="min-h-10 w-full min-w-0 rounded-md border border-border bg-surface px-3 py-2 text-sm text-text-primary outline-none focus-visible:border-accent sm:w-auto sm:max-w-xs sm:flex-1"
          />
          <select
            aria-label={t('auditLog.actionFilter')}
            value={action}
            onChange={event => { setAction(event.target.value); setPage(1) }}
            className="min-h-10 min-w-0 flex-1 rounded-md border border-border bg-surface px-3 py-2 text-sm text-text-primary outline-none focus-visible:border-accent sm:flex-none"
          >
            <option value="">{t('auditLog.allActions')}</option>
            {actions.map(value => <option key={value} value={value}>{t(`auditLog.actions.${value}`, { defaultValue: value })}</option>)}
          </select>
          <span className="text-xs text-text-muted">{t('auditLog.resultCount', { count: filteredEvents.length })}</span>
        </div>
      )}

      <QueryState data={events} isLoading={isLoading} error={listError} isEmpty={(list) => list.length === 0} empty={{ title: t('auditLog.empty') }}>
        {() => (
          filteredEvents.length === 0 ? (
            <p role="status" className="border-y border-border py-5 text-sm text-text-muted">{t('auditLog.noMatches')}</p>
          ) : <>
            <ul className="divide-y divide-border border-y border-border md:hidden">
              {visibleEvents.map(e => (
                <li key={e.id} className="min-w-0 space-y-2 py-3 text-sm">
                  <div className="flex min-w-0 items-start justify-between gap-3">
                    <span className="min-w-0 break-words text-xs font-medium">
                      {t(`auditLog.actions.${e.action}`, { defaultValue: e.action })}
                    </span>
                    <time className="shrink-0 text-xs text-text-muted" dateTime={e.created_at}>
                      {timeFormatter.format(new Date(e.created_at))}
                    </time>
                  </div>
                  <dl className="grid min-w-0 grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-xs">
                    <dt className="text-text-muted">{t('auditLog.resource')}</dt>
                    <dd className="min-w-0 break-all">
                      {t(`auditLog.resources.${e.resource_type}`, { defaultValue: e.resource_type })}
                      {e.resource_id && <span className="ml-2 font-mono">{e.resource_id.slice(0, 8)}</span>}
                    </dd>
                    <dt className="text-text-muted">{t('auditLog.actor')}</dt>
                    <dd className="min-w-0 break-words">{e.actor || '—'}</dd>
                  </dl>
                </li>
              ))}
            </ul>
            <Card className="hidden md:block">
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
                  {visibleEvents.map(e => (
                    <TableRow key={e.id}>
                      <TableCell className="text-xs">{t(`auditLog.actions.${e.action}`, { defaultValue: e.action })}</TableCell>
                      <TableCell className="text-xs">
                        <span className="text-text-muted">{t(`auditLog.resources.${e.resource_type}`, { defaultValue: e.resource_type })}</span>
                        {e.resource_id && <span className="ml-2 font-mono text-text-muted">{e.resource_id.slice(0, 8)}</span>}
                      </TableCell>
                      <TableCell className="text-xs text-text-muted">
                        {e.actor ? (
                          <span className="inline-flex items-center gap-1.5">
                            <UserAvatar name={e.actor} size="xs" />
                            <span>{e.actor}</span>
                          </span>
                        ) : ('—')}
                      </TableCell>
                      <TableCell className="text-xs text-text-muted">{timeFormatter.format(new Date(e.created_at))}</TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </Card>
          </>
        )}
      </QueryState>

      {filteredEvents.length > 20 && (
        <nav aria-label={t('auditLog.pages')} className="flex items-center justify-end gap-2">
          <Button type="button" variant="outline" size="sm" className="min-h-10" disabled={currentPage === 1} onClick={() => setPage(currentPage - 1)}>{t('auditLog.previous')}</Button>
          <span className="text-sm tabular-nums text-text-muted">{currentPage} / {pageCount}</span>
          <Button type="button" variant="outline" size="sm" className="min-h-10" disabled={currentPage === pageCount} onClick={() => setPage(currentPage + 1)}>{t('auditLog.next')}</Button>
        </nav>
      )}
      {events.length > 0 && <p className="text-xs text-text-muted">{t('auditLog.scope')}</p>}
    </div>
  )
}
