import { useEffect, useState } from 'react'
import { Link, useParams, useSearchParams } from 'react-router'
import { useTranslation } from 'react-i18next'
import { ArrowLeft, Check, ChevronLeft, ChevronRight, FileText, Folder, GitBranch, GitCompareArrows, GitPullRequest, Package, Pencil, Plus, Search, Tag, Trash2, X } from 'lucide-react'
import { toast } from 'sonner'
import { useRepositoryCommits, useRepositoryRefs, useRepositoryTree, useRepositoryBlob, useRepositoryTags, useReleases, useCreateRelease, useDeleteRelease } from '@/api/hooks'
import {
  Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter,
} from '@sdlc/ui/ui'
import { Input } from '@sdlc/ui/ui'
import { Label } from '@sdlc/ui/ui'
import { Textarea } from '@sdlc/ui/ui'
import { ConfirmDialog } from '@/shared/ui/confirm-dialog'
import { QueryState } from '@/shared/ui/query-state'
import { useDebouncedValue } from '@/shared/lib/use-debounced-value'
import type { Release, RepositoryRef } from '@/api/types'
import { Button } from '@sdlc/ui/ui'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@sdlc/ui/ui'

const browserTabs = ['code', 'commits', 'branches', 'tags', 'releases'] as const
const refPageSize = 100
const refSuggestionLimit = 51

function parentPath(path: string): string {
  return path.slice(0, path.lastIndexOf('/'))
}

function formatDate(value: string, locale: string): string {
  const date = new Date(value)
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString(locale)
}

