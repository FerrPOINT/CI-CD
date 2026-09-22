import { Link, useParams, useSearchParams } from 'react-router'
import { useTranslation } from 'react-i18next'
import { usePullRequests, useRepositoryComparison } from '@/api/hooks'
import { Button } from '@sdlc/ui/ui'
import { ChevronRight, GitPullRequest, FileDiff, ArrowLeft } from 'lucide-react'
import type { PullRequest, PullRequestStatus } from '@/api/types'
import { formatDate } from '@/shared/lib/format'
import { QueryState } from '@/shared/ui/query-state'
import { PullRequestActions } from '@/pages/pull-requests/actions'
import { UserAvatar } from '@/shared/ui/user-avatar'

export function buildCompareHref(pr: Pick<PullRequest, 'repository_name' | 'source_branch' | 'target_branch'>): string {
  return `/repositories/${encodeURIComponent(pr.repository_name)}/compare?from=${encodeURIComponent(pr.target_branch)}&to=${encodeURIComponent(pr.source_branch)}`
}

export function buildPrDiffHref(pr: Pick<PullRequest, 'repository_name' | 'number'>): string {
  return `/repositories/${encodeURIComponent(pr.repository_name)}/pulls/${pr.number}?view=diff`
}

function PatchView({ patch, truncated }: { patch: string; truncated: boolean }) {
  const { t } = useTranslation()
  return (
    <div className="space-y-2">
      {truncated && <p role="status" className="text-sm text-text-muted">{t('compare.patchTruncated')}</p>}
      <pre tabIndex={0} aria-label={t('compare.patch')} className="max-h-[48rem] overflow-auto rounded-md bg-zinc-950 p-4 text-xs leading-relaxed">
        {patch.split('\n').map((line, index) => {
          const key = `${index}-${line}`
          if (line.startsWith('+++') || line.startsWith('---')) return <span key={key} className="block font-mono text-indigo-300">{line}</span>
          if (line.startsWith('@@')) return <span key={key} className="block font-mono text-sky-400">{line}</span>
          if (line.startsWith('+')) return <span key={key} className="block bg-green-500/10 font-mono text-green-400">{line}</span>
          if (line.startsWith('-')) return <span key={key} className="block bg-red-500/10 font-mono text-red-400">{line}</span>
          if (line.startsWith('diff --git') || line.startsWith('index ')) return <span key={key} className="block font-mono text-zinc-400">{line}</span>
          return <span key={key} className="block font-mono text-zinc-300">{line}</span>
        })}
      </pre>
    </div>
  )
}

const statusStyles: Record<PullRequestStatus, string> = {
  open: 'bg-accent/15 text-accent',
  closed: 'bg-danger/15 text-danger',
  merged: 'bg-success/15 text-success',
}

