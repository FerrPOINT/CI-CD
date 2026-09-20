import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useParams } from 'react-router'
import { AlertCircle, ChevronDown, Globe, Plus, RotateCcw, Search, Trash2 } from 'lucide-react'
import { toast } from 'sonner'
import { useCreateDeployment, useCreateEnvironment, useDeleteEnvironment, useDeployments, useEnvironments, useRecordDeploymentApproval, useRollbackDeployment } from '@/api/hooks'
import type { Deployment, Environment, EnvironmentStatus } from '@/api/types'
import { Button, Input, Label } from '@sdlc/ui/ui'
import { CapabilityCallout } from '@/shared/ui/capability-callout'
import { ConfirmDialog } from '@/shared/ui/confirm-dialog'
import { QueryState } from '@/shared/ui/query-state'

const pageSize = 20
type StatusFilter = 'all' | EnvironmentStatus
type DeploymentAction = { deployment: Deployment; kind: 'approved' | 'rejected' | 'rollback' }

export function EnvironmentsPage() {
  const { t } = useTranslation()
  const { projectId } = useParams()
  const { data: environments = [], isLoading, error, refetch } = useEnvironments(projectId)
  const createEnv = useCreateEnvironment(projectId)
  const deleteEnv = useDeleteEnvironment()
  const [showForm, setShowForm] = useState(false)
  const [form, setForm] = useState({ name: '', url: '', protected: false, required_approvals: 1 })
  const [formError, setFormError] = useState<string | null>(null)
  const [pendingEnv, setPendingEnv] = useState<Environment | null>(null)
  const [deleteError, setDeleteError] = useState<string | null>(null)
  const [selectedEnvId, setSelectedEnvId] = useState<string | null>(null)
  const [search, setSearch] = useState('')
  const [status, setStatus] = useState<StatusFilter>('all')
  const [page, setPage] = useState(1)

  const query = search.trim().toLocaleLowerCase()
  const filtered = environments.filter(env =>
    (status === 'all' || env.status === status) &&
    (env.name.toLocaleLowerCase().includes(query) || env.url?.toLocaleLowerCase().includes(query)),
  )
  const totalPages = Math.max(1, Math.ceil(filtered.length / pageSize))
  const currentPage = Math.min(page, totalPages)
  const visible = filtered.slice((currentPage - 1) * pageSize, currentPage * pageSize)

  function createEnvironment(event: React.FormEvent) {
    event.preventDefault()
    setFormError(null)
    createEnv.mutate({
      name: form.name.trim(),
      url: form.url.trim() || undefined,
      protected: form.protected,
      required_approvals: form.protected ? form.required_approvals : 0,
    }, {
      onSuccess: () => {
        setShowForm(false)
        setForm({ name: '', url: '', protected: false, required_approvals: 1 })
        toast.success(t('environments.created'))
      },
      onError: () => setFormError(t('environments.createFailed')),
    })
  }

  function deleteEnvironment() {
    if (!pendingEnv) return
    setDeleteError(null)
    deleteEnv.mutate(pendingEnv.id, {
      onSuccess: () => {
        if (selectedEnvId === pendingEnv.id) setSelectedEnvId(null)
        setPendingEnv(null)
        toast.success(t('environments.deleted'))
      },
      onError: () => setDeleteError(t('environments.deleteFailed')),
    })
  }

  return (
    <div className="min-w-0 max-w-6xl space-y-5">
      <header className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex items-center gap-2">
          <Globe className="h-5 w-5 text-accent" aria-hidden />
          <h1 className="text-xl font-bold sm:text-2xl">{t('environments.title')}</h1>
        </div>
        <Button type="button" size="sm" className="min-h-10 sm:min-h-10" aria-expanded={showForm} onClick={() => { setShowForm(!showForm); setFormError(null) }}>
          <Plus className="h-4 w-4" aria-hidden />{t('environments.create')}
        </Button>
      </header>

      <CapabilityCallout tone="mvp" title={t('environments.capabilityTitle')} label={t('capability.currentMvp')} description={t('environments.capabilityDescription')} />

      {showForm && (
        <form onSubmit={createEnvironment} className="grid gap-3 border-y border-border py-4 sm:grid-cols-2">
          <div className="space-y-1.5">
            <Label htmlFor="env-name">{t('environments.name')}</Label>
            <Input id="env-name" required value={form.name} onChange={event => setForm({ ...form, name: event.target.value })} />
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="env-url">{t('environments.url')}</Label>
            <Input id="env-url" type="url" placeholder="https://app.example.com" value={form.url} onChange={event => setForm({ ...form, url: event.target.value })} />
          </div>
          <label className="flex min-h-10 items-center gap-2 text-sm text-text-secondary">
            <input type="checkbox" checked={form.protected} onChange={event => setForm({ ...form, protected: event.target.checked })} className="h-4 w-4 accent-accent" />
            {t('environments.protected')}
          </label>
          {form.protected && (
            <div className="space-y-1.5">
              <Label htmlFor="env-required-approvals">{t('environments.requiredApprovals')}</Label>
              <Input id="env-required-approvals" type="number" min={1} max={10} required value={form.required_approvals} onChange={event => setForm({ ...form, required_approvals: Number(event.target.value) || 1 })} />
            </div>
          )}
          {formError && <InlineError message={formError} />}
          <div className="flex flex-wrap gap-2 sm:col-span-2">
            <Button type="submit" disabled={createEnv.isPending}>{t('environments.create')}</Button>
            <Button type="button" variant="outline" onClick={() => { setShowForm(false); setFormError(null) }}>{t('common.cancel')}</Button>
          </div>
        </form>
      )}

      <QueryState data={environments} isLoading={isLoading} error={error} errorMessage={t('environments.loadFailed')} isEmpty={list => list.length === 0} empty={{ title: t('environments.empty') }} onRetry={() => void refetch()}>
        {() => (
          <section aria-label={t('environments.title')} className="space-y-3">
            <div className="flex flex-wrap gap-2">
              <div className="relative min-w-0 flex-1 sm:max-w-xl">
                <Search className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-text-muted" aria-hidden />
                <Input type="search" aria-label={t('environments.search')} placeholder={t('environments.search')} className="min-h-10 pl-9" value={search} onChange={event => { setSearch(event.target.value); setPage(1) }} />
              </div>
              <select aria-label={t('environments.statusFilter')} className="min-h-10 rounded-md border border-border-strong bg-surface px-3 text-sm text-text-primary" value={status} onChange={event => { setStatus(event.target.value as StatusFilter); setPage(1) }}>
                <option value="all">{t('environments.all')}</option>
                <option value="available">{t('environments.statusAvailable')}</option>
                <option value="stopped">{t('environments.statusStopped')}</option>
                <option value="degraded">{t('environments.statusDegraded')}</option>
              </select>
            </div>
            <p className="text-xs text-text-muted">{t('environments.shown', { count: visible.length, total: filtered.length })}</p>
            {filtered.length === 0 ? <p role="status" className="border-y border-border py-6 text-sm text-text-muted">{t('environments.noMatches')}</p> : (
              <ul className="divide-y divide-border border-y border-border">
                {visible.map(env => {
                  const expanded = selectedEnvId === env.id
                  const safeUrl = externalUrl(env.url)
                  return (
                    <li key={env.id} className="min-w-0 py-2">
                      <div className="flex min-w-0 items-start gap-2">
                        <Globe className="mt-1 h-4 w-4 shrink-0 text-accent" aria-hidden />
                        <div className="min-w-0 flex-1">
                          <p className="break-all text-sm font-medium">{env.name}</p>
                          <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-text-secondary">
                            <span className="inline-flex items-center gap-1"><span aria-hidden className={`h-2 w-2 rounded-full ${env.status === 'available' ? 'bg-success' : env.status === 'degraded' ? 'bg-warning' : 'bg-text-muted'}`} />{t('environments.status' + capitalize(env.status))}</span>
                            {env.protected && <span>{t('environments.protected')} · {env.required_approvals}</span>}
                          </div>
                          {env.url && (safeUrl ? <a href={safeUrl} target="_blank" rel="noopener noreferrer" className="block w-fit max-w-full truncate text-xs text-accent underline-offset-2 hover:underline" title={env.url}>{env.url}</a> : <span className="block max-w-full truncate text-xs text-text-muted" title={env.url}>{env.url}</span>)}
                        </div>
                        <button type="button" aria-label={t('environments.deploymentsFor', { name: env.name })} title={t('environments.deploymentsFor', { name: env.name })} aria-expanded={expanded} aria-controls={`deployments-${env.id}`} className="inline-flex h-10 w-10 shrink-0 items-center justify-center rounded-md text-accent hover:bg-surface-raised focus-visible:outline-2 focus-visible:outline-accent" onClick={() => setSelectedEnvId(expanded ? null : env.id)}>
                          <ChevronDown className={`h-4 w-4 transition-transform ${expanded ? 'rotate-180' : ''}`} aria-hidden />
                        </button>
                        <button type="button" aria-label={t('environments.deleteFor', { name: env.name })} title={t('environments.deleteFor', { name: env.name })} className="inline-flex h-10 w-10 shrink-0 items-center justify-center rounded-md text-danger hover:bg-surface-raised focus-visible:outline-2 focus-visible:outline-accent" onClick={() => { setPendingEnv(env); setDeleteError(null) }}>
                          <Trash2 className="h-4 w-4" aria-hidden />
                        </button>
                      </div>
                      {expanded && <div id={`deployments-${env.id}`} className="ml-6 mt-3 border-t border-border pt-3"><DeploymentsSection environmentId={env.id} /></div>}
                    </li>
                  )
                })}
              </ul>
            )}
            {filtered.length > pageSize && <Pagination label={t('environments.pages')} current={currentPage} total={totalPages} previous={t('environments.previous')} next={t('environments.next')} onPage={setPage} />}
          </section>
        )}
      </QueryState>

      <ConfirmDialog open={pendingEnv !== null} title={pendingEnv ? t('environments.deleteConfirmFor', { name: pendingEnv.name }) : ''} description={t('environments.deleteWarning')} error={deleteError ?? undefined} closeOnConfirm={false} pending={deleteEnv.isPending} onCancel={() => { setPendingEnv(null); setDeleteError(null) }} onConfirm={deleteEnvironment} />
    </div>
  )
}

