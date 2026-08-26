import { useState } from 'react'
import { useParams, Link } from 'react-router'
import { useTranslation } from 'react-i18next'
import { usePipelines, useTriggerPipeline, useProjects } from '@/api/hooks'
import { Card } from '@/shared/ui/card'
import { Button } from '@/shared/ui/button'
import { Input } from '@/shared/ui/input'
import { Label } from '@/shared/ui/label'
import { Play, ChevronRight, Clock } from 'lucide-react'
import { toast } from 'sonner'
import type { Status } from '@/api/types'

const statusColors: Record<Status, string> = {
  queued: 'text-text-muted',
  running: 'text-warning',
  success: 'text-success',
  failed: 'text-danger',
  canceled: 'text-text-muted',
}

export function PipelinesPage() {
  const { t } = useTranslation()
  const { projectId } = useParams<{ projectId: string }>()
  const { data: projects = [] } = useProjects()
  const project = projects.find(p => p.id === projectId)
  const { data: pipelines = [], isLoading } = usePipelines(projectId)
  const trigger = useTriggerPipeline(projectId)
  const [showForm, setShowForm] = useState(false)
  const [gitRef, setGitRef] = useState('')

  function openForm() {
    setGitRef(project?.default_branch ?? 'main')
    setShowForm(true)
  }

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    trigger.mutate(gitRef, {
      onSuccess: () => { setShowForm(false); toast.success('Pipeline triggered') },
      onError: err => toast.error(err.message),
    })
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <div className="flex items-center gap-2 text-sm text-text-muted">
            <Link to="/projects" className="hover:text-text-primary">{t('navigation.projects')}</Link>
            <ChevronRight className="h-3 w-3" />
            <span>{project?.name ?? projectId}</span>
          </div>
          <h1 className="mt-1 text-2xl font-bold">{t('pipelines.title')}</h1>
        </div>
        <Button size="sm" onClick={openForm} disabled={trigger.isPending}>
          <Play className="h-4 w-4" />
          {t('pipelines.run')}
        </Button>
      </div>

      {showForm && (
        <Card className="p-4">
          <form onSubmit={handleSubmit} className="flex flex-wrap items-end gap-3">
            <div className="min-w-48 flex-1 space-y-1.5 sm:max-w-xs">
              <Label htmlFor="git-ref">{t('pipelines.gitRef')}</Label>
              <Input
                id="git-ref"
                required
                placeholder="main"
                value={gitRef}
                onChange={e => setGitRef(e.target.value)}
              />
            </div>
            <Button type="submit" disabled={trigger.isPending}>{t('pipelines.run')}</Button>
            <Button type="button" variant="ghost" onClick={() => setShowForm(false)}>{t('common.cancel')}</Button>
          </form>
        </Card>
      )}

      {isLoading ? (
        <p className="text-sm text-text-muted">{t('common.loading')}</p>
      ) : pipelines.length === 0 ? (
        <Card className="p-8 text-center"><p className="text-text-muted">{t('pipelines.empty')}</p></Card>
      ) : (
        <div className="space-y-2">
          {pipelines.map(p => (
            <Link key={p.id} to={`/pipelines/${p.id}`}>
              <Card className="flex cursor-pointer items-center justify-between p-3 transition-colors hover:border-accent">
                <div className="flex items-center gap-3">
                  <code className="rounded bg-surface-raised px-2 py-1 text-xs">#{p.id.slice(0, 8)}</code>
                  <code className="text-xs text-text-secondary">{p.git_ref}</code>
                  <span className={`text-sm font-medium ${statusColors[p.status]}`}>{t(`pipelines.${p.status}`)}</span>
                </div>
                <div className="flex items-center gap-2 text-xs text-text-muted">
                  <Clock className="h-3 w-3" />
                  <time>{new Date(p.created_at).toLocaleString()}</time>
                  <ChevronRight className="h-4 w-4" />
                </div>
              </Card>
            </Link>
          ))}
        </div>
      )}
    </div>
  )
}