export function PullRequestDetailPage() {
  const { t, i18n } = useTranslation()
  const { repo, number } = useParams<{ repo: string; number: string }>()
  const [searchParams, setSearchParams] = useSearchParams()
  const { data: pullRequests = [], isLoading, error, refetch } = usePullRequests(repo)

  const pr = pullRequests.find(p => String(p.number) === number)
  const showDiff = searchParams.get('view') === 'diff'
  const { data: comparison, isLoading: diffLoading, isError: diffError, error: diffErrorValue, refetch: refetchDiff } = useRepositoryComparison(
    repo,
    showDiff && pr ? pr.target_branch : '',
    showDiff && pr ? pr.source_branch : '',
  )

  if (!repo) return <p className="text-sm text-text-muted">{t('repositories.notFound')}</p>
  if (isLoading || error) return (
    <QueryState data={pullRequests} isLoading={isLoading} error={error} onRetry={() => refetch()}>
      {() => null}
    </QueryState>
  )
  if (!pr) return <p className="text-sm text-text-muted">{t('pulls.notFound')}</p>

  if (showDiff) {
    return (
      <div className="space-y-6">
        <div>
          <div className="flex flex-wrap items-center gap-2 text-sm text-text-muted">
            <Link to="/repositories" className="inline-flex min-h-10 items-center hover:text-text-primary">{t('navigation.repositories')}</Link>
            <ChevronRight className="h-3 w-3" />
            <Link to={`/repositories/${encodeURIComponent(repo)}/pulls`} className="inline-flex min-h-10 items-center hover:text-text-primary">{t('repositoryBrowser.pullRequests')}</Link>
            <ChevronRight className="h-3 w-3" />
            <Link to={`/repositories/${encodeURIComponent(repo)}/pulls/${pr.number}`} className="inline-flex min-h-10 items-center hover:text-text-primary">#{pr.number}</Link>
            <ChevronRight className="h-3 w-3" />
            <span>{t('pulls.viewDiff')}</span>
          </div>
          <div className="mt-2 flex flex-wrap items-center gap-3">
            <FileDiff className="hidden h-6 w-6 text-accent sm:block" aria-hidden />
            <h1 className="min-w-0 break-words text-2xl font-bold">{t('pulls.viewDiff')} <span className="whitespace-nowrap text-text-muted">#{pr.number}</span></h1>
          </div>
          <p className="mt-2 break-all font-mono text-sm text-text-secondary">{pr.target_branch} → {pr.source_branch}</p>
          <p className="mt-2 text-sm text-text-muted">{pr.title}</p>
        </div>
        <Button variant="outline" className="h-10" onClick={() => setSearchParams({}, { replace: true })}>
          <ArrowLeft className="h-4 w-4" /> {t('common.back')}
        </Button>
        {diffLoading ? (
          <p className="text-sm text-text-muted">{t('common.loading')}</p>
        ) : diffError ? (
          <div role="alert" className="flex flex-wrap items-center gap-3 border-y border-border py-4 text-sm text-danger">
            <span>{t('common.error')}: {diffErrorValue instanceof Error ? diffErrorValue.message : String(diffErrorValue)}</span>
            <Button type="button" variant="outline" className="h-10" onClick={() => refetchDiff()}>{t('common.retry')}</Button>
          </div>
        ) : !comparison || comparison.files.length === 0 ? (
          <p className="border-y border-border py-6 text-center text-sm text-text-muted">{t('compare.noChanges')}</p>
        ) : (
          <>
            <div className="flex flex-wrap items-center gap-x-3 gap-y-1 border-y border-border py-3 text-sm">
              <span className="text-text-muted">{t('compare.mergeBase')}: </span>
              <code className="break-all rounded bg-surface-raised px-1.5 py-0.5">{comparison.merge_base}</code>
              {comparison.files.some((file) => !file.binary) ? (
                <>
                  <span className="text-success">+{comparison.files.reduce((sum, file) => sum + file.additions, 0)}</span>
                  <span className="text-danger">−{comparison.files.reduce((sum, file) => sum + file.deletions, 0)}</span>
                </>
              ) : null}
            </div>
            <div>
              <h2 className="text-sm font-semibold uppercase tracking-wide">{t('compare.filesChanged')}</h2>
              <ul className="mt-3 divide-y divide-border overflow-hidden rounded-md border border-border">
                {comparison.files.map((file) => (
                  <li key={file.path} className="flex items-center gap-3 px-4 py-3 text-sm">
                    <FileDiff className="h-4 w-4 text-text-muted" aria-hidden />
                    <code className="flex-1 truncate">{file.path}</code>
                    {file.binary ? (
                      <span className="text-text-muted">{t('compare.binaryFile')}</span>
                    ) : (
                      <>
                        <span className="text-success">+{file.additions}</span>
                        <span className="text-danger">−{file.deletions}</span>
                      </>
                    )}
                  </li>
                ))}
              </ul>
            </div>
            {comparison.patch.trim() && <><h2 className="text-sm font-semibold uppercase tracking-wide">{t('compare.patch')}</h2><PatchView patch={comparison.patch} truncated={comparison.patch_truncated} /></>}
          </>
        )}
      </div>
    )
  }

  return (
    <div className="space-y-6">
      <div>
        <div className="flex flex-wrap items-center gap-2 text-sm text-text-muted">
          <Link to="/repositories" className="inline-flex min-h-10 items-center hover:text-text-primary">{t('navigation.repositories')}</Link>
          <ChevronRight className="h-3 w-3" />
          <Link to={`/repositories/${encodeURIComponent(repo)}`} className="inline-flex min-h-10 items-center break-all hover:text-text-primary">{repo}</Link>
          <ChevronRight className="h-3 w-3" />
          <Link to={`/repositories/${encodeURIComponent(repo)}/pulls`} className="inline-flex min-h-10 items-center hover:text-text-primary">{t('repositoryBrowser.pullRequests')}</Link>
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
        <section className="min-w-0 flex-1 border-y border-border py-4">
          {pr.description && <p className="whitespace-pre-wrap text-sm text-text-secondary">{pr.description}</p>}
          <dl className="mt-4 grid grid-cols-[auto_1fr] items-center gap-x-4 gap-y-2 text-sm">
            <dt className="text-text-muted">{t('pulls.sourceBranch')}</dt>
            <dd><code className="break-all rounded bg-surface-raised px-1.5 py-0.5">{pr.source_branch}</code></dd>
            <dt className="text-text-muted">{t('pulls.targetBranch')}</dt>
            <dd><code className="break-all rounded bg-surface-raised px-1.5 py-0.5">{pr.target_branch}</code></dd>
            <dt className="text-text-muted">{t('pulls.createdBy')}</dt>
            <dd className="break-all">
              <UserAvatar name={pr.created_by} size="sm" withName />
            </dd>
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
        </section>

        <aside className="h-fit lg:w-60">
          <h2 className="text-sm font-semibold uppercase tracking-wide">{t('pulls.actions')}</h2>
          <div className="mt-3 flex flex-col gap-2">
            <Button asChild variant="outline" className="h-10 justify-start">
              <Link to={buildPrDiffHref(pr)}>
                <FileDiff className="h-4 w-4" /> {t('pulls.viewDiff')}
              </Link>
            </Button>
            <PullRequestActions repo={repo} pullRequest={pr} />
          </div>
        </aside>
      </div>
    </div>
  )
}