function DeploymentsSection({ environmentId }: { environmentId: string }) {
  const { t } = useTranslation()
  const { data: deployments = [], isLoading, error, refetch } = useDeployments(environmentId)
  const createDep = useCreateDeployment(environmentId)
  const recordApproval = useRecordDeploymentApproval(environmentId)
  const rollbackDeployment = useRollbackDeployment(environmentId)
  const [gitRef, setGitRef] = useState('')
  const [formError, setFormError] = useState<string | null>(null)
  const [action, setAction] = useState<DeploymentAction | null>(null)
  const [actionError, setActionError] = useState<string | null>(null)
  const [search, setSearch] = useState('')
  const [page, setPage] = useState(1)
  const filtered = deployments.filter(deployment => deployment.git_ref.toLocaleLowerCase().includes(search.trim().toLocaleLowerCase()))
  const totalPages = Math.max(1, Math.ceil(filtered.length / pageSize))
  const currentPage = Math.min(page, totalPages)
  const visible = filtered.slice((currentPage - 1) * pageSize, currentPage * pageSize)
  const actionPending = recordApproval.isPending || rollbackDeployment.isPending

  function createDeployment(event: React.FormEvent) {
    event.preventDefault()
    setFormError(null)
    createDep.mutate({ git_ref: gitRef.trim() }, {
      onSuccess: () => { setGitRef(''); toast.success(t('deployments.created')) },
      onError: () => setFormError(t('deployments.createFailed')),
    })
  }

  function confirmAction() {
    if (!action) return
    setActionError(null)
    const onSuccess = () => {
      toast.success(t(action.kind === 'rollback' ? 'deployments.rollbackCreated' : 'deployments.approvalRecorded'))
      setAction(null)
    }
    const onError = () => setActionError(t('deployments.actionFailed'))
    if (action.kind === 'rollback') rollbackDeployment.mutate({ deploymentId: action.deployment.id }, { onSuccess, onError })
    else recordApproval.mutate({ deploymentId: action.deployment.id, decision: action.kind }, { onSuccess, onError })
  }

  return (
    <section aria-label={t('environments.deployments')} className="min-w-0 space-y-3">
      <h2 className="text-sm font-semibold">{t('environments.deployments')}</h2>
      <form onSubmit={createDeployment} className="flex flex-wrap items-end gap-2">
        <div className="min-w-0 flex-1 space-y-1 sm:max-w-xs">
          <Label htmlFor={`git-ref-${environmentId}`}>{t('deployments.gitRef')}</Label>
          <Input id={`git-ref-${environmentId}`} required placeholder="main" className="min-h-10" value={gitRef} onChange={event => setGitRef(event.target.value)} />
        </div>
        <Button type="submit" size="sm" className="min-h-10 sm:min-h-10" disabled={createDep.isPending}>{t('deployments.create')}</Button>
      </form>
      {formError && <InlineError message={formError} />}
      <QueryState data={deployments} isLoading={isLoading} error={error} errorMessage={t('deployments.loadFailed')} isEmpty={list => list.length === 0} empty={{ title: t('deployments.empty') }} onRetry={() => void refetch()}>
        {() => (
          <div className="space-y-2">
            {deployments.length > pageSize && <Input type="search" aria-label={t('deployments.search')} placeholder={t('deployments.search')} className="min-h-10 sm:max-w-xs" value={search} onChange={event => { setSearch(event.target.value); setPage(1) }} />}
            {filtered.length === 0 ? <p role="status" className="py-3 text-sm text-text-muted">{t('deployments.noMatches')}</p> : (
              <ul className="divide-y divide-border border-y border-border">
                {visible.map(deployment => (
                  <li key={deployment.id} className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1 py-2 text-xs">
                    <div className="min-w-0 flex-1 basis-40">
                      <p className="break-all font-mono text-sm font-medium text-text-primary">{deployment.git_ref}</p>
                      <p className="text-text-secondary">{t('deployments.status' + capitalize(deployment.status))} · {approvalLabel(deployment, t)}</p>
                      <p className="text-text-muted"><time dateTime={deployment.created_at} title={new Date(deployment.created_at).toLocaleString()}>{formatDate(deployment.created_at)}</time> · <span title={deployment.rollback_of_id ?? deployment.pipeline_id ?? undefined}>{traceLabel(deployment, t)}</span></p>
                    </div>
                    <div className="flex flex-wrap gap-1">
                      {deployment.approval_required && deployment.approval_state === 'pending' && (
                        <>
                          <Button type="button" size="sm" variant="outline" aria-label={t('deployments.approveFor', { ref: deployment.git_ref })} className="min-h-10 sm:min-h-10" onClick={() => { setAction({ deployment, kind: 'approved' }); setActionError(null) }}>{t('deployments.approve')}</Button>
                          <Button type="button" size="sm" variant="outline" aria-label={t('deployments.rejectFor', { ref: deployment.git_ref })} className="min-h-10 sm:min-h-10" onClick={() => { setAction({ deployment, kind: 'rejected' }); setActionError(null) }}>{t('deployments.reject')}</Button>
                        </>
                      )}
                      {deployment.status === 'success' && <Button type="button" size="sm" variant="outline" aria-label={t('deployments.rollbackFor', { ref: deployment.git_ref })} className="min-h-10 sm:min-h-10" onClick={() => { setAction({ deployment, kind: 'rollback' }); setActionError(null) }}><RotateCcw className="h-4 w-4" aria-hidden />{t('deployments.rollback')}</Button>}
                    </div>
                  </li>
                ))}
              </ul>
            )}
            {filtered.length > pageSize && <Pagination label={t('deployments.pages')} current={currentPage} total={totalPages} previous={t('deployments.previous')} next={t('deployments.next')} onPage={setPage} />}
          </div>
        )}
      </QueryState>
      <ConfirmDialog open={action !== null} title={action ? t('deployments.confirm' + capitalize(action.kind), { ref: action.deployment.git_ref }) : ''} description={t('deployments.actionWarning')} error={actionError ?? undefined} confirmLabel={action ? t(action.kind === 'rollback' ? 'deployments.rollback' : 'deployments.' + (action.kind === 'approved' ? 'approve' : 'reject')) : ''} closeOnConfirm={false} pending={actionPending} onCancel={() => { setAction(null); setActionError(null) }} onConfirm={confirmAction} />
    </section>
  )
}

