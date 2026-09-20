import { useState } from 'react'
import { Link, useParams, useSearchParams } from 'react-router'
import { useTranslation } from 'react-i18next'
import { GitBranch, GitCompareArrows, GitPullRequest, ChevronRight, Folder, FileText, Tag, Package, ArrowLeft } from 'lucide-react'
import { toast } from 'sonner'
import { useRepositoryCommits, useRepositoryRefs, useRepositoryTree, useRepositoryBlob, useRepositoryTags, useReleases, useCreateRelease, useDeleteRelease } from '@/api/hooks'
import {
  Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter,
} from '@sdlc/ui/ui'
import { Input } from '@sdlc/ui/ui'
import { Label } from '@sdlc/ui/ui'
import { Textarea } from '@sdlc/ui/ui'
import {
  AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription,
  AlertDialogFooter, AlertDialogHeader, AlertDialogTitle,
} from '@sdlc/ui/ui'
import { UserAvatar } from '@/shared/ui/user-avatar'
import { QueryState } from '@/shared/ui/query-state'
import type { RepositoryRef } from '@/api/types'
import { Button } from '@sdlc/ui/ui'
import { Card } from '@sdlc/ui/ui'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@sdlc/ui/ui'

const browserTabs = ['code', 'commits', 'branches', 'tags', 'releases'] as const

function parentPath(path: string): string {
  return path.slice(0, path.lastIndexOf('/'))
}

function formatDate(value: string, locale: string): string {
  const date = new Date(value)
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString(locale)
}

export function RepositoryBrowserPage() {
  const { t, i18n } = useTranslation()
  const { repo } = useParams<{ repo: string }>()
  const [searchParams, setSearchParams] = useSearchParams()
  const tabParam = searchParams.get('tab')
  const tab = browserTabs.find((value) => value === tabParam) ?? 'code'
  const gitRef = searchParams.get('ref') || 'HEAD'
  const filePath = searchParams.get('file') || null
  const dirPath = searchParams.get('dir') ?? (filePath ? parentPath(filePath) : '')
  const refsQuery = useRepositoryRefs(repo)

  if (!repo) return <p className="text-sm text-text-muted">{t('repositories.notFound')}</p>

  function updateParams(changes: Record<string, string | null>) {
    const next = new URLSearchParams(searchParams)
    for (const [key, value] of Object.entries(changes)) {
      if (value) next.set(key, value)
      else next.delete(key)
    }
    setSearchParams(next)
  }

  return (
    <div className="space-y-6">
      <div className="flex flex-col gap-4 sm:flex-row sm:items-end sm:justify-between">
        <div>
          <div className="flex items-center gap-2 text-sm text-text-muted">
            <Link to="/repositories" className="hover:text-text-primary">{t('navigation.repositories')}</Link>
            <ChevronRight className="h-3 w-3" />
            <span className="min-w-0 break-all">{repo}</span>
          </div>
          <div className="mt-2 flex items-center gap-3">
            <GitBranch className="h-6 w-6 text-accent" />
            <h1 className="min-w-0 break-all text-2xl font-bold">{repo}</h1>
          </div>
        </div>
        <div className="flex flex-wrap gap-2">
          <Button asChild variant="outline" size="sm">
            <Link to={`/repositories/${encodeURIComponent(repo)}/compare`}>
              <GitCompareArrows className="h-4 w-4" /> {t('repositoryBrowser.compareChanges')}
            </Link>
          </Button>
          <Button asChild size="sm">
            <Link to={`/repositories/${encodeURIComponent(repo)}/pulls`}>
              <GitPullRequest className="h-4 w-4" /> {t('repositoryBrowser.createPullRequest')}
            </Link>
          </Button>
        </div>
      </div>

      <Tabs value={tab} onValueChange={(value) => updateParams({ tab: value === 'code' ? null : value })}>
          <TabsList className="grid h-auto w-full grid-cols-3 gap-1 sm:inline-flex sm:w-auto">
            <TabsTrigger value="code" className="min-h-10 min-w-0 px-2 sm:px-3">{t('repositoryBrowser.code', 'Код')}</TabsTrigger>
            <TabsTrigger value="commits" className="min-h-10 min-w-0 px-2 sm:px-3">{t('repositoryBrowser.commits')}</TabsTrigger>
            <TabsTrigger value="branches" className="min-h-10 min-w-0 px-2 sm:px-3">{t('repositoryBrowser.branches')}</TabsTrigger>
            <TabsTrigger value="tags" className="min-h-10 min-w-0 px-2 sm:px-3">{t('repositoryBrowser.tags', 'Теги')}</TabsTrigger>
            <TabsTrigger value="releases" className="min-h-10 min-w-0 px-2 sm:px-3">{t('repositoryBrowser.releases', 'Релизы')}</TabsTrigger>
          </TabsList>

          <TabsContent value="code" className="mt-4">
            <CodeBrowser
              repo={repo}
              gitRef={gitRef}
              dirPath={dirPath}
              filePath={filePath}
              refsQuery={refsQuery}
              onNavigate={updateParams}
            />
          </TabsContent>
          <TabsContent value="commits" className="mt-4">
            <CommitsList repo={repo} gitRef={gitRef} locale={i18n.language} />
          </TabsContent>
          <TabsContent value="branches" className="mt-4">
            <BranchesList refsQuery={refsQuery} onSelect={(ref) => updateParams({ tab: null, ref: `refs/heads/${ref.name}`, dir: null, file: null })} />
          </TabsContent>
          <TabsContent value="tags" className="mt-4">
            <TagsList repo={repo} />
          </TabsContent>
          <TabsContent value="releases" className="mt-4">
            <ReleasesList repo={repo} />
          </TabsContent>
      </Tabs>
    </div>
  )
}

