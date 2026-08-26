import { useState } from 'react'
import { Link, useParams } from 'react-router'
import { useTranslation } from 'react-i18next'
import { ChevronRight, GitPullRequest, GitMerge, Plus, RotateCcw, XCircle } from 'lucide-react'
import { toast } from 'sonner'
import {
  useCreatePullRequest,
  usePullRequestAction,
  usePullRequests,
  useRepositoryRefs,
} from '@/api/hooks'
import { Button } from '@/shared/ui/button'
import { Card } from '@/shared/ui/card'
import { Input } from '@/shared/ui/input'
import { Label } from '@/shared/ui/label'
import { Textarea } from '@/shared/ui/textarea'
import type { PullRequest, PullRequestStatus } from '@/api/types'

const statusStyles: Record<PullRequestStatus, string> = {
  open: 'bg-success/15 text-success',
  closed: 'bg-danger/15 text-danger',
  merged: 'bg-accent/15 text-accent',
}

function formatDate(value: string, locale: string): string {
  const date = new Date(value)
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString(locale)
}

function CreatePullRequestForm({ repo, onClose }: { repo: string; onClose: () => void }) {
  const { t } = useTranslation()
  const { data: refs = [] } = useRepositoryRefs(repo)
  const createPullRequest = useCreatePullRequest(repo)
  const [form, setForm] = useState({ title: '', description: '', source_branch: 'feature/login', target_branch: 'main' })

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    createPullRequest.mutate(
      {
        repository_name: repo,
        title: form.title.trim(),
        description: form.description.trim() || undefined,
        source_branch: form.source_branch.trim(),
        target_branch: form.target_branch.trim(),
      },
      {
        onSuccess: () => {
          toast.success(t('pulls.created'))
          onClose()
        },
        onError: (err) => toast.error(err.message),
      },
    )
  }

  return (
    <Card className="p-4">
      <form onSubmit={handleSubmit} className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
        <div className="space-y-1.5">
          <Label htmlFor="pr-title">{t('pulls.titleField')}</Label>
          <Input
            id="pr-title"
            required
            value={form.title}
            onChange={(e) => setForm({ ...form, title: e.target.value })}
          />
        </div>
        <div className="space-y-1.5">
          <Label htmlFor="pr-source">{t('pulls.sourceBranch')}</Label>
          <Input
            id="pr-source"
            required
            list="pr-source-refs"
            className="font-mono"
            value={form.source_branch}
            onChange={(e) => setForm({ ...form, source_branch: e.target.value })}
          />
          <datalist id="pr-source-refs">
            {refs.map((ref) => <option key={ref.name} value={ref.name} />)}
          </datalist>
        </div>
        <div className="space-y-1.5">
          <Label htmlFor="pr-target">{t('pulls.targetBranch')}</Label>
          <Input
            id="pr-target"
            required
            list="pr-target-refs"
            className="font-mono"
            value={form.target_branch}
            onChange={(e) => setForm({ ...form, target_branch: e.target.value })}
          />
          <datalist id="pr-target-refs">
            {refs.map((ref) => <option key={ref.name} value={ref.name} />)}
          </datalist>
        </div>
        <div className="space-y-1.5">
          <Label htmlFor="pr-description">{t('pulls.descriptionField')}</Label>
          <Textarea
            id="pr-description"
            rows={1}
            className="min-h-9"
            value={form.description}
            onChange={(e) => setForm({ ...form, description: e.target.value })}
          />
        </div>
        <div className="flex gap-2 sm:col-span-2 lg:col-span-4">
          <Button type="submit" disabled={createPullRequest.isPending}>{t('pulls.create')}</Button>
          <Button type="button" variant="ghost" onClick={onClose}>{t('common.cancel')}</Button>
        </div>
      </form>
    </Card>
  )
}

