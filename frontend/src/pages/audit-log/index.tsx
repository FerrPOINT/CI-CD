import { useTranslation } from 'react-i18next'
import { useAuditLog } from '@/api/hooks'
import { Card } from '@sdlc/ui/ui'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@sdlc/ui/ui'
import { History } from 'lucide-react'

import { UserAvatar } from '@/shared/ui/user-avatar'

export function AuditLogPage() {
  const { t } = useTranslation()
  const { data: events = [], isLoading } = useAuditLog()

  return (
    <div className="space-y-6">
      <div className="flex items-center gap-2">
        <History className="h-5 w-5 text-accent" />
        <h1 className="text-2xl font-bold">{t('auditLog.title')}</h1>
      </div>

      {isLoading ? (
        <p className="text-sm text-text-muted">{t('common.loading')}</p>
      ) : events.length === 0 ? (
        <Card className="p-8 text-center"><p className="text-text-muted">{t('auditLog.empty')}</p></Card>
      ) : (
        <Card>
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
                  <TableCell className="font-mono text-xs">{e.action}</TableCell>
                  <TableCell className="text-xs">
                    <span className="text-text-muted">{e.resource_type}</span>
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
                  <TableCell className="text-xs text-text-muted">{new Date(e.created_at).toLocaleString()}</TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </Card>
      )}
    </div>
  )
}