function CommitsList({ repo, gitRef, locale }: { repo: string; gitRef: string; locale: string }) {
  const { t } = useTranslation()
  const query = useRepositoryCommits(repo, gitRef)
  return (
    <QueryState
      data={query.data}
      isLoading={query.isLoading}
      error={query.error}
      errorMessage={t('repositoryBrowser.commitsLoadError')}
      onRetry={() => void query.refetch()}
      isEmpty={(commits) => commits.length === 0}
      empty={{ title: t('repositoryBrowser.noCommits') }}
    >
      {(commits) => (
        <ul className="divide-y divide-border rounded-md border border-border">
          {commits.map((commit) => (
            <li key={commit.sha} className="flex min-w-0 flex-col gap-1 px-3 py-2.5 text-sm sm:flex-row sm:items-center sm:gap-4">
              <code className="shrink-0 text-xs text-accent">{commit.short_sha}</code>
              <span className="min-w-0 flex-1 break-words font-medium">{commit.message}</span>
              <span className="min-w-0 break-words text-xs text-text-secondary">{commit.author}</span>
              <time className="shrink-0 text-xs text-text-muted" dateTime={commit.date}>{formatDate(commit.date, locale)}</time>
            </li>
          ))}
        </ul>
      )}
    </QueryState>
  )
}

function BranchesList({ refsQuery, onSelect }: { refsQuery: ReturnType<typeof useRepositoryRefs>; onSelect: (ref: RepositoryRef) => void }) {
  const { t } = useTranslation()
  const branches = refsQuery.data?.filter((ref) => ref.kind === 'branch')
  return (
    <QueryState
      data={branches}
      isLoading={refsQuery.isLoading}
      error={refsQuery.error}
      errorMessage={t('repositoryBrowser.branchesLoadError')}
      onRetry={() => void refsQuery.refetch()}
      isEmpty={(items) => items.length === 0}
      empty={{ title: t('repositoryBrowser.noBranches') }}
    >
      {(items) => (
        <ul className="divide-y divide-border rounded-md border border-border">
          {items.map((ref) => (
            <li key={ref.name} className="flex min-w-0 flex-wrap items-center gap-x-4 gap-y-1 px-3 py-2 text-sm">
              <button type="button" className="flex min-h-10 min-w-0 flex-1 items-center gap-2 text-left font-medium hover:text-accent" onClick={() => onSelect(ref)}>
                <GitBranch className="h-4 w-4 shrink-0 text-accent" aria-hidden />
                <span className="break-all">{ref.name}</span>
              </button>
              <code className="text-xs text-text-muted">{ref.sha.slice(0, 7)}</code>
              {ref.target && <span className="w-full break-words text-xs text-text-muted sm:w-auto sm:max-w-[40%] sm:truncate">{ref.target}</span>}
            </li>
          ))}
        </ul>
      )}
    </QueryState>
  )
}

