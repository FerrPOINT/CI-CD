import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useRunners, useRegisterRunner, useRunnerHeartbeat, useDeleteRunner } from '@/api/hooks'
import { Card } from '@sdlc/ui/ui'
import { Button } from '@sdlc/ui/ui'
import { Input } from '@sdlc/ui/ui'
import { Label } from '@sdlc/ui/ui'
import { ConfirmDialog } from '@/shared/ui/confirm-dialog'
import { CapabilityCallout } from '@/shared/ui/capability-callout'
import { Server, Plus, Activity, Trash2 } from 'lucide-react'
import { toast } from 'sonner'
import type { Runner } from '@/api/types'

export function RunnersPage() {
  const { t } = useTranslation()
  const { data: runners = [], isLoading } = useRunners()
  const registerRunner = useRegisterRunner()
  const heartbeat = useRunnerHeartbeat()
  const deleteRunner = useDeleteRunner()
  const [showForm, setShowForm] = useState(false)
  const [form, setForm] = useState({ name: '', tags: '' })
  const [pendingDelete, setPendingDelete] = useState<Runner | null>(null)

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
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex min-w-0 items-center gap-2">
          <Server className="h-5 w-5 shrink-0 text-accent" />
          <h1 className="text-xl font-bold sm:text-2xl">{t('runners.title')}</h1>
        </div>
        <Button size="sm" className="shrink-0" onClick={() => setShowForm(v => !v)}>
          <Plus className="h-4 w-4" />
          {t('runners.register')}
        </Button>
      </div>

      <CapabilityCallout
        tone="mvp"
        title={t('runners.capabilityTitle')}
        label={t('capability.currentMvp')}
        description={t('runners.capabilityDescription')}
      />

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
        <ul className="grid gap-3 md:hidden" aria-label={t('runners.title')}>
          {runners.map(r => (
            <li key={r.id}>
              <Card className="p-4">
                <div className="flex items-start justify-between gap-3">
                  <div className="min-w-0">
                    <p className="truncate font-medium">{r.name}</p>
                    <p className="mt-0.5 text-xs text-text-muted">{r.tags.length ? r.tags.join(', ') : '—'}</p>
                  </div>
                  <span className={`shrink-0 rounded-full px-2 py-0.5 text-xs ${
                    r.status === 'online' ? 'bg-emerald-500/15 text-emerald-500' : r.status === 'paused' ? 'bg-amber-500/15 text-amber-500' : 'bg-surface-raised text-text-muted'
                  }`}>
                    {t(`runners.status_${r.status}`)}
                  </span>
                </div>
                <p className="mt-2 text-xs text-text-muted">{t('runners.lastSeen')}: {r.last_seen_at ? new Date(r.last_seen_at).toLocaleString() : '—'}</p>
                <div className="mt-3 flex gap-2">
                  <Button size="sm" variant="outline" className="min-h-9 flex-1" onClick={() =>
                    heartbeat.mutate({ id: r.id }, { onSuccess: () => toast.success(t('runners.heartbeatSent')), onError: (err) => toast.error(err.message) })
                  }>
                    <Activity className="h-3 w-3" /> {t('runners.heartbeat')}
                  </Button>
                  <Button size="sm" variant="ghost" aria-label={`${t('common.delete')} ${r.name}`} className="min-h-9 text-danger hover:text-danger" onClick={() => setPendingDelete(r)}>
                    <Trash2 className="h-3 w-3" />
                  </Button>
                </div>
              </Card>
            </li>
          ))}
        </ul>
      )}

      {runners.length > 0 && (
        <Card className="hidden md:block">
          <table className="w-full text-sm">
            <caption className="sr-only">{t('runners.title')}</caption>
            <thead>
              <tr className="border-b border-border text-left text-xs uppercase tracking-wide text-text-muted">
                <th scope="col" className="px-4 py-3">{t('runners.name')}</th>
                <th scope="col" className="px-4 py-3">{t('runners.tags')}</th>
                <th scope="col" className="px-4 py-3">{t('runners.status')}</th>
                <th scope="col" className="px-4 py-3">{t('runners.lastSeen')}</th>
                <th scope="col" className="w-28 px-4 py-3"><span className="sr-only">{t('common.delete')}</span></th>
              </tr>
            </thead>
            <tbody>
              {runners.map(r => (
                <tr key={r.id} className="border-b border-border/50 last:border-0">
                  <td className="px-4 py-3 font-medium">{r.name}</td>
                  <td className="px-4 py-3 text-xs text-text-muted">{r.tags.length ? r.tags.join(', ') : '—'}</td>
                  <td className="px-4 py-3">
                    <span className={`inline-flex items-center gap-1.5 rounded-full px-2 py-0.5 text-xs ${
                      r.status === 'online' ? 'bg-emerald-500/15 text-emerald-500' : r.status === 'paused' ? 'bg-amber-500/15 text-amber-500' : 'bg-surface-raised text-text-muted'
                    }`}>
                      <span aria-hidden className={`h-1.5 w-1.5 rounded-full ${r.status === 'online' ? 'bg-emerald-500' : 'bg-text-muted'}`} />
                      {t(`runners.status_${r.status}`)}
                    </span>
                  </td>
                  <td className="px-4 py-3 text-xs text-text-muted">{r.last_seen_at ? new Date(r.last_seen_at).toLocaleString() : '—'}</td>
                  <td className="px-4 py-3">
                    <div className="flex gap-1">
                      <Button size="sm" variant="ghost" className="h-7 gap-1 px-2 text-xs" onClick={() =>
                        heartbeat.mutate({ id: r.id }, { onSuccess: () => toast.success(t('runners.heartbeatSent')), onError: (err) => toast.error(err.message) })
                      }>
                        <Activity className="h-3 w-3" /> {t('runners.heartbeat')}
                      </Button>
                      <Button size="sm" variant="ghost" aria-label={`${t('common.delete')} ${r.name}`} className="h-7 px-2 text-danger hover:text-danger" onClick={() => setPendingDelete(r)}>
                        <Trash2 className="h-3 w-3" />
                      </Button>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>
      )}

      <ConfirmDialog
        open={pendingDelete !== null}
        title={`${t('runners.deleteConfirm')}${pendingDelete ? ` "${pendingDelete.name}"?` : '?'}`}
        description={t('runners.deleteWarning')}
        onCancel={() => setPendingDelete(null)}
        onConfirm={() => {
          if (pendingDelete) {
            deleteRunner.mutate(pendingDelete.id, { onSuccess: () => toast.success(t('runners.deleted')), onError: (err) => toast.error(err.message) })
          }
          setPendingDelete(null)
        }}
      />
    </div>
  )
}
