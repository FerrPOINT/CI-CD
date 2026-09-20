import { useState, type FormEvent } from 'react'
import { Link, useSearchParams } from 'react-router'
import { useTranslation } from 'react-i18next'
import { Check, Copy, GitFork, Plus, Search, Trash2 } from 'lucide-react'
import { toast } from 'sonner'
import { useRepositories, useCreateRepository, useDeleteRepository, useProjects } from '@/api/hooks'
import type { Project, Repository } from '@/api/types'
import { Button, Input, Label } from '@sdlc/ui/ui'
import { ConfirmDialog } from '@/shared/ui/confirm-dialog'
import { QueryState } from '@/shared/ui/query-state'

const pageSize = 20

function buildCloneUrl(name: string): string {
  return `${window.location.origin}/git/${name}.git`
}

function projectNameForRepository(name: string, projects: Project[]): string | null {
  const suffix = `/${name}.git`
  const sshSuffix = `:${name}.git`
  return projects.find((project) =>
    project.repository_url.endsWith(suffix) || project.repository_url.endsWith(sshSuffix),
  )?.name ?? null
}

export function RepositoriesPage() {
  const { t } = useTranslation()
  const [searchParams, setSearchParams] = useSearchParams()
  const projectFilter = searchParams.get('project') ?? 'all'
  const { data: projects = [], isLoading: projectsLoading, error: projectsError, refetch: refetchProjects } = useProjects()
  const { data: repositories = [], isLoading, error: listError, refetch } = useRepositories()
  const createRepository = useCreateRepository()
  const deleteRepository = useDeleteRepository()
  const [showForm, setShowForm] = useState(false)
  const [formName, setFormName] = useState('')
  const [pendingDelete, setPendingDelete] = useState<Repository | null>(null)
  const [search, setSearch] = useState('')
  const [page, setPage] = useState(1)
  const [copied, setCopied] = useState<string | null>(null)

  const normalizedSearch = search.trim().toLocaleLowerCase()
  const filteredRepositories = repositories
    .filter((repository) => {
      const projectName = projectNameForRepository(repository.name, projects)
      return (projectFilter === 'all' || projectName === projectFilter) &&
        repository.name.toLocaleLowerCase().includes(normalizedSearch)
    })
    .sort((a, b) => a.name.localeCompare(b.name))
  const totalPages = Math.max(1, Math.ceil(filteredRepositories.length / pageSize))
  const currentPage = Math.min(page, totalPages)
  const visibleRepositories = filteredRepositories.slice((currentPage - 1) * pageSize, currentPage * pageSize)

  function setProjectFilter(value: string) {
    const next = new URLSearchParams(searchParams)
    if (value === 'all') next.delete('project')
    else next.set('project', value)
    setSearchParams(next, { replace: true })
    setPage(1)
  }

  function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const name = formName.trim()
    if (!name) return
    createRepository.mutate({ name }, {
      onSuccess: () => {
        setShowForm(false)
        setFormName('')
        toast.success(t('repositories.created'))
      },
      onError: (error) => toast.error(error.message),
    })
  }

  function copyUrl(name: string) {
    if (!navigator.clipboard?.writeText) {
      toast.error(t('repositories.copyError'))
      return
    }
    void navigator.clipboard.writeText(buildCloneUrl(name)).then(() => {
      setCopied(name)
      setTimeout(() => setCopied(null), 2000)
      toast.success(t('repositories.copySuccess'))
    }).catch(() => toast.error(t('repositories.copyError')))
  }

  return (
    <div className="space-y-5">
      <header className="flex flex-wrap items-center justify-between gap-3">
        <h1 className="text-2xl font-bold">{t('repositories.title')}</h1>
        <Button
          size="sm"
          className="min-h-10 sm:min-h-10"
          aria-expanded={showForm}
          disabled={createRepository.isPending}
          onClick={() => {
            if (showForm) setFormName('')
            setShowForm(!showForm)
          }}
        >
          <Plus className="h-4 w-4" aria-hidden />
          {t('repositories.create')}
        </Button>
      </header>

      {showForm && (
        <form onSubmit={handleSubmit} aria-label={t('repositories.create')} className="flex flex-wrap items-end gap-3 border-y border-border py-4">
          <div className="min-w-56 flex-1 space-y-1.5">
            <Label htmlFor="repo-name">{t('repositories.name')}</Label>
            <Input
              id="repo-name"
              className="min-h-10"
              required
              autoFocus
              placeholder="my-repo"
              value={formName}
              onChange={(event) => setFormName(event.target.value)}
            />
          </div>
          <div className="flex gap-2">
            <Button type="button" variant="outline" className="min-h-10 sm:min-h-10" disabled={createRepository.isPending} onClick={() => { setShowForm(false); setFormName('') }}>
              {t('common.cancel')}
            </Button>
            <Button type="submit" className="min-h-10 sm:min-h-10" disabled={createRepository.isPending || !formName.trim()}>
              {t('repositories.create')}
            </Button>
          </div>
        </form>
      )}

      {projectsError && projectFilter === 'all' && (
        <div role="alert" className="flex flex-wrap items-center gap-3 border-y border-border py-3 text-sm text-text-secondary">
          <span>{t('repositories.projectLoadError')}</span>
          <Button type="button" size="sm" variant="outline" className="min-h-10 sm:min-h-10" onClick={() => void refetchProjects()}>
            {t('common.retry')}
          </Button>
        </div>
      )}

      <QueryState
        data={repositories}
        isLoading={isLoading || (projectFilter !== 'all' && projectsLoading)}
        error={listError}
        errorMessage={t('repositories.listLoadError')}
        isEmpty={(list) => list.length === 0}
        empty={{ title: t('repositories.empty') }}
        onRetry={() => { void refetch(); if (projectFilter !== 'all') void refetchProjects() }}
      >
        {() => projectFilter !== 'all' && projectsError ? (
          <div role="alert" className="flex flex-wrap items-center gap-3 border-y border-border py-4 text-sm text-text-secondary">
            <span>{t('repositories.projectLoadError')}</span>
            <Button type="button" size="sm" variant="outline" className="min-h-10 sm:min-h-10" onClick={() => void refetchProjects()}>
              {t('common.retry')}
            </Button>
            <Button type="button" size="sm" variant="outline" className="min-h-10 sm:min-h-10" onClick={() => setProjectFilter('all')}>
              {t('repositories.allProjects')}
            </Button>
          </div>
        ) : (
          <section aria-label={t('repositories.title')} className="space-y-3">
            <div className="grid gap-3 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-end">
              <div className="relative">
                <Search className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-text-muted" aria-hidden />
                <Input
                  type="search"
                  aria-label={t('repositories.search')}
                  placeholder={t('repositories.search')}
                  className="min-h-10 pl-9"
                  value={search}
                  onChange={(event) => { setSearch(event.target.value); setPage(1) }}
                />
              </div>
              <div className="flex min-w-0 items-center gap-2">
                <Label htmlFor="project-filter" className="shrink-0">{t('repositories.projectFilter')}</Label>
                <select
                  id="project-filter"
                  value={projectFilter}
                  disabled={projectsLoading || !!projectsError}
                  onChange={(event) => setProjectFilter(event.target.value)}
                  className="min-h-10 min-w-0 flex-1 rounded-md border border-border bg-surface px-2 text-sm text-text-primary outline-none focus-visible:border-accent sm:w-44"
                >
                  <option value="all">{t('repositories.allProjects')}</option>
                  {projects.map((project) => <option key={project.id} value={project.name}>{project.name}</option>)}
                </select>
              </div>
            </div>
            <p className="text-xs text-text-muted">
              {t('repositories.shown', { count: visibleRepositories.length, total: filteredRepositories.length })}
            </p>
            {filteredRepositories.length === 0 ? (
              <p role="status" className="border-y border-border py-6 text-sm text-text-muted">{t('repositories.noMatches')}</p>
            ) : (
              <ul className="divide-y divide-border border-y border-border">
                {visibleRepositories.map((repository) => {
                  const projectName = projectNameForRepository(repository.name, projects)
                  const cloneUrl = buildCloneUrl(repository.name)
                  return (
                    <li key={repository.id} className="flex min-w-0 items-center gap-1">
                      <Link
                        to={`/repositories/${encodeURIComponent(repository.name)}`}
                        aria-label={t('repositories.openRepository', { name: repository.name })}
                        className="flex min-h-16 min-w-0 flex-1 items-center gap-3 py-2 hover:bg-surface-raised focus-visible:outline-2 focus-visible:outline-accent"
                      >
                        <GitFork className="h-4 w-4 shrink-0 text-accent" aria-hidden />
                        <span className="min-w-0 flex-1">
                          <span className="flex min-w-0 items-baseline gap-2">
                            <span className="min-w-0 flex-1 truncate text-sm font-medium" title={repository.name}>{repository.name}</span>
                            {projectName && <span className="max-w-[40%] truncate text-xs text-text-secondary" title={projectName}>{projectName}</span>}
                          </span>
                          <span className="block truncate font-mono text-xs text-text-muted" title={cloneUrl}>{cloneUrl}</span>
                        </span>
                        <time dateTime={repository.created_at} className="hidden shrink-0 text-xs text-text-muted lg:block">
                          {new Date(repository.created_at).toLocaleDateString()}
                        </time>
                      </Link>
                      <button
                        type="button"
                        aria-label={t('repositories.copyUrlFor', { name: repository.name })}
                        title={t('repositories.copyUrlFor', { name: repository.name })}
                        className="inline-flex h-10 w-10 shrink-0 items-center justify-center rounded-md text-text-secondary hover:bg-surface-raised hover:text-text-primary focus-visible:outline-2 focus-visible:outline-accent"
                        onClick={() => copyUrl(repository.name)}
                      >
                        {copied === repository.name ? <Check className="h-4 w-4 text-success" aria-hidden /> : <Copy className="h-4 w-4" aria-hidden />}
                      </button>
                      <button
                        type="button"
                        aria-label={t('repositories.deleteRepository', { name: repository.name })}
                        title={t('repositories.deleteRepository', { name: repository.name })}
                        disabled={deleteRepository.isPending}
                        className="inline-flex h-10 w-10 shrink-0 items-center justify-center rounded-md text-danger hover:bg-surface-raised focus-visible:outline-2 focus-visible:outline-accent disabled:opacity-50"
                        onClick={() => setPendingDelete(repository)}
                      >
                        <Trash2 className="h-4 w-4" aria-hidden />
                      </button>
                    </li>
                  )
                })}
              </ul>
            )}
            {filteredRepositories.length > pageSize && (
              <nav aria-label={t('repositories.pages')} className="flex items-center justify-end gap-2">
                <Button type="button" size="sm" variant="outline" className="min-h-10 sm:min-h-10" disabled={currentPage === 1} onClick={() => setPage(currentPage - 1)}>
                  {t('repositories.previous')}
                </Button>
                <span className="text-sm text-text-muted">{currentPage} / {totalPages}</span>
                <Button type="button" size="sm" variant="outline" className="min-h-10 sm:min-h-10" disabled={currentPage === totalPages} onClick={() => setPage(currentPage + 1)}>
                  {t('repositories.next')}
                </Button>
              </nav>
            )}
          </section>
        )}
      </QueryState>

      <ConfirmDialog
        open={pendingDelete !== null}
        title={pendingDelete ? `${t('repositories.deleteConfirm')} "${pendingDelete.name}"?` : ''}
        description={t('repositories.deleteWarning')}
        pending={deleteRepository.isPending}
        closeOnConfirm={false}
        onCancel={() => setPendingDelete(null)}
        onConfirm={() => {
          if (!pendingDelete) return
          deleteRepository.mutate(pendingDelete.name, {
            onSuccess: () => { setPendingDelete(null); toast.success(t('repositories.deleted')) },
            onError: (error) => toast.error(error.message),
          })
        }}
      />
    </div>
  )
}