type NavigateParams = (changes: Record<string, string | null>) => void

function CodeBrowser({ repo, gitRef, dirPath, filePath, refsQuery, onNavigate }: {
  repo: string
  gitRef: string
  dirPath: string
  filePath: string | null
  refsQuery: ReturnType<typeof useRepositoryRefs>
  onNavigate: NavigateParams
}) {
  const { t } = useTranslation()
  const refs = refsQuery.data?.filter((ref) => ref.kind === 'branch' || ref.kind === 'tag') ?? []
  const refOptions = refs.map((ref) => ({ value: `refs/${ref.kind === 'branch' ? 'heads' : 'tags'}/${ref.name}`, label: `${ref.kind === 'branch' ? t('repositoryBrowser.branches') : t('repositoryBrowser.tags')} / ${ref.name}` }))

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center gap-3">
        <label htmlFor="repository-ref" className="text-sm font-medium">{t('repositoryBrowser.gitRef')}</label>
        <select
          id="repository-ref"
          className="min-h-10 min-w-0 max-w-full rounded-md border border-border bg-surface px-3 text-sm text-text-primary"
          value={gitRef}
          onChange={(event) => onNavigate({ ref: event.target.value === 'HEAD' ? null : event.target.value, dir: null, file: null })}
        >
          <option value="HEAD">{t('repositoryBrowser.defaultRef')}</option>
          {gitRef !== 'HEAD' && !refOptions.some((option) => option.value === gitRef) && <option value={gitRef}>{gitRef}</option>}
          {refOptions.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}
        </select>
      </div>
      {Boolean(refsQuery.error) && (
        <div role="alert" className="flex flex-wrap items-center gap-3 rounded-md border border-status-failed/40 p-3 text-sm">
          <span>{t('repositoryBrowser.refsLoadError')}</span>
          <Button type="button" size="sm" variant="outline" onClick={() => void refsQuery.refetch()}>{t('common.retry')}</Button>
        </div>
      )}
      {filePath ? (
        <FilePreview repo={repo} gitRef={gitRef} filePath={filePath} onBack={() => onNavigate({ file: null, dir: parentPath(filePath) })} />
      ) : (
        <TreeView repo={repo} gitRef={gitRef} dirPath={dirPath} onNavigate={onNavigate} />
      )}
    </div>
  )
}

function FilePreview({ repo, gitRef, filePath, onBack }: { repo: string; gitRef: string; filePath: string; onBack: () => void }) {
  const { t } = useTranslation()
  const query = useRepositoryBlob(repo, gitRef, filePath)
  return (
    <div className="overflow-hidden rounded-md border border-border">
      <div className="flex min-w-0 flex-wrap items-center gap-2 border-b border-border p-2 text-sm">
        <Button type="button" variant="ghost" size="sm" className="min-h-10 min-w-10" aria-label={t('repositoryBrowser.backToTree')} title={t('repositoryBrowser.backToTree')} onClick={onBack}>
          <ArrowLeft className="h-4 w-4" aria-hidden />
        </Button>
        <FileText className="h-4 w-4 shrink-0 text-accent" aria-hidden />
        <span className="min-w-0 flex-1 break-all font-medium">{filePath}</span>
        {query.data && <span className="text-xs text-text-muted">{query.data.size} B · {query.data.sha.slice(0, 7)}</span>}
      </div>
      <div className="p-4">
        <QueryState data={query.data} isLoading={query.isLoading} error={query.error} errorMessage={t('repositoryBrowser.fileLoadError')} onRetry={() => void query.refetch()}>
          {(blob) => blob.binary ? (
            <p className="text-sm text-text-muted">{t('repositoryBrowser.binaryFile')}</p>
          ) : blob.content.length === 0 ? (
            <p className="text-sm text-text-muted">{t('repositoryBrowser.emptyFile')}</p>
          ) : (
            <div>
              {blob.truncated && <p className="mb-3 text-xs text-text-muted">{t('repositoryBrowser.truncatedFile')}</p>}
              <pre className="max-h-[70vh] overflow-auto text-xs leading-relaxed"><code>{blob.content}</code></pre>
            </div>
          )}
        </QueryState>
      </div>
    </div>
  )
}