function parsePage(value: string | null): number {
  const page = Number(value)
  return Number.isSafeInteger(page) && page > 0 ? page : 1
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
  const refPage = parsePage(searchParams.get('refPage'))
  const refSearch = searchParams.get('refSearch')?.trim() ?? ''

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
            <Link to="/repositories" className="inline-flex min-h-10 items-center hover:text-text-primary">{t('navigation.repositories')}</Link>
            <ChevronRight className="h-3 w-3" />
            <span className="min-w-0 break-all">{repo}</span>
          </div>
          <div className="mt-2 flex items-center gap-3">
            <GitBranch className="h-6 w-6 text-accent" />
            <h1 className="min-w-0 break-all text-2xl font-bold">{repo}</h1>
          </div>
        </div>
        <div className="flex flex-wrap gap-2">
          <Button asChild variant="outline" size="sm" className="h-10">
            <Link to={`/repositories/${encodeURIComponent(repo)}/compare`}>
              <GitCompareArrows className="h-4 w-4" /> {t('repositoryBrowser.compareChanges')}
            </Link>
          </Button>
          <Button asChild size="sm" className="h-10">
            <Link to={`/repositories/${encodeURIComponent(repo)}/pulls`}>
              <GitPullRequest className="h-4 w-4" /> {t('repositoryBrowser.createPullRequest')}
            </Link>
          </Button>
        </div>
      </div>

      <Tabs value={tab} onValueChange={(value) => updateParams({ tab: value === 'code' ? null : value, refPage: null, refSearch: null })}>
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
              onNavigate={updateParams}
            />
          </TabsContent>
          <TabsContent value="commits" className="mt-4">
            <CommitsList repo={repo} gitRef={gitRef} locale={i18n.language} />
          </TabsContent>
          <TabsContent value="branches" className="mt-4">
            <BranchesList repo={repo} page={refPage} search={refSearch} onNavigate={updateParams} onSelect={(ref) => updateParams({ tab: null, ref: `refs/heads/${ref.name}`, dir: null, file: null, refPage: null, refSearch: null })} />
          </TabsContent>
          <TabsContent value="tags" className="mt-4">
            <TagsList repo={repo} page={refPage} search={refSearch} onNavigate={updateParams} onSelect={(tag) => updateParams({ tab: null, ref: `refs/tags/${tag.name}`, dir: null, file: null, refPage: null, refSearch: null })} />
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

type NavigateParams = (changes: Record<string, string | null>) => void

function RefListSearch({ search, onSearch }: { search: string; onSearch: (search: string) => void }) {
  const { t } = useTranslation()
  const [draft, setDraft] = useState(search)

  useEffect(() => setDraft(search), [search])

  return (
    <form className="flex min-w-0 items-center gap-2" role="search" onSubmit={(event) => { event.preventDefault(); onSearch(draft.trim()) }}>
      <div className="relative min-w-0 flex-1 sm:max-w-sm">
        <Search className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-text-muted" aria-hidden />
        <Input type="search" className="h-10 pl-9" aria-label={t('repositoryBrowser.searchRefs')} placeholder={t('repositoryBrowser.searchRefs')} value={draft} onChange={(event) => setDraft(event.target.value)} />
      </div>
      <Button type="submit" variant="outline" size="sm" className="h-10 w-10 p-0" aria-label={t('repositoryBrowser.applyRefSearch')} title={t('repositoryBrowser.applyRefSearch')}>
        <Search className="h-4 w-4" aria-hidden />
      </Button>
      {(draft || search) && (
        <Button type="button" variant="ghost" size="sm" className="h-10 w-10 p-0" aria-label={t('repositoryBrowser.clearRefSearch')} title={t('repositoryBrowser.clearRefSearch')} onClick={() => { setDraft(''); onSearch('') }}>
          <X className="h-4 w-4" aria-hidden />
        </Button>
      )}
    </form>
  )
}

function RefPagination({ page, hasNext, onPage }: { page: number; hasNext: boolean; onPage: (page: number) => void }) {
  const { t } = useTranslation()
  if (page <= 1 && !hasNext) return null
  return (
    <nav aria-label={t('repositoryBrowser.refPagination')} className="flex items-center justify-center gap-3 border-t border-border pt-2">
      <Button type="button" variant="outline" size="sm" className="h-10 w-10 p-0" disabled={page <= 1} aria-label={t('repositoryBrowser.previousRefPage')} title={t('repositoryBrowser.previousRefPage')} onClick={() => onPage(page - 1)}>
        <ChevronLeft className="h-4 w-4" aria-hidden />
      </Button>
      <span className="text-sm text-text-secondary">{t('repositoryBrowser.refPage', { page })}</span>
      <Button type="button" variant="outline" size="sm" className="h-10 w-10 p-0" disabled={!hasNext} aria-label={t('repositoryBrowser.nextRefPage')} title={t('repositoryBrowser.nextRefPage')} onClick={() => onPage(page + 1)}>
        <ChevronRight className="h-4 w-4" aria-hidden />
      </Button>
    </nav>
  )
}

function BranchesList({ repo, page, search, onNavigate, onSelect }: { repo: string; page: number; search: string; onNavigate: NavigateParams; onSelect: (ref: RepositoryRef) => void }) {
  const { t } = useTranslation()
  const query = useRepositoryRefs(repo, { kind: 'branch', limit: refPageSize + 1, offset: (page - 1) * refPageSize, search })
  const allBranches = query.data?.filter((ref) => ref.kind === 'branch')
  const branches = allBranches?.slice(0, refPageSize)
  const hasNext = (allBranches?.length ?? 0) > refPageSize
  return (
    <div className="space-y-2">
      <RefListSearch search={search} onSearch={(value) => onNavigate({ refSearch: value || null, refPage: null })} />
      <QueryState data={branches} isLoading={query.isLoading} error={query.error} errorMessage={t('repositoryBrowser.branchesLoadError')} onRetry={() => void query.refetch()} isEmpty={(items) => items.length === 0} empty={{ title: search ? t('repositoryBrowser.noBranchMatches') : t('repositoryBrowser.noBranches') }}>
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
      <RefPagination page={page} hasNext={hasNext} onPage={(nextPage) => onNavigate({ refPage: nextPage > 1 ? String(nextPage) : null })} />
    </div>
  )
}

function CodeBrowser({ repo, gitRef, dirPath, filePath, onNavigate }: {
  repo: string
  gitRef: string
  dirPath: string
  filePath: string | null
  onNavigate: NavigateParams
}) {
  const { t } = useTranslation()
  const [draftRef, setDraftRef] = useState(gitRef === 'HEAD' ? '' : gitRef)
  const deferredRef = useDebouncedValue(draftRef.trim().replace(/^refs\/(?:heads|tags)\//, ''))
  const refsQuery = useRepositoryRefs(repo, { limit: refSuggestionLimit, search: deferredRef })
  const refs = refsQuery.data?.filter((ref) => ref.kind === 'branch' || ref.kind === 'tag').slice(0, refSuggestionLimit - 1) ?? []
  const refOptions = refs.map((ref) => ({ value: `refs/${ref.kind === 'branch' ? 'heads' : 'tags'}/${ref.name}`, label: `${ref.kind === 'branch' ? t('repositoryBrowser.branches') : t('repositoryBrowser.tags')} / ${ref.name}` }))

  useEffect(() => setDraftRef(gitRef === 'HEAD' ? '' : gitRef), [gitRef])

  function applyRef(event: React.FormEvent) {
    event.preventDefault()
    const value = draftRef.trim()
    onNavigate({ ref: value && value !== 'HEAD' ? value : null, dir: null, file: null })
  }

  return (
    <div className="space-y-3">
      <form className="flex min-w-0 flex-wrap items-end gap-2" role="search" aria-label={t('repositoryBrowser.refPicker')} onSubmit={applyRef}>
        <div className="min-w-0 flex-1 space-y-1.5 sm:max-w-md">
        <label htmlFor="repository-ref" className="text-sm font-medium">{t('repositoryBrowser.gitRef')}</label>
        <Input
          id="repository-ref"
          type="search"
          list="repository-ref-options"
          className="h-10 font-mono"
          placeholder={t('repositoryBrowser.defaultRef')}
          value={draftRef}
          onChange={(event) => setDraftRef(event.target.value)}
        />
        <datalist id="repository-ref-options">
          <option value="HEAD">{t('repositoryBrowser.defaultRef')}</option>
          {refOptions.map((option) => <option key={option.value} value={option.value} label={option.label} />)}
        </datalist>
        </div>
        <Button type="submit" variant="outline" size="sm" className="h-10 w-10 p-0" aria-label={t('repositoryBrowser.applyRef')} title={t('repositoryBrowser.applyRef')}>
          <Check className="h-4 w-4" aria-hidden />
        </Button>
      </form>
      {refsQuery.isLoading && <p role="status" className="text-xs text-text-muted">{t('repositoryBrowser.loadingRefs')}</p>}
      {(refsQuery.data?.length ?? 0) > refSuggestionLimit - 1 && <p role="status" className="text-xs text-text-muted">{t('repositoryBrowser.refSuggestionsLimited')}</p>}
      {Boolean(refsQuery.error) && (
        <div role="alert" className="flex flex-wrap items-center gap-3 rounded-md border border-status-failed/40 p-3 text-sm">
          <span>{t('repositoryBrowser.refsUnavailable')}</span>
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

function TagsList({ repo, page, search, onNavigate, onSelect }: { repo: string; page: number; search: string; onNavigate: NavigateParams; onSelect: (tag: { name: string }) => void }) {
  const { t } = useTranslation()
  const query = useRepositoryTags(repo, { limit: refPageSize + 1, offset: (page - 1) * refPageSize, search })
  const tags = query.data?.slice(0, refPageSize)
  const hasNext = (query.data?.length ?? 0) > refPageSize
  return (
    <div className="space-y-2">
      <RefListSearch search={search} onSearch={(value) => onNavigate({ refSearch: value || null, refPage: null })} />
      <QueryState data={tags} isLoading={query.isLoading} error={query.error} errorMessage={t('repositoryBrowser.tagsLoadError')} onRetry={() => void query.refetch()} isEmpty={(items) => items.length === 0} empty={{ title: search ? t('repositoryBrowser.noTagMatches') : t('repositoryBrowser.noTags') }}>
        {(items) => (
          <ul className="divide-y divide-border rounded-md border border-border">
            {items.map((tag) => (
              <li key={tag.name} className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1 px-3 py-2 text-sm">
                <button type="button" className="flex min-h-10 min-w-0 flex-1 items-center gap-2 text-left font-medium hover:text-accent" onClick={() => onSelect(tag)}>
                  <Tag className="h-4 w-4 shrink-0 text-accent" aria-hidden />
                  <span className="break-all">{tag.name}</span>
                </button>
                <code className="text-xs text-text-muted">{tag.sha.slice(0, 7)}</code>
                {tag.message && <span className="w-full min-w-0 break-words text-xs text-text-muted sm:w-auto sm:max-w-[40%] sm:truncate">{tag.message}</span>}
              </li>
            ))}
          </ul>
        )}
      </QueryState>
      <RefPagination page={page} hasNext={hasNext} onPage={(nextPage) => onNavigate({ refPage: nextPage > 1 ? String(nextPage) : null })} />
    </div>
  )
}

function ReleasesList({ repo }: { repo: string }) {
  const { t, i18n } = useTranslation()
  const releasesQuery = useReleases(repo)
  const createRelease = useCreateRelease(repo)
  const deleteRelease = useDeleteRelease(repo)
  const [open, setOpen] = useState(false)
  const [editingTag, setEditingTag] = useState<string | null>(null)
  const [pendingDelete, setPendingDelete] = useState<string | null>(null)
  const [tagError, setTagError] = useState<string | null>(null)
  const [form, setForm] = useState({ tag_name: '', name: '', description: '', prerelease: false })
  const deferredTag = useDebouncedValue(form.tag_name.trim())
  const tagsQuery = useRepositoryTags(repo, { limit: refSuggestionLimit, search: deferredTag })

  function openCreate() {
    setEditingTag(null)
    setTagError(null)
    setForm({ tag_name: '', name: '', description: '', prerelease: false })
    setOpen(true)
  }

  function openEdit(release: Release) {
    setEditingTag(release.tag_name)
    setTagError(null)
    setForm({ tag_name: release.tag_name, name: release.name, description: release.description, prerelease: release.prerelease })
    setOpen(true)
  }

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    if (!editingTag && (!releasesQuery.data || releasesQuery.error)) {
      setTagError(t('releases.listUnavailable'))
      return
    }
    const tagName = form.tag_name.trim()
    if (!tagName) {
      setTagError(t('releases.tagRequired'))
      return
    }
    if (!editingTag && releasesQuery.data?.some((release) => release.tag_name === tagName)) {
      setTagError(t('releases.alreadyExists'))
      return
    }
    setTagError(null)
    createRelease.mutate(
      { tag_name: tagName, name: form.name.trim() || tagName, description: form.description.trim(), prerelease: form.prerelease },
      {
        onSuccess: () => {
          toast.success(t(editingTag ? 'releases.updated' : 'releases.created'))
          setOpen(false)
        },
        onError: (err) => toast.error(err.message),
      },
    )
  }

  return (
    <div className="space-y-3">
      <div className="flex justify-end">
        <Button type="button" className="min-h-10" disabled={!releasesQuery.data || Boolean(releasesQuery.error)} onClick={openCreate}><Plus className="h-4 w-4" aria-hidden />{t('releases.create')}</Button>
      </div>
      <QueryState data={releasesQuery.data} isLoading={releasesQuery.isLoading} error={releasesQuery.error} errorMessage={t('repositoryBrowser.releasesLoadError')} onRetry={() => void releasesQuery.refetch()} isEmpty={(releases) => releases.length === 0} empty={{ title: t('releases.none') }}>
        {(releases) => <ul className="divide-y divide-border rounded-md border border-border">
          {releases.map((rel) => (
            <li key={rel.id} className="flex min-w-0 flex-wrap items-start gap-x-3 gap-y-1 px-3 py-3 text-sm sm:flex-nowrap">
              <Package className="mt-1 h-4 w-4 shrink-0 text-accent" aria-hidden />
              <div className="min-w-0 flex-1">
                <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
                  <span className="min-w-0 break-words font-medium">{rel.name}</span>
                  {rel.prerelease && <span className="rounded-sm bg-warning/15 px-1.5 py-0.5 text-xs text-text-primary">{t('releases.prerelease')}</span>}
                </div>
                <div className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-text-muted">
                  <span className="flex min-w-0 items-center gap-1 break-all"><Tag className="h-3 w-3 shrink-0" aria-hidden />{rel.tag_name}</span>
                  <time dateTime={rel.created_at}>{formatDate(rel.created_at, i18n.language)}</time>
                </div>
                {rel.description && <p className="mt-2 whitespace-pre-wrap break-words text-sm text-text-secondary">{rel.description}</p>}
              </div>
              <div className="flex w-full shrink-0 items-center justify-end gap-1 sm:w-auto">
                <button type="button" className="inline-flex h-10 w-10 items-center justify-center rounded-md text-text-secondary hover:bg-surface-raised hover:text-text-primary focus-visible:outline-2 focus-visible:outline-accent" aria-label={`${t('releases.edit')} ${rel.tag_name}`} title={`${t('releases.edit')} ${rel.tag_name}`} onClick={() => openEdit(rel)}>
                  <Pencil className="h-4 w-4" aria-hidden />
                </button>
                <button type="button" className="inline-flex h-10 w-10 items-center justify-center rounded-md text-danger hover:bg-surface-raised focus-visible:outline-2 focus-visible:outline-accent disabled:opacity-50" aria-label={`${t('releases.deleteAction')} ${rel.tag_name}`} title={`${t('releases.deleteAction')} ${rel.tag_name}`} disabled={deleteRelease.isPending} onClick={() => setPendingDelete(rel.tag_name)}>
                  <Trash2 className="h-4 w-4" aria-hidden />
                </button>
              </div>
            </li>
          ))}
        </ul>}
      </QueryState>

      <Dialog open={open} onOpenChange={(next) => { if (!createRelease.isPending) setOpen(next) }}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t(editingTag ? 'releases.edit' : 'releases.create')}</DialogTitle>
          </DialogHeader>
          <form onSubmit={handleSubmit} aria-label={t(editingTag ? 'releases.edit' : 'releases.create')} className="grid gap-3">
            <div className="space-y-1.5">
              <Label htmlFor="rel-tag">{t('releases.tag')}</Label>
              <Input id="rel-tag" value={form.tag_name} onChange={(e) => { setForm({ ...form, tag_name: e.target.value }); setTagError(null) }} list={editingTag ? undefined : 'release-tags'} readOnly={!!editingTag} aria-invalid={!!tagError} aria-describedby={tagError ? 'rel-tag-error' : undefined} placeholder="v1.0.0" required />
              <datalist id="release-tags">{tagsQuery.data?.slice(0, refSuggestionLimit - 1).map((tag) => <option key={tag.name} value={tag.name} />)}</datalist>
              {tagError && <p id="rel-tag-error" role="alert" className="text-xs text-danger">{tagError}</p>}
              {tagsQuery.isLoading && <p role="status" className="text-xs text-text-muted">{t('repositoryBrowser.loadingRefs')}</p>}
              {(tagsQuery.data?.length ?? 0) > refSuggestionLimit - 1 && <p role="status" className="text-xs text-text-muted">{t('repositoryBrowser.refSuggestionsLimited')}</p>}
              {Boolean(tagsQuery.error) && <div role="alert" className="flex flex-wrap items-center gap-2 text-xs text-text-secondary"><span>{t('repositoryBrowser.refsUnavailable')}</span><Button type="button" size="sm" variant="outline" className="min-h-10" onClick={() => void tagsQuery.refetch()}>{t('common.retry')}</Button></div>}
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="rel-name">{t('releases.name')}</Label>
              <Input id="rel-name" value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="rel-desc">{t('releases.description')}</Label>
              <Textarea id="rel-desc" rows={4} value={form.description} onChange={(e) => setForm({ ...form, description: e.target.value })} />
            </div>
            <div className="flex items-center gap-2">
              <input
                id="rel-pre"
                type="checkbox"
                className="h-4 w-4 rounded border-border bg-surface"
                checked={form.prerelease}
                onChange={(e) => setForm({ ...form, prerelease: e.target.checked })}
              />
              <Label htmlFor="rel-pre">{t('releases.prerelease')}</Label>
            </div>
            <DialogFooter>
              <Button type="button" variant="outline" disabled={createRelease.isPending} onClick={() => setOpen(false)}>{t('common.cancel')}</Button>
              <Button type="submit" disabled={createRelease.isPending}>{createRelease.isPending ? t('common.saving') : t(editingTag ? 'common.save' : 'common.create')}</Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>

      <ConfirmDialog
        open={pendingDelete !== null}
        title={pendingDelete ? `${t('releases.deleteTitle')} «${pendingDelete}»?` : ''}
        description={t('releases.deleteConfirm')}
        pending={deleteRelease.isPending}
        closeOnConfirm={false}
        onCancel={() => setPendingDelete(null)}
        onConfirm={() => {
          if (!pendingDelete) return
          deleteRelease.mutate(pendingDelete, {
            onSuccess: () => { setPendingDelete(null); toast.success(t('releases.deleted')) },
            onError: (error) => toast.error(error.message),
          })
        }}
      />
    </div>
  )
}
