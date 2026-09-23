import { useEffect, useState } from 'react'
import { Link, useParams, useSearchParams } from 'react-router'
import { useTranslation } from 'react-i18next'
import { ChevronRight, FileDiff, GitPullRequest, Plus, Search } from 'lucide-react'
import { toast } from 'sonner'
import { useCreatePullRequest, usePullRequests, useRepositoryRefs } from '@/api/hooks'
import { currentSession } from '@/api/auth'
import type { PullRequest, PullRequestStatus } from '@/api/types'
import { formatDate } from '@/shared/lib/format'
import { QueryState } from '@/shared/ui/query-state'
import { UserAvatar } from '@/shared/ui/user-avatar'
import { Button, Input, Label, Textarea } from '@sdlc/ui/ui'
import { PullRequestActions } from './actions'

const pageSize = 20
const pullRequestStatuses: PullRequestStatus[] = ['open', 'closed', 'merged']

const statusStyles: Record<PullRequestStatus, string> = {
  open: 'bg-accent/15 text-accent',
  closed: 'bg-danger/15 text-danger',
  merged: 'bg-success/15 text-success',
}

function CreatePullRequestForm({ repo, onClose }: { repo: string; onClose: () => void }) {
  const { t } = useTranslation()
  const { data: refs = [], isLoading: refsLoading, error: refsError, refetch: refetchRefs } = useRepositoryRefs(repo)
  const createPullRequest = useCreatePullRequest(repo)
  const [form, setForm] = useState({ title: '', description: '', source_branch: '', target_branch: '' })
  const sameBranch = Boolean(form.source_branch.trim() && form.source_branch.trim() === form.target_branch.trim())
  const branches = refs.filter((ref) => ref.kind === 'branch')

  function handleSubmit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (sameBranch || !form.title.trim() || !form.source_branch.trim() || !form.target_branch.trim()) return
    createPullRequest.mutate({
      repository_name: repo,
      title: form.title.trim(),
      description: form.description.trim() || undefined,
      source_branch: form.source_branch.trim(),
      target_branch: form.target_branch.trim(),
      author: currentSession()?.username,
    }, {
      onSuccess: () => {
        toast.success(t('pulls.created'))
        onClose()
      },
      onError: (error) => toast.error(error.message),
    })
  }

  return (
    <form aria-label={t('pulls.create')} onSubmit={handleSubmit} className="grid gap-4 border-y border-border py-4 sm:grid-cols-2">
      <div className="space-y-1.5 sm:col-span-2">
        <Label htmlFor="pr-title">{t('pulls.titleField')}</Label>
        <Input id="pr-title" required value={form.title} onChange={(event) => setForm({ ...form, title: event.target.value })} />
      </div>
      <div className="space-y-1.5">
        <Label htmlFor="pr-source">{t('pulls.sourceBranch')}</Label>
        <Input id="pr-source" required list="pr-source-refs" className="font-mono"
          value={form.source_branch} onChange={(event) => setForm({ ...form, source_branch: event.target.value })} />
        <datalist id="pr-source-refs">{branches.map((ref) => <option key={ref.name} value={ref.name} />)}</datalist>
      </div>
      <div className="space-y-1.5">
        <Label htmlFor="pr-target">{t('pulls.targetBranch')}</Label>
        <Input id="pr-target" required list="pr-target-refs" className="font-mono"
          value={form.target_branch} onChange={(event) => setForm({ ...form, target_branch: event.target.value })} />
        <datalist id="pr-target-refs">{branches.map((ref) => <option key={ref.name} value={ref.name} />)}</datalist>
      </div>
      {sameBranch && <p role="alert" className="text-sm text-danger sm:col-span-2">{t('pulls.branchesMustDiffer')}</p>}
      {refsLoading && <p role="status" className="text-xs text-text-muted sm:col-span-2">{t('pulls.loadingRefs')}</p>}
      {Boolean(refsError) && (
        <div role="alert" className="flex flex-wrap items-center gap-2 text-xs text-text-secondary sm:col-span-2">
          <span>{t('pulls.refsUnavailable')}</span>
          <Button type="button" size="sm" variant="outline" className="min-h-10 sm:min-h-10" onClick={() => refetchRefs()}>{t('common.retry')}</Button>
        </div>
      )}
      <div className="space-y-1.5 sm:col-span-2">
        <Label htmlFor="pr-description">{t('pulls.descriptionField')}</Label>
        <Textarea id="pr-description" rows={3} value={form.description}
          onChange={(event) => setForm({ ...form, description: event.target.value })} />
      </div>
      <div className="flex flex-wrap gap-2 sm:col-span-2">
        <Button type="submit" className="min-h-10" disabled={createPullRequest.isPending || sameBranch}>{t('pulls.create')}</Button>
        <Button type="button" variant="ghost" className="min-h-10" onClick={onClose}>{t('common.cancel')}</Button>
      </div>
    </form>
  )
}