function PullRequestCard({ repo, pullRequest, locale }: { repo: string; pullRequest: PullRequest; locale: string }) {
  const { t } = useTranslation()
  const prAction = usePullRequestAction(repo)

  function handleAction(action: 'merge' | 'close' | 'reopen') {
    prAction.mutate(
      { number: pullRequest.number, action },
      { onError: (err) => toast.error(err.message) },
    )
  }

  return (
    <Card className="p-4">
      <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            <GitPullRequest className="h-4 w-4 shrink-0 text-accent" />
            <span className="font-medium">#{pullRequest.number}</span>
            <span className="truncate font-medium">{pullRequest.title}</span>
            <span className={`rounded px-2 py-0.5 text-xs font-medium ${statusStyles[pullRequest.status]}`}>
              {t(`pulls.status_${pullRequest.status}`)}
            </span>
          </div>
          <p className="mt-2 flex flex-wrap items-center gap-2 text-xs text-text-muted">
            <code className="rounded bg-surface-raised px-1.5 py-0.5">{pullRequest.source_branch}</code>
            <span>→</span>
            <code className="rounded bg-surface-raised px-1.5 py-0.5">{pullRequest.target_branch}</code>
            <span>· {formatDate(pullRequest.created_at, locale)}</span>
          </p>
          {pullRequest.description && (
            <p className="mt-2 text-sm text-text-secondary">{pullRequest.description}</p>
          )}
          {pullRequest.merge_commit_sha && (
            <p className="mt-2 text-xs text-text-muted">
              {t('pulls.mergeCommit')}: <code className="rounded bg-surface-raised px-1.5 py-0.5">{pullRequest.merge_commit_sha.slice(0, 7)}</code>
            </p>
          )}
        </div>
        <div className="flex shrink-0 flex-wrap gap-2">
          {pullRequest.status === 'open' && (
            <>
              <Button size="sm" disabled={prAction.isPending} onClick={() => handleAction('merge')}>
                <GitMerge className="h-3 w-3" /> {t('pulls.merge')}
              </Button>
              <Button size="sm" variant="outline" disabled={prAction.isPending} onClick={() => handleAction('close')}>
                <XCircle className="h-3 w-3" /> {t('pulls.close')}
              </Button>
            </>
          )}
          {pullRequest.status === 'closed' && (
            <Button size="sm" variant="outline" disabled={prAction.isPending} onClick={() => handleAction('reopen')}>
              <RotateCcw className="h-3 w-3" /> {t('pulls.reopen')}
            </Button>
          )}
        </div>
      </div>
    </Card>
  )
}

export function PullRequestsPage() {
  const { t, i18n } = useTranslation()
  const { repo } = useParams<{ repo: string }>()
  const { data: pullRequests = [], isLoading, isError, error } = usePullRequests(repo)
  const [showForm, setShowForm] = useState(false)

  if (!repo) return <p className="text-sm text-text-muted">{t('repositories.notFound')}</p>

  return (
    <div className="space-y-6">
      <div className="flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <div className="flex items-center gap-2 text-sm text-text-muted">
            <Link to="/repositories" className="hover:text-text-primary">{t('navigation.repositories')}</Link>
            <ChevronRight className="h-3 w-3" />
            <Link to={`/repositories/${encodeURIComponent(repo)}`} className="hover:text-text-primary">{repo}</Link>
            <ChevronRight className="h-3 w-3" />
            <span>{t('repositoryBrowser.pullRequests')}</span>
          </div>
          <div className="mt-2 flex items-center gap-3">
            <GitPullRequest className="h-6 w-6 text-accent" />
            <h1 className="text-2xl font-bold">{t('pulls.title')}</h1>
          </div>
        </div>
        <Button size="sm" onClick={() => setShowForm((v) => !v)}>
          <Plus className="h-4 w-4" />
          {t('pulls.create')}
        </Button>
      </div>

      {showForm && <CreatePullRequestForm repo={repo} onClose={() => setShowForm(false)} />}

      {isLoading ? (
        <p className="text-sm text-text-muted">{t('common.loading')}</p>
      ) : isError ? (
        <Card className="p-6 text-sm text-danger">
          {t('common.error')}: {error instanceof Error ? error.message : String(error)}
        </Card>
      ) : pullRequests.length === 0 ? (
        <Card className="p-8 text-center text-text-muted">{t('pulls.empty')}</Card>
      ) : (
        <div className="space-y-3">
          {pullRequests.map((pullRequest) => (
            <PullRequestCard key={pullRequest.id} repo={repo} pullRequest={pullRequest} locale={i18n.language} />
          ))}
        </div>
      )}
    </div>
  )
}