function InlineError({ message }: { message: string }) {
  return <p role="alert" className="flex items-center gap-2 text-sm text-text-primary"><AlertCircle className="h-4 w-4 shrink-0 text-danger" aria-hidden />{message}</p>
}

function Pagination({ label, current, total, previous, next, onPage }: { label: string; current: number; total: number; previous: string; next: string; onPage: (page: number) => void }) {
  return <nav aria-label={label} className="flex items-center justify-end gap-2">
    <Button type="button" size="sm" variant="outline" className="min-h-10 sm:min-h-10" disabled={current === 1} onClick={() => onPage(current - 1)}>{previous}</Button>
    <span className="text-sm text-text-muted">{current} / {total}</span>
    <Button type="button" size="sm" variant="outline" className="min-h-10 sm:min-h-10" disabled={current === total} onClick={() => onPage(current + 1)}>{next}</Button>
  </nav>
}

function externalUrl(value: string | null): string | null {
  if (!value) return null
  try {
    const url = new URL(value)
    return url.protocol === 'https:' || url.protocol === 'http:' ? url.href : null
  } catch { return null }
}

function capitalize(value: string): string { return value[0].toUpperCase() + value.slice(1) }

function formatDate(value: string): string { return new Date(value).toLocaleString(undefined, { dateStyle: 'short', timeStyle: 'short' }) }

function approvalLabel(deployment: Deployment, t: (key: string) => string): string {
  if (!deployment.approval_required) return t('deployments.approvalNotRequired')
  if (deployment.approval_state === 'approved') return `${t('deployments.approved')} ${deployment.approval_count}/${deployment.required_approvals}`
  if (deployment.approval_state === 'rejected') return t('deployments.rejected')
  return `${t('deployments.approvalPending')} ${deployment.approval_count}/${deployment.required_approvals}`
}

function traceLabel(deployment: Deployment, t: (key: string) => string): string {
  if (deployment.rollback_of_id) return `${t('deployments.rollbackOf')} ${deployment.rollback_of_id.slice(0, 8)}`
  return deployment.pipeline_id ? `${t('deployments.pipeline')} ${deployment.pipeline_id.slice(0, 8)}` : '—'
}