function PullRequestRow({ repo, pullRequest, locale }: { repo: string; pullRequest: PullRequest; locale: string }) {
  const { t } = useTranslation()
  const detailHref = `/repositories/${encodeURIComponent(repo)}/pulls/${pullRequest.number}`

  return (
    <li className="flex min-w-0 flex-col gap-2 py-3 sm:flex-row sm:items-center sm:gap-3">
      <div className="flex min-w-0 flex-1 items-start gap-2">
        <GitPullRequest className="mt-1 h-4 w-4 shrink-0 text-accent" aria-hidden />
        <div className="min-w-0 flex-1">
          <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1">
            <Link to={detailHref} className="min-w-0 break-words text-sm font-medium hover:text-accent focus-visible:outline-2 focus-visible:outline-accent">
              <span className="text-text-muted">#{pullRequest.number}</span> {pullRequest.title}
            </Link>
            <span className={`shrink-0 rounded px-2 py-0.5 text-xs font-medium ${statusStyles[pullRequest.status]}`}>
              {t(`pulls.status_${pullRequest.status}`)}
            </span>
          </div>
          <div className="mt-1 flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 text-xs text-text-muted">
            <code className="break-all">{pullRequest.source_branch}</code>
            <span aria-hidden>→</span>
            <code className="break-all">{pullRequest.target_branch}</code>
            <span className="inline-flex items-center gap-1"><UserAvatar name={pullRequest.created_by} size="xs" />{pullRequest.created_by || t('pulls.unknownAuthor')}</span>
            <time dateTime={pullRequest.created_at}>{formatDate(pullRequest.created_at, locale)}</time>
          </div>
        </div>
      </div>
      <div className="flex shrink-0 items-center gap-1 pl-6 sm:pl-0">
        <Link to={`${detailHref}?view=diff`} aria-label={`${t('pulls.viewDiff')} #${pullRequest.number}`}
          title={`${t('pulls.viewDiff')} #${pullRequest.number}`}
          className="inline-flex h-10 w-10 items-center justify-center rounded-md text-text-secondary hover:bg-surface-raised hover:text-text-primary focus-visible:outline-2 focus-visible:outline-accent">
          <FileDiff className="h-4 w-4" aria-hidden />
        </Link>
        <PullRequestActions repo={repo} pullRequest={pullRequest} compact />
      </div>
    </li>
  )
}

