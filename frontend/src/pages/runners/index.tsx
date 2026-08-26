import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useRunners, useRegisterRunner, useRunnerHeartbeat, useDeleteRunner } from '@/api/hooks'
import { Card } from '@/shared/ui/card'
import { Button } from '@/shared/ui/button'
import { Input } from '@/shared/ui/input'
import { Label } from '@/shared/ui/label'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/shared/ui/table'
import { Server, Plus, Activity, Trash2 } from 'lucide-react'
import { toast } from 'sonner'

export function RunnersPage() {
  const { t } = useTranslation()
  const { data: runners = [], isLoading } = useRunners()
  const registerRunner = useRegisterRunner()
  const heartbeat = useRunnerHeartbeat()
  const deleteRunner = useDeleteRunner()
  const [showForm, setShowForm] = useState(false)
  const [form, setForm] = useState({ name: '', tags: '' })

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    registerRunner.mutate(
      { name: form.name, tags: form.tags ? form.tags.split(',').map(s => s.trim()).filter(Boolean) : [] },
      {
        onSuccess: () => { setShowForm(false); setForm({ name: '', tags: '' }); toast.success(t('runners.registered')) },
        onError: (err) => toast.error(err.message),
      },
    )
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Server className="h-5 w-5 text-accent" />
          <h1 className="text-2xl font-bold">{t('runners.title')}</h1>
        </div>
        <Button size="sm" onClick={() => setShowForm(v => !v)}>
          <Plus className="h-4 w-4" />
          {t('runners.register')}
        </Button>
      </div>

      {showForm && (
        <Card className="p-4">
          <form onSubmit={handleSubmit} className="grid gap-3 sm:grid-cols-2">
            <div className="space-y-1.5">
              <Label htmlFor="runner-name">{t('runners.name')}</Label>
              <Input id="runner-name" required value={form.name} onChange={e => setForm({ ...form, name: e.target.value })} />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="runner-tags">{t('runners.tags')}</Label>
              <Input id="runner-tags" placeholder="linux, docker" value={form.tags} onChange={e => setForm({ ...form, tags: e.target.value })} />
            </div>
            <div className="flex gap-2 sm:col-span-2">
              <Button type="submit" disabled={registerRunner.isPending}>{t('runners.register')}</Button>
              <Button type="button" variant="ghost" onClick={() => setShowForm(false)}>{t('common.cancel')}</Button>
            </div>
          </form>
        </Card>
      )}

      {isLoading ? (
        <p className="text-sm text-text-muted">{t('common.loading')}</p>
      ) : runners.length === 0 ? (
        <Card className="p-8 text-center"><p className="text-text-muted">{t('runners.empty')}</p></Card>
      ) : (
        <Card>
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>{t('runners.name')}</TableHead>
                <TableHead>{t('runners.tags')}</TableHead>
                <TableHead>{t('runners.status')}</TableHead>
                <TableHead>{t('runners.lastSeen')}</TableHead>
                <TableHead className="w-28"></TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {runners.map(r => (
                <TableRow key={r.id}>
                  <TableCell className="font-medium">{r.name}</TableCell>
                  <TableCell className="text-xs text-text-muted">{r.tags.length ? r.tags.join(', ') : '—'}</TableCell>
                  <TableCell>
                    <span className={`inline-flex items-center gap-1.5 rounded-full px-2 py-0.5 text-xs ${
                      r.status === 'online' ? 'bg-emerald-500/15 text-emerald-500' : r.status === 'paused' ? 'bg-amber-500/15 text-amber-500' : 'bg-surface-raised text-text-muted'
                    }`}>
                      <span className={`h-1.5 w-1.5 rounded-full ${r.status === 'online' ? 'bg-emerald-500' : 'bg-text-muted'}`} />
                      {r.status}
                    </span>
                  </TableCell>
                  <TableCell className="text-xs text-text-muted">{r.last_seen_at ? new Date(r.last_seen_at).toLocaleString() : '—'}</TableCell>
                  <TableCell>
                    <div className="flex gap-1">
                      <Button size="sm" variant="ghost" className="h-7 gap-1 px-2 text-xs" onClick={() =>
                        heartbeat.mutate({ id: r.id }, { onSuccess: () => toast.success(t('runners.heartbeatSent')), onError: (err) => toast.error(err.message) })
                      }>
                        <Activity className="h-3 w-3" /> {t('runners.heartbeat')}
                      </Button>
                      <Button size="sm" variant="ghost" className="h-7 gap-1 px-2 text-xs text-danger hover:text-danger" onClick={() => {
                        if (window.confirm(`${t('runners.deleteConfirm')} "${r.name}"?`)) {
                          deleteRunner.mutate(r.id, { onSuccess: () => toast.success(t('runners.deleted')), onError: (err) => toast.error(err.message) })
                        }
                      }}>
                        <Trash2 className="h-3 w-3" />
                      </Button>
                    </div>
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
