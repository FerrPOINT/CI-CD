import { useTranslation } from 'react-i18next'
import { QueryState } from '@/shared/ui/query-state'
import { useAuditLog } from '@/api/hooks'
import { Card } from '@sdlc/ui/ui'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@sdlc/ui/ui'
import { History } from 'lucide-react'

import { UserAvatar } from '@/shared/ui/user-avatar'

export function AuditLogPage() {
  const { t, i18n } = useTranslation()
  const { data: events = [], isLoading, error: listError } = useAuditLog()
  const timeFormatter = new Intl.DateTimeFormat(i18n.language, { dateStyle: 'short', timeStyle: 'short' })

  return (
    <div className="space-y-6">
      <div className="flex items-center gap-2">
        <History className="h-5 w-5 text-accent" />
        <h1 className="text-2xl font-bold">{t('auditLog.title')}</h1>
      </div>

      <QueryState data={events} isLoading={isLoading} error={listError} isEmpty={(list) => list.length === 0} empty={{ title: t('auditLog.empty') }}>
        {() => (
          <>
            <ul className="divide-y divide-border border-y border-border md:hidden">
              {events.map(e => (
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
                  {events.map(e => (
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
    </div>
  )
}
