import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useParams } from 'react-router'
import { useEnvironments, useCreateEnvironment, useDeleteEnvironment, useDeployments, useCreateDeployment } from '@/api/hooks'
import { Card } from '@/shared/ui/card'
import { Button } from '@/shared/ui/button'
import { Input } from '@/shared/ui/input'
import { Label } from '@/shared/ui/label'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/shared/ui/table'
import { Globe, Plus, Trash2, Rocket } from 'lucide-react'
import { toast } from 'sonner'
import type { Environment } from '@/api/types'

export function EnvironmentsPage() {
  const { t } = useTranslation()
  const { projectId } = useParams()
  const { data: environments = [], isLoading } = useEnvironments(projectId)
  const createEnv = useCreateEnvironment(projectId)
  const deleteEnv = useDeleteEnvironment()
  const [showForm, setShowForm] = useState(false)
  const [form, setForm] = useState({ name: '', url: '' })
  const [selectedEnv, setSelectedEnv] = useState<Environment | null>(null)

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Globe className="h-5 w-5 text-accent" />
          <h1 className="text-2xl font-bold">{t('environments.title')}</h1>
        </div>
        <Button size="sm" onClick={() => setShowForm(v => !v)}>
          <Plus className="h-4 w-4" />
          {t('environments.create')}
        </Button>
      </div>

      {showForm && (
        <Card className="p-4">
          <form onSubmit={(e) => {
            e.preventDefault()
            createEnv.mutate({ name: form.name, url: form.url || undefined }, {
              onSuccess: () => { setShowForm(false); setForm({ name: '', url: '' }); toast.success(t('environments.created')) },
              onError: (err) => toast.error(err.message),
            })
          }} className="grid gap-3 sm:grid-cols-2">
            <div className="space-y-1.5">
              <Label htmlFor="env-name">{t('environments.name')}</Label>
              <Input id="env-name" required value={form.name} onChange={e => setForm({ ...form, name: e.target.value })} />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="env-url">{t('environments.url')}</Label>
              <Input id="env-url" placeholder="https://app.example.com" value={form.url} onChange={e => setForm({ ...form, url: e.target.value })} />
            </div>
            <div className="flex gap-2 sm:col-span-2">
              <Button type="submit" disabled={createEnv.isPending}>{t('environments.create')}</Button>
              <Button type="button" variant="ghost" onClick={() => setShowForm(false)}>{t('common.cancel')}</Button>
            </div>
          </form>
        </Card>
      )}

      {isLoading ? (
        <p className="text-sm text-text-muted">{t('common.loading')}</p>
      ) : environments.length === 0 ? (
        <Card className="p-8 text-center"><p className="text-text-muted">{t('environments.empty')}</p></Card>
      ) : (
        <div className="space-y-3">
          {environments.map(env => (
            <Card key={env.id} className="p-4">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <span className="font-medium">{env.name}</span>
                  <span className={`rounded-full px-2 py-0.5 text-xs ${
                    env.status === 'available' ? 'bg-emerald-500/15 text-emerald-500' : env.status === 'degraded' ? 'bg-amber-500/15 text-amber-500' : 'bg-surface-raised text-text-muted'
                  }`}>{env.status}</span>
                  {env.url && <a href={env.url} target="_blank" rel="noreferrer" className="text-xs text-accent hover:underline">{env.url}</a>}
                </div>
                <div className="flex gap-1">
                  <Button size="sm" variant="ghost" className="h-7 gap-1 px-2 text-xs" onClick={() => setSelectedEnv(selectedEnv?.id === env.id ? null : env)}>
                    <Rocket className="h-3 w-3" /> {t('environments.deployments')}
                  </Button>
                  <Button size="sm" variant="ghost" className="h-7 gap-1 px-2 text-xs text-danger hover:text-danger" onClick={() => {
                    if (window.confirm(`${t('environments.deleteConfirm')} "${env.name}"?`)) deleteEnv.mutate(env.id, { onSuccess: () => toast.success(t('environments.deleted')), onError: (err) => toast.error(err.message) })
                  }}>
                    <Trash2 className="h-3 w-3" />
                  </Button>
                </div>
              </div>
              {selectedEnv?.id === env.id && <DeploymentsSection environmentId={env.id} />}
            </Card>
          ))}
        </div>
      )}
    </div>
  )
}

function DeploymentsSection({ environmentId }: { environmentId: string }) {
  const { t } = useTranslation()
  const { data: deployments = [], isLoading } = useDeployments(environmentId)
  const createDep = useCreateDeployment(environmentId)
  const [form, setForm] = useState({ git_ref: '' })

  return (
    <div className="mt-4 space-y-3 border-t border-border pt-4">
      <form onSubmit={(e) => {
        e.preventDefault()
        createDep.mutate({ git_ref: form.git_ref }, {
          onSuccess: () => { setForm({ git_ref: '' }); toast.success(t('deployments.created')) },
          onError: (err) => toast.error(err.message),
        })
      }} className="flex gap-2">
        <Input required placeholder="main" value={form.git_ref} onChange={e => setForm({ git_ref: e.target.value })} className="max-w-48" />
        <Button type="submit" size="sm" disabled={createDep.isPending}>{t('deployments.create')}</Button>
      </form>
      {isLoading ? (
        <p className="text-xs text-text-muted">{t('common.loading')}</p>
      ) : deployments.length === 0 ? (
        <p className="text-xs text-text-muted">{t('deployments.empty')}</p>
      ) : (
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>{t('deployments.gitRef')}</TableHead>
              <TableHead>{t('deployments.status')}</TableHead>
              <TableHead>{t('deployments.created')}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {deployments.map(d => (
              <TableRow key={d.id}>
                <TableCell className="font-mono text-sm">{d.git_ref}</TableCell>
                <TableCell className="text-xs">{d.status}</TableCell>
                <TableCell className="text-xs text-text-muted">{new Date(d.created_at).toLocaleString()}</TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}
    </div>
  )
}