function TreeView({ repo, gitRef, dirPath, onNavigate }: { repo: string; gitRef: string; dirPath: string; onNavigate: NavigateParams }) {
  const { t } = useTranslation()
  const query = useRepositoryTree(repo, gitRef, dirPath || undefined)
  const crumbs = dirPath ? dirPath.split('/') : []
  return (
    <div className="overflow-hidden rounded-md border border-border">
      <nav aria-label={t('repositoryBrowser.code')} className="flex min-w-0 flex-wrap items-center gap-1 border-b border-border px-2 py-1 text-sm">
        <Folder className="h-4 w-4 shrink-0 text-accent" aria-hidden />
        <button type="button" className="min-h-10 px-2 hover:text-accent" onClick={() => onNavigate({ dir: null, file: null })}>/</button>
        {crumbs.map((part, index) => (
          <span key={`${index}-${part}`} className="flex min-w-0 items-center gap-1">
            <span className="text-text-muted">/</span>
            <button type="button" className="min-h-10 min-w-0 break-all px-2 text-left hover:text-accent" onClick={() => onNavigate({ dir: crumbs.slice(0, index + 1).join('/'), file: null })}>{part}</button>
          </span>
        ))}
      </nav>
      <div className="p-2">
        <QueryState data={query.data} isLoading={query.isLoading} error={query.error} errorMessage={t('repositoryBrowser.codeLoadError')} onRetry={() => void query.refetch()} isEmpty={(entries) => entries.length === 0} empty={{ title: t('repositoryBrowser.emptyTree') }}>
          {(entries) => (
            <ul className="divide-y divide-border">
              {[...entries].sort((a, b) => (a.kind === b.kind ? a.name.localeCompare(b.name) : a.kind === 'tree' ? -1 : 1)).map((entry) => (
                <li key={entry.path} className="flex min-w-0 items-center gap-2 text-sm hover:bg-surface-raised">
                  <button type="button" className="flex min-h-11 min-w-0 flex-1 items-center gap-3 px-2 text-left" onClick={() => onNavigate(entry.kind === 'tree' ? { dir: entry.path, file: null } : { file: entry.path })}>
                    {entry.kind === 'tree' ? <Folder className="h-4 w-4 shrink-0 text-accent" aria-hidden /> : <FileText className="h-4 w-4 shrink-0 text-text-muted" aria-hidden />}
                    <span className="min-w-0 break-all">{entry.name}</span>
                  </button>
                  <code className="hidden shrink-0 pr-2 text-xs text-text-muted sm:inline">{entry.sha.slice(0, 7)}</code>
                  {entry.size != null && <span className="hidden w-20 shrink-0 pr-2 text-right text-xs text-text-muted md:inline">{entry.size} B</span>}
                </li>
              ))}
            </ul>
          )}
        </QueryState>
      </div>
    </div>
  )
}

function TagsList({ repo }: { repo: string }) {
  const { t } = useTranslation()
  const query = useRepositoryTags(repo)
  return (
    <QueryState data={query.data} isLoading={query.isLoading} error={query.error} errorMessage={t('repositoryBrowser.tagsLoadError')} onRetry={() => void query.refetch()} isEmpty={(tags) => tags.length === 0} empty={{ title: t('repositoryBrowser.noTags') }}>
      {(tags) => (
        <ul className="divide-y divide-border rounded-md border border-border">
          {tags.map((tag) => (
            <li key={tag.name} className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1 px-3 py-2.5 text-sm">
              <Tag className="h-4 w-4 shrink-0 text-accent" aria-hidden />
              <span className="min-w-0 break-all font-medium">{tag.name}</span>
              <code className="text-xs text-text-muted">{tag.sha.slice(0, 7)}</code>
              {tag.message && <span className="w-full min-w-0 break-words text-xs text-text-muted sm:ml-auto sm:w-auto sm:max-w-[40%] sm:truncate">{tag.message}</span>}
            </li>
          ))}
        </ul>
      )}
    </QueryState>
  )
}

