import { FormEvent, useEffect, useState } from 'react'
import { Link, useNavigate, useParams } from 'react-router'
import { useTranslation } from 'react-i18next'
import { ChevronRight, Clock, Play } from 'lucide-react'
import { toast } from 'sonner'
import { usePipelines, useProjects, useTriggerPipeline } from '@/api/hooks'
import { QueryState } from '@/shared/ui/query-state'
import { Button, Input, Label } from '@sdlc/ui/ui'

const pageSize = 20
const statusColors: Record<string, string> = {
  queued: 'text-text-muted',
  running: 'text-warning',
  success: 'text-success',
  failed: 'text-danger',
  canceled: 'text-text-muted',
}

export function PipelinesPage() {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const { projectId } = useParams<{ projectId: string }>()
  const { data: projects = [] } = useProjects()
  const project = projects.find((item) => item.id === projectId)
  const [page, setPage] = useState(0)
  useEffect(() => setPage(0), [projectId])
  const pipelinesQuery = usePipelines(projectId, page, pageSize)
  const pipelines = (pipelinesQuery.data ?? []).slice(0, pageSize)
  const hasNextPage = (pipelinesQuery.data?.length ?? 0) > pageSize
  const trigger = useTriggerPipeline(projectId)
  const [showForm, setShowForm] = useState(false)
  const [gitRef, setGitRef] = useState('')

  function openForm() {
    setGitRef(project?.default_branch ?? 'main')
    setShowForm(true)
  }

  function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!gitRef.trim() || trigger.isPending) return
    trigger.mutate(gitRef.trim(), {
      onSuccess: (detail) => {
        setShowForm(false)
        toast.success(t('pipelines.started'))
        navigate(`/pipelines/${detail.pipeline.id}`)
      },
      onError: (error) => toast.error(error.message),
    })
  }

  return (
    <div className="space-y-5">
      <header className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0">
          <nav
            aria-label={t('pipelines.title')}
            className="flex min-w-0 items-center gap-2 text-sm text-text-muted"
          >
            <Link to="/projects" className="min-h-10 content-center hover:text-text-primary">
              {t('navigation.projects')}
            </Link>
            <ChevronRight className="h-4 w-4 shrink-0" aria-hidden />
            <span className="truncate">{project?.name ?? projectId}</span>
          </nav>
          <h1 className="text-2xl font-bold">{t('pipelines.title')}</h1>
        </div>
        <Button
          type="button"
          size="sm"
          className="min-h-10 sm:min-h-10"
          aria-expanded={showForm}
          aria-controls="pipeline-trigger-form"
          onClick={openForm}
          disabled={trigger.isPending}
        >
          <Play className="h-4 w-4" aria-hidden />
          {t('pipelines.run')}
        </Button>
      </header>

      {showForm && (
        <form
          id="pipeline-trigger-form"
          aria-label={t('pipelines.run')}
          onSubmit={handleSubmit}
          className="flex flex-wrap items-end gap-3 border-y border-border py-3"
        >
          <div className="min-w-48 flex-1 space-y-1.5 sm:max-w-xs">
            <Label htmlFor="git-ref">{t('pipelines.gitRef')}</Label>
            <Input
              id="git-ref"
              className="min-h-10"
              required
              placeholder="main"
              value={gitRef}
              onChange={(event) => setGitRef(event.target.value)}
              disabled={trigger.isPending}
            />
          </div>
          <Button type="submit" className="min-h-10" disabled={trigger.isPending || !gitRef.trim()}>
            {t('pipelines.run')}
          </Button>
          <Button
            type="button"
            variant="outline"
            className="min-h-10"
            disabled={trigger.isPending}
            onClick={() => setShowForm(false)}
          >
            {t('common.cancel')}
          </Button>
        </form>
      )}

      <QueryState
        data={pipelines}
        isLoading={pipelinesQuery.isLoading}
        error={pipelinesQuery.error}
        onRetry={() => void pipelinesQuery.refetch()}
        isEmpty={(items) => items.length === 0}
        empty={{ title: t('pipelines.empty') }}
      >
        {() => (
          <section
            aria-label={t('pipelines.title')}
            className="divide-y divide-border border-y border-border"
          >
            {pipelines.map((pipeline) => (
              <Link
                key={pipeline.id}
                to={`/pipelines/${pipeline.id}`}
                className="flex min-h-14 min-w-0 flex-wrap items-center justify-between gap-x-4 gap-y-1 px-2 py-2 hover:bg-surface-raised focus-visible:outline-2 focus-visible:outline-accent"
              >
                <span className="flex min-w-0 flex-wrap items-center gap-2">
                  <code className="text-sm font-medium text-text-primary">
                    #{pipeline.id.slice(0, 8)}
                  </code>
                  <code className="min-w-0 break-all text-xs text-text-secondary">
                    {pipeline.git_ref}
                  </code>
                  <span className={`text-sm font-medium ${statusColors[pipeline.status]}`}>
                    {t(`pipelines.${pipeline.status}`)}
                  </span>
                </span>
                <span className="flex items-center gap-2 text-xs text-text-muted">
                  <Clock className="h-4 w-4" aria-hidden />
                  <time dateTime={pipeline.created_at}>
                    {new Date(pipeline.created_at).toLocaleString()}
                  </time>
                  <ChevronRight className="h-4 w-4" aria-hidden />
                </span>
              </Link>
            ))}
          </section>
        )}
      </QueryState>
      {(page > 0 || hasNextPage) && !pipelinesQuery.isError && (
        <nav aria-label={t('pipelines.title')} className="flex items-center justify-end gap-2">
          <Button
            type="button"
            size="sm"
            variant="outline"
            className="min-h-10 sm:min-h-10"
            disabled={page === 0 || pipelinesQuery.isFetching}
            onClick={() => setPage(page - 1)}
          >
            {t('projects.previous')}
          </Button>
          <span className="text-sm text-text-muted">{t('pipelines.page', { page: page + 1 })}</span>
          <Button
            type="button"
            size="sm"
            variant="outline"
            className="min-h-10 sm:min-h-10"
            disabled={!hasNextPage || pipelinesQuery.isFetching}
            onClick={() => setPage(page + 1)}
          >
            {t('projects.next')}
          </Button>
        </nav>
      )}
    </div>
  )
}
