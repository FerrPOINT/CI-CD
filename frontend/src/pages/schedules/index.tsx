import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useParams } from 'react-router'
import { useSchedules, useCreateSchedule, useDeleteSchedule } from '@/api/hooks'
import { Card } from '@/shared/ui/card'
import { Button } from '@/shared/ui/button'
import { Input } from '@/shared/ui/input'
import { Label } from '@/shared/ui/label'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/shared/ui/table'
import { Clock, Plus, Trash2 } from 'lucide-react'
import { toast } from 'sonner'
import type { Schedule } from '@/api/types'

export function SchedulesPage() {
  const { t } = useTranslation()
  const { projectId } = useParams()
  const { data: schedules = [], isLoading } = useSchedules(projectId)
  const createSchedule = useCreateSchedule(projectId)
  const deleteSchedule = useDeleteSchedule()
  const [showForm, setShowForm] = useState(false)
  const [form, setForm] = useState({ cron: '', git_ref: 'main' })

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    createSchedule.mutate(form, {
      onSuccess: () => { setShowForm(false); setForm({ cron: '', git_ref: 'main' }); toast.success(t('schedules.created')) },
      onError: (err) => toast.error(err.message),
    })
  }

  function handleDelete(s: Schedule) {
    if (!window.confirm(`${t('schedules.deleteConfirm')}?`)) return
    deleteSchedule.mutate(s.id, { onSuccess: () => toast.success(t('schedules.deleted')), onError: (err) => toast.error(err.message) })
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Clock className="h-5 w-5 text-accent" />
          <h1 className="text-2xl font-bold">{t('schedules.title')}</h1>
        </div>
        <Button size="sm" onClick={() => setShowForm(v => !v)}>
          <Plus className="h-4 w-4" />
          {t('schedules.create')}
        </Button>
      </div>

      {showForm && (
        <Card className="p-4">
          <form onSubmit={handleSubmit} className="grid gap-3 sm:grid-cols-2">
            <div className="space-y-1.5">
              <Label htmlFor="cron">{t('schedules.cron')}</Label>
              <Input id="cron" required placeholder="0 4 * * 1" value={form.cron} onChange={e => setForm({ ...form, cron: e.target.value })} />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="ref">{t('schedules.gitRef')}</Label>
              <Input id="ref" required value={form.git_ref} onChange={e => setForm({ ...form, git_ref: e.target.value })} />
            </div>
            <div className="flex gap-2 sm:col-span-2">
              <Button type="submit" disabled={createSchedule.isPending}>{t('schedules.create')}</Button>
              <Button type="button" variant="ghost" onClick={() => setShowForm(false)}>{t('common.cancel')}</Button>
            </div>
          </form>
        </Card>
      )}

      {isLoading ? (
        <p className="text-sm text-text-muted">{t('common.loading')}</p>
      ) : schedules.length === 0 ? (
        <Card className="p-8 text-center"><p className="text-text-muted">{t('schedules.empty')}</p></Card>
      ) : (
        <Card>
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>{t('schedules.cron')}</TableHead>
                <TableHead>{t('schedules.gitRef')}</TableHead>
                <TableHead>{t('schedules.enabled')}</TableHead>
                <TableHead>{t('schedules.created')}</TableHead>
                <TableHead className="w-20"></TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {schedules.map(s => (
                <TableRow key={s.id}>
                  <TableCell className="font-mono text-sm">{s.cron}</TableCell>
                  <TableCell className="font-mono text-sm">{s.git_ref}</TableCell>
                  <TableCell>
                    <span className={`rounded-full px-2 py-0.5 text-xs ${s.enabled ? 'bg-emerald-500/15 text-emerald-500' : 'bg-surface-raised text-text-muted'}`}>
                      {s.enabled ? t('schedules.enabledOn') : t('schedules.enabledOff')}
                    </span>
                  </TableCell>
                  <TableCell className="text-xs text-text-muted">{new Date(s.created_at).toLocaleString()}</TableCell>
                  <TableCell>
                    <Button size="sm" variant="ghost" className="h-7 gap-1 px-2 text-xs text-danger hover:text-danger" onClick={() => handleDelete(s)}>
                      <Trash2 className="h-3 w-3" />
                    </Button>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </Card>
      )}
    </div>
  )
}