function ReleasesList({ repo }: { repo: string }) {
  const { t } = useTranslation()
  const releasesQuery = useReleases(repo)
  const createRelease = useCreateRelease(repo)
  const deleteRelease = useDeleteRelease(repo)
  const [open, setOpen] = useState(false)
  const [pendingDelete, setPendingDelete] = useState<string | null>(null)
  const [form, setForm] = useState({ tag_name: '', name: '', description: '', prerelease: false })

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    createRelease.mutate(
      { tag_name: form.tag_name.trim(), name: form.name.trim() || form.tag_name.trim(), description: form.description.trim() || undefined, prerelease: form.prerelease },
      {
        onSuccess: () => {
          toast.success(t('releases.created', 'Релиз создан'))
          setOpen(false)
          setForm({ tag_name: '', name: '', description: '', prerelease: false })
        },
        onError: (err) => toast.error(err.message),
      },
    )
  }

  return (
    <div className="space-y-3">
      <div className="flex justify-end">
        <Button onClick={() => setOpen(true)}>+ {t('releases.create', 'Создать релиз')}</Button>
      </div>
      <QueryState data={releasesQuery.data} isLoading={releasesQuery.isLoading} error={releasesQuery.error} errorMessage={t('repositoryBrowser.releasesLoadError')} onRetry={() => void releasesQuery.refetch()} isEmpty={(releases) => releases.length === 0} empty={{ title: t('releases.none') }}>
        {(releases) => <ul className="grid gap-3">
          {releases.map((rel) => (
            <li key={rel.id}>
              <Card className="p-4">
                <div className="flex items-start justify-between gap-3">
                  <div className="min-w-0">
                    <div className="flex items-center gap-2">
                      <Package className="h-4 w-4 text-accent" />
                      <p className="truncate font-medium">{rel.name}</p>
                      {rel.prerelease && (
                        <span className="rounded-full bg-amber-500/15 px-2 py-0.5 text-xs text-amber-500">{t('releases.prerelease', 'пре-релиз')}</span>
                      )}
                    </div>
                    <p className="mt-1 flex items-center gap-2 text-xs text-text-muted">
                      <Tag className="h-3 w-3" />{rel.tag_name}
                      {rel.created_by && <><UserAvatar name={rel.created_by} size="xs" />{rel.created_by.slice(0, 8)}</>}
                    </p>
                    {rel.description && <p className="mt-2 whitespace-pre-wrap text-sm">{rel.description}</p>}
                  </div>
                  <Button variant="outline" size="sm" className="text-destructive" onClick={() => setPendingDelete(rel.tag_name)}>
                    {t('common.delete')}
                  </Button>
                </div>
              </Card>
            </li>
          ))}
        </ul>}
      </QueryState>

      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t('releases.create', 'Создать релиз')}</DialogTitle>
          </DialogHeader>
          <form onSubmit={handleSubmit} className="grid gap-3">
            <div className="space-y-1.5">
              <Label htmlFor="rel-tag">{t('releases.tag', 'Тег')}</Label>
              <Input id="rel-tag" value={form.tag_name} onChange={(e) => setForm({ ...form, tag_name: e.target.value })} placeholder="v1.0.0" required />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="rel-name">{t('releases.name', 'Название')}</Label>
              <Input id="rel-name" value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="rel-desc">{t('releases.description', 'Описание')}</Label>
              <Textarea id="rel-desc" value={form.description} onChange={(e) => setForm({ ...form, description: e.target.value })} />
            </div>
            <div className="flex items-center gap-2">
              <input
                id="rel-pre"
                type="checkbox"
                className="h-4 w-4 rounded border-border bg-surface"
                checked={form.prerelease}
                onChange={(e) => setForm({ ...form, prerelease: e.target.checked })}
              />
              <Label htmlFor="rel-pre">{t('releases.prerelease', 'пре-релиз')}</Label>
            </div>
            <DialogFooter>
              <Button type="button" variant="outline" onClick={() => setOpen(false)}>{t('common.cancel')}</Button>
              <Button type="submit" disabled={createRelease.isPending}>{t('common.create')}</Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>

      <AlertDialog open={!!pendingDelete} onOpenChange={(v) => !v && setPendingDelete(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('releases.deleteTitle', 'Удалить релиз?')}</AlertDialogTitle>
            <AlertDialogDescription>
              {t('releases.deleteConfirm', 'Релиз будет удалён. Тег в git останется.')}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel onClick={() => setPendingDelete(null)}>{t('common.cancel')}</AlertDialogCancel>
            <AlertDialogAction
              onClick={() => {
                if (pendingDelete) deleteRelease.mutate(pendingDelete, { onSuccess: () => setPendingDelete(null) })
              }}
            >
              {t('common.delete')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  )
}