export function PullRequestsPage() {
  const { t, i18n } = useTranslation()
  const { repo } = useParams<{ repo: string }>()
  const [searchParams, setSearchParams] = useSearchParams()
  const [showForm, setShowForm] = useState(false)
  const urlSearch = searchParams.get('q')?.slice(0, 200) ?? ''
  const rawStatus = searchParams.get('status')
  const status: PullRequestStatus | 'all' = pullRequestStatuses.includes(rawStatus as PullRequestStatus)
    ? rawStatus as PullRequestStatus
    : 'all'
  const rawPage = Number(searchParams.get('page'))
  const page = Number.isSafeInteger(rawPage) && rawPage > 0 ? rawPage : 1
  const [searchDraft, setSearchDraft] = useState(urlSearch)
  const { data, isLoading, isFetching, isPlaceholderData, error, refetch } = usePullRequests(repo, {
    limit: pageSize,
    offset: (page - 1) * pageSize,
    status: status === 'all' ? undefined : status,
    search: urlSearch || undefined,
  })
  const hasFilters = Boolean(urlSearch || status !== 'all')

  useEffect(() => setSearchDraft(urlSearch), [urlSearch])

  useEffect(() => {
    const timeout = window.setTimeout(() => {
      const nextQuery = searchDraft.trim()
      if (nextQuery === urlSearch) return
      setSearchParams((current) => {
        const next = new URLSearchParams(current)
        if (nextQuery) next.set('q', nextQuery)
        else next.delete('q')
        next.delete('page')
        return next
      }, { replace: true })
    }, 300)
    return () => window.clearTimeout(timeout)
  }, [searchDraft, setSearchParams, urlSearch])

  useEffect(() => {
    if (!data || isPlaceholderData) return
    const lastPage = Math.max(1, Math.ceil(data.total / pageSize))
    if (page <= lastPage) return
    setSearchParams((current) => {
      const next = new URLSearchParams(current)
      if (lastPage === 1) next.delete('page')
      else next.set('page', String(lastPage))
      return next
    }, { replace: true })
  }, [data, isPlaceholderData, page, setSearchParams])

  function changePage(nextPage: number) {
    setSearchParams((current) => {
      const next = new URLSearchParams(current)
      if (nextPage <= 1) next.delete('page')
      else next.set('page', String(nextPage))
      return next
    })
  }

  function changeStatus(nextStatus: PullRequestStatus | 'all') {
    setSearchParams((current) => {
      const next = new URLSearchParams(current)
      if (nextStatus === 'all') next.delete('status')
      else next.set('status', nextStatus)
      next.delete('page')
      return next
    }, { replace: true })
  }

  if (!repo) return <p className="text-sm text-text-muted">{t('repositories.notFound')}</p>

  return (
    <div className="space-y-5">
      <div className="flex flex-col gap-3 sm:flex-row sm:items-end sm:justify-between">
        <div className="min-w-0">
          <div className="flex min-w-0 flex-wrap items-center gap-2 text-sm text-text-muted">
            <Link to="/repositories" className="hover:text-text-primary">{t('navigation.repositories')}</Link>
            <ChevronRight className="h-3 w-3" aria-hidden />
            <Link to={`/repositories/${encodeURIComponent(repo)}`} className="break-all hover:text-text-primary">{repo}</Link>
            <ChevronRight className="h-3 w-3" aria-hidden />
            <span>{t('repositoryBrowser.pullRequests')}</span>
          </div>
          <div className="mt-2 flex items-center gap-3">
            <GitPullRequest className="h-6 w-6 text-accent" aria-hidden />
            <h1 className="text-2xl font-bold">{t('pulls.title')}</h1>
          </div>
        </div>
        <Button type="button" className="min-h-10 self-start sm:self-auto" onClick={() => setShowForm((value) => !value)}>
          <Plus className="h-4 w-4" aria-hidden />{t('pulls.create')}
        </Button>
      </div>

      {showForm && <CreatePullRequestForm repo={repo} onClose={() => setShowForm(false)} />}

      <QueryState data={data} isLoading={isLoading} error={error} onRetry={() => refetch()}
        isEmpty={(result) => result.total === 0 && !hasFilters} empty={{ title: t('pulls.empty') }}>
        {(result) => {
          const pageCount = Math.max(1, Math.ceil(result.total / pageSize))
          return (
          <section aria-label={t('pulls.title')} aria-busy={isFetching} className="space-y-3">
            <div className="flex flex-col gap-2 sm:flex-row sm:items-center">
              <div className="relative min-w-0 flex-1">
                <Search className="pointer-events-none absolute left-3 top-3 h-4 w-4 text-text-muted" aria-hidden />
                <Input type="search" aria-label={t('pulls.search')} placeholder={t('pulls.search')}
                  className="min-h-10 pl-9" maxLength={200} value={searchDraft}
                  onChange={(event) => setSearchDraft(event.target.value)} />
              </div>
              <Label htmlFor="pr-status" className="sr-only">{t('pulls.statusFilter')}</Label>
              <select id="pr-status" value={status} onChange={(event) => changeStatus(event.target.value as PullRequestStatus | 'all')}
                className="min-h-10 rounded-md border border-border bg-surface px-2 text-sm text-text-primary outline-none focus-visible:border-accent sm:w-40">
                <option value="all">{t('pulls.allStatuses')}</option>
                <option value="open">{t('pulls.status_open')}</option>
                <option value="closed">{t('pulls.status_closed')}</option>
                <option value="merged">{t('pulls.status_merged')}</option>
              </select>
            </div>
            <p className="flex flex-wrap gap-2 text-xs text-text-muted" aria-live="polite">
              <span>{t('pulls.shown', { count: result.items.length, total: result.total })}</span>
              {isFetching && <span>{t('common.loading')}</span>}
            </p>
            {result.total === 0 ? (
              <p role="status" className="border-y border-border py-6 text-sm text-text-muted">{t('pulls.noMatches')}</p>
            ) : (
              <ul className="divide-y divide-border border-y border-border">
                {result.items.map((pullRequest) => <PullRequestRow key={pullRequest.id} repo={repo} pullRequest={pullRequest} locale={i18n.language} />)}
              </ul>
            )}
            {result.total > pageSize && (
              <nav aria-label={t('pulls.pages')} className="flex items-center justify-end gap-2">
                <Button type="button" size="sm" variant="outline" className="min-h-10 sm:min-h-10" disabled={page === 1 || isFetching} onClick={() => changePage(page - 1)}>{t('pulls.previous')}</Button>
                <span className="text-sm text-text-muted">{page} / {pageCount}</span>
                <Button type="button" size="sm" variant="outline" className="min-h-10 sm:min-h-10" disabled={page === pageCount || isFetching} onClick={() => changePage(page + 1)}>{t('pulls.next')}</Button>
              </nav>
            )}
          </section>
          )
        }}
      </QueryState>
    </div>
  )
}
