import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useParams } from 'react-router'
import { useEnvironments, useCreateEnvironment, useDeleteEnvironment, useDeployments, useCreateDeployment, useRecordDeploymentApproval, useRollbackDeployment } from '@/api/hooks'
import { Card } from '@/shared/ui/card'
import { Button } from '@/shared/ui/button'
import { Input } from '@/shared/ui/input'
import { Label } from '@/shared/ui/label'
import { CapabilityCallout } from '@/shared/ui/capability-callout'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/shared/ui/table'
import { CheckCircle2, Globe, Plus, RotateCcw, Trash2, Rocket, XCircle } from 'lucide-react'
import { toast } from 'sonner'
import { ConfirmDialog } from '@/shared/ui/confirm-dialog'
import type { Deployment, Environment } from '@/api/types'

export function EnvironmentsPage() {
  const { t } = useTranslation()
  const { projectId } = useParams()
  const { data: environments = [], isLoading } = useEnvironments(projectId)
  const createEnv = useCreateEnvironment(projectId)
  const deleteEnv = useDeleteEnvironment()
  const [showForm, setShowForm] = useState(false)
  const [pendingEnv, setPendingEnv] = useState<Environment | null>(null)
  const [form, setForm] = useState({ name: '', url: '', protected: false, required_approvals: 1 })
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

      <CapabilityCallout
        tone="mvp"
        title={t('environments.capabilityTitle')}
        label={t('capability.currentMvp')}
        description={t('environments.capabilityDescription')}
      />

      {showForm && (
        <Card className="p-4">
          <form onSubmit={(e) => {
            e.preventDefault()
            createEnv.mutate({
              name: form.name,
              url: form.url || undefined,
              protected: form.protected,
              required_approvals: form.protected ? form.required_approvals : 0,
            }, {
              onSuccess: () => { setShowForm(false); setForm({ name: '', url: '', protected: false, required_approvals: 1 }); toast.success(t('environments.created')) },
              onError: (err) => toast.error(err.message),
            })
          }} className="grid gap-3 sm:grid-cols-2 lg:grid-cols-[minmax(0,1fr)_minmax(0,1.4fr)_auto]">
            <div className="space-y-1.5">
              <Label htmlFor="env-name">{t('environments.name')}</Label>
              <Input id="env-name" required value={form.name} onChange={e => setForm({ ...form, name: e.target.value })} />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="env-url">{t('environments.url')}</Label>
              <Input id="env-url" placeholder="https://app.example.com" value={form.url} onChange={e => setForm({ ...form, url: e.target.value })} />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="env-required-approvals">{t('environments.requiredApprovals')}</Label>
              <Input
                id="env-required-approvals"
                type="number"
                min={1}
                max={10}
                disabled={!form.protected}
                value={form.required_approvals}
                onChange={e => setForm({ ...form, required_approvals: Number(e.target.value) || 1 })}
              />
            </div>
            <label className="flex items-center gap-2 text-sm text-text-secondary sm:col-span-2 lg:col-span-1">
              <input
                type="checkbox"
                checked={form.protected}
                onChange={e => setForm({ ...form, protected: e.target.checked })}
                className="h-4 w-4 rounded border-border bg-surface"
              />
              {t('environments.protected')}
            </label>
            <div className="flex gap-2 sm:col-span-2 lg:col-span-3">
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
                  {env.protected && <span className="rounded-full bg-amber-500/15 px-2 py-0.5 text-xs text-amber-500">{t('environments.protected')} · {env.required_approvals}</span>}
                  {env.url && <a href={env.url} target="_blank" rel="noreferrer" className="text-xs text-accent hover:underline">{env.url}</a>}
                </div>
                <div className="flex gap-1">
                  <Button size="sm" variant="ghost" className="h-7 gap-1 px-2 text-xs" onClick={() => setSelectedEnv(selectedEnv?.id === env.id ? null : env)}>
                    <Rocket className="h-3 w-3" /> {t('environments.deployments')}
                  </Button>
                  <Button size="sm" variant="ghost" aria-label={`${t('common.delete')} ${env.name}`} title={`${t('common.delete')} ${env.name}`} className="h-7 gap-1 px-2 text-xs text-danger hover:text-danger" onClick={() => {
                    setPendingEnv(env)
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

      <ConfirmDialog
        open={pendingEnv !== null}
        title={pendingEnv ? `${t('environments.deleteConfirm')} "${pendingEnv.name}"?` : ''}
        onCancel={() => setPendingEnv(null)}
        onConfirm={() => {
          if (pendingEnv) deleteEnv.mutate(pendingEnv.id, { onSuccess: () => toast.success(t('environments.deleted')), onError: (err: Error) => toast.error(err.message) })
          setPendingEnv(null)
        }}
      />
    </div>
  )
}

function DeploymentsSection({ environmentId }: { environmentId: string }) {
  const { t } = useTranslation()
  const { data: deployments = [], isLoading } = useDeployments(environmentId)
  const createDep = useCreateDeployment(environmentId)
  const recordApproval = useRecordDeploymentApproval(environmentId)
  const rollbackDeployment = useRollbackDeployment(environmentId)
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
              <TableHead>{t('deployments.approval')}</TableHead>
              <TableHead>{t('deployments.trace')}</TableHead>
              <TableHead>{t('deployments.created')}</TableHead>
              <TableHead className="text-right">{t('common.actions')}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {deployments.map(d => (
              <TableRow key={d.id}>
                <TableCell className="font-mono text-sm">{d.git_ref}</TableCell>
                <TableCell className="text-xs">{d.status}</TableCell>
                <TableCell className="text-xs">{approvalLabel(d, t)}</TableCell>
                <TableCell className="font-mono text-xs text-text-muted">{traceLabel(d, t)}</TableCell>
                <TableCell className="text-xs text-text-muted">{new Date(d.created_at).toLocaleString()}</TableCell>
                <TableCell>
                  <div className="flex justify-end gap-1">
                    {d.approval_required && d.approval_state === 'pending' && (
                      <>
                        <Button
                          type="button"
                          size="sm"
                          variant="ghost"
                          className="h-7 gap-1 px-2 text-xs"
                          disabled={recordApproval.isPending}
                          onClick={() => recordApproval.mutate({ deploymentId: d.id, decision: 'approved' }, {
                            onSuccess: () => toast.success(t('deployments.approvalRecorded')),
                            onError: (err) => toast.error(err.message),
                          })}
                        >
                          <CheckCircle2 className="h-3 w-3" />
                          {t('deployments.approve')}
                        </Button>
                        <Button
                          type="button"
                          size="sm"
                          variant="ghost"
                          className="h-7 gap-1 px-2 text-xs text-danger hover:text-danger"
                          disabled={recordApproval.isPending}
                          onClick={() => recordApproval.mutate({ deploymentId: d.id, decision: 'rejected' }, {
                            onSuccess: () => toast.success(t('deployments.approvalRecorded')),
                            onError: (err) => toast.error(err.message),
                          })}
                        >
                          <XCircle className="h-3 w-3" />
                          {t('deployments.reject')}
                        </Button>
                      </>
                    )}
                    {d.status === 'success' && (
                      <Button
                        type="button"
                        size="sm"
                        variant="ghost"
                        className="h-7 gap-1 px-2 text-xs"
                        disabled={rollbackDeployment.isPending}
                        onClick={() => rollbackDeployment.mutate({ deploymentId: d.id }, {
                          onSuccess: () => toast.success(t('deployments.rollbackCreated')),
                          onError: (err) => toast.error(err.message),
                        })}
                      >
                        <RotateCcw className="h-3 w-3" />
                        {t('deployments.rollback')}
                      </Button>
                    )}
                  </div>
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}
    </div>
  )
}

function approvalLabel(deployment: Deployment, t: (key: string) => string) {
  if (!deployment.approval_required) return t('deployments.approvalNotRequired')
  if (deployment.approval_state === 'approved') return `${t('deployments.approved')} ${deployment.approval_count}/${deployment.required_approvals}`
  if (deployment.approval_state === 'rejected') return t('deployments.rejected')
  return `${t('deployments.approvalPending')} ${deployment.approval_count}/${deployment.required_approvals}`
}

function traceLabel(deployment: Deployment, t: (key: string) => string) {
  if (deployment.rollback_of_id) return `${t('deployments.rollbackOf')} ${deployment.rollback_of_id.slice(0, 8)}`
  return deployment.pipeline_id ? deployment.pipeline_id.slice(0, 8) : '—'
}
