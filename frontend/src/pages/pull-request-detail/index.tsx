import { Link, useParams } from 'react-router'
import { useTranslation } from 'react-i18next'
import { usePullRequests, usePullRequestAction } from '@/api/hooks'
import { Card } from '@/shared/ui/card'
import { Button } from '@/shared/ui/button'
import { ChevronRight, GitPullRequest, GitMerge, RotateCcw, XCircle, FileDiff } from 'lucide-react'
import { toast } from 'sonner'
import type { PullRequest, PullRequestStatus } from '@/api/types'
import { formatDate } from '@/shared/lib/format'

export function buildCompareHref(pr: Pick<PullRequest, 'repository_name' | 'source_branch' | 'target_branch'>): string {
  return `/repositories/${encodeURIComponent(pr.repository_name)}/compare?from=${encodeURIComponent(pr.target_branch)}&to=${encodeURIComponent(pr.source_branch)}`
}

const statusStyles: Record<PullRequestStatus, string> = {
  open: 'bg-accent/15 text-accent',
  closed: 'bg-danger/15 text-danger',
  merged: 'bg-success/15 text-success',
}

export function PullRequestDetailPage() {
  const { t, i18n } = useTranslation()
  const { repo, number } = useParams<{ repo: string; number: string }>()
  const { data: pullRequests = [], isLoading } = usePullRequests(repo)
  const prAction = usePullRequestAction(repo)

  const pr = pullRequests.find(p => String(p.number) === number)

  if (!repo) return <p className="text-sm text-text-muted">{t('repositories.notFound')}</p>
  if (isLoading) return <p className="text-sm text-text-muted">{t('common.loading')}</p>
  if (!pr) return <p className="text-sm text-text-muted">{t('pulls.notFound')}</p>

  function handleAction(action: 'merge' | 'close' | 'reopen') {
    prAction.mutate(
      { number: pr!.number, action },
      { onSuccess: () => toast.success(t(`pulls.action_${action}`)), onError: (err) => toast.error(err.message) },
    )
  }

  return (
    <div className="space-y-6">
      <div>
        <div className="flex flex-wrap items-center gap-2 text-sm text-text-muted">
          <Link to="/repositories" className="hover:text-text-primary">{t('navigation.repositories')}</Link>
          <ChevronRight className="h-3 w-3" />
          <Link to={`/repositories/${encodeURIComponent(repo)}`} className="hover:text-text-primary">{repo}</Link>
          <ChevronRight className="h-3 w-3" />
          <Link to={`/repositories/${encodeURIComponent(repo)}/pulls`} className="hover:text-text-primary">{t('repositoryBrowser.pullRequests')}</Link>
          <ChevronRight className="h-3 w-3" />
          <span>#{pr.number}</span>
        </div>
        <div className="mt-2 flex flex-wrap items-center gap-3">
          <GitPullRequest className="h-6 w-6 shrink-0 text-accent" />
          <h1 className="min-w-0 flex-1 break-words text-2xl font-bold">
            <span className="text-text-muted">#{pr.number}</span> {pr.title}
          </h1>
          <span className={`shrink-0 rounded px-2 py-0.5 text-xs font-medium ${statusStyles[pr.status]}`}>
            {t(`pulls.status_${pr.status}`)}
          </span>
        </div>
      </div>

      <div className="flex flex-col gap-4 lg:flex-row">
        <Card className="min-w-0 flex-1 p-4">
          {pr.description && <p className="whitespace-pre-wrap text-sm text-text-secondary">{pr.description}</p>}
          <dl className="mt-4 grid grid-cols-[auto_1fr] items-center gap-x-4 gap-y-2 text-sm">
            <dt className="text-text-muted">{t('pulls.sourceBranch')}</dt>
            <dd><code className="break-all rounded bg-surface-raised px-1.5 py-0.5">{pr.source_branch}</code></dd>
            <dt className="text-text-muted">{t('pulls.targetBranch')}</dt>
            <dd><code className="break-all rounded bg-surface-raised px-1.5 py-0.5">{pr.target_branch}</code></dd>
            <dt className="text-text-muted">{t('pulls.createdBy')}</dt>
            <dd className="break-all">{pr.created_by}</dd>
            <dt className="text-text-muted">{t('pulls.createdAt')}</dt>
            <dd>{formatDate(pr.created_at, i18n.language)}</dd>
            {pr.merged_at && (
              <>
                <dt className="text-text-muted">{t('pulls.mergedAt')}</dt>
                <dd>{formatDate(pr.merged_at, i18n.language)}</dd>
              </>
            )}
            {pr.merge_commit_sha && (
              <>
                <dt className="text-text-muted">{t('pulls.mergeCommit')}</dt>
                <dd><code className="rounded bg-surface-raised px-1.5 py-0.5">{pr.merge_commit_sha.slice(0, 7)}</code></dd>
              </>
            )}
          </dl>
        </Card>

        <Card className="h-fit p-4 lg:w-72">
          <h2 className="text-sm font-semibold uppercase tracking-wide">{t('pulls.actions')}</h2>
          <div className="mt-3 flex flex-col gap-2">
            <Button asChild variant="outline" className="min-h-9 justify-start">
              <Link to={buildCompareHref(pr)}>
                <FileDiff className="h-4 w-4" /> {t('pulls.viewDiff')}
              </Link>
            </Button>
            {pr.status === 'open' && (
              <>
                <Button className="min-h-9 justify-start" disabled={prAction.isPending} onClick={() => handleAction('merge')}>
                  <GitMerge className="h-4 w-4" /> {t('pulls.merge')}
                </Button>
                <Button variant="outline" className="min-h-9 justify-start" disabled={prAction.isPending} onClick={() => handleAction('close')}>
                  <XCircle className="h-4 w-4" /> {t('pulls.close')}
                </Button>
              </>
            )}
            {pr.status === 'closed' && (
              <Button variant="outline" className="min-h-9 justify-start" disabled={prAction.isPending} onClick={() => handleAction('reopen')}>
                <RotateCcw className="h-4 w-4" /> {t('pulls.reopen')}
              </Button>
            )}
          </div>
          <p className="mt-4 text-xs text-text-muted">{t('pulls.pipelineStatusHint')}</p>
        </Card>
      </div>
    </div>
  )
}

