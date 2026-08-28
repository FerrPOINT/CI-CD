import { useState } from 'react'
import { Link, useParams } from 'react-router'
import { useTranslation } from 'react-i18next'
import { GitBranch, GitCompareArrows, GitPullRequest, ChevronRight, GitCommitHorizontal, Folder, FileText, Tag, Package, ArrowLeft } from 'lucide-react'
import { toast } from 'sonner'
import { useRepositoryCommits, useRepositoryRefs, useRepositoryTree, useRepositoryBlob, useRepositoryTags, useReleases, useCreateRelease, useDeleteRelease } from '@/api/hooks'
import {
  Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter,
} from '@/shared/ui/dialog'
import { Input } from '@/shared/ui/input'
import { Label } from '@/shared/ui/label'
import { Textarea } from '@/shared/ui/textarea'
import {
  AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription,
  AlertDialogFooter, AlertDialogHeader, AlertDialogTitle,
} from '@/shared/ui/alert-dialog'
import { UserAvatar } from '@/shared/ui/user-avatar'
import { Button } from '@/shared/ui/button'
import { Card } from '@/shared/ui/card'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/shared/ui/tabs'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/shared/ui/table'

function formatDate(value: string, locale: string): string {
  const date = new Date(value)
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString(locale)
}

export function RepositoryBrowserPage() {
  const { t, i18n } = useTranslation()
  const { repo } = useParams<{ repo: string }>()
  const [tab, setTab] = useState('commits')
  const { data: refs = [], isLoading: refsLoading, isError: refsError, error: refsErrorValue } = useRepositoryRefs(repo)
  const { data: commits = [], isLoading: commitsLoading, isError: commitsError, error: commitsErrorValue } = useRepositoryCommits(repo)

  if (!repo) return <p className="text-sm text-text-muted">{t('repositories.notFound')}</p>

  const errorValue = refsError ? refsErrorValue : commitsError ? commitsErrorValue : null
  const error = errorValue instanceof Error ? errorValue : null

  return (
    <div className="space-y-6">
      <div className="flex flex-col gap-4 sm:flex-row sm:items-end sm:justify-between">
        <div>
          <div className="flex items-center gap-2 text-sm text-text-muted">
            <Link to="/repositories" className="hover:text-text-primary">{t('navigation.repositories')}</Link>
            <ChevronRight className="h-3 w-3" />
            <span>{repo}</span>
          </div>
          <div className="mt-2 flex items-center gap-3">
            <GitBranch className="h-6 w-6 text-accent" />
            <h1 className="text-2xl font-bold">{repo}</h1>
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

      {error ? (
        <Card className="p-6 text-sm text-danger">
          {t('repositoryBrowser.loadError')}: {error.message}
        </Card>
      ) : (
        <Tabs value={tab} onValueChange={setTab}>
          <TabsList className="h-auto w-full justify-start overflow-x-auto sm:w-auto">
            <TabsTrigger value="commits">{t('repositoryBrowser.commits')}</TabsTrigger>
            <TabsTrigger value="branches">{t('repositoryBrowser.branches')}</TabsTrigger>
            <TabsTrigger value="code">{t('repositoryBrowser.code', 'Код')}</TabsTrigger>
            <TabsTrigger value="tags">{t('repositoryBrowser.tags', 'Теги')}</TabsTrigger>
            <TabsTrigger value="releases">{t('repositoryBrowser.releases', 'Релизы')}</TabsTrigger>
            <TabsTrigger value="compare" asChild>
              <Link to={`/repositories/${encodeURIComponent(repo)}/compare`}>{t('repositoryBrowser.compare')}</Link>
            </TabsTrigger>
            <TabsTrigger value="pulls" asChild>
              <Link to={`/repositories/${encodeURIComponent(repo)}/pulls`}>{t('repositoryBrowser.pullRequests')}</Link>
            </TabsTrigger>
          </TabsList>

          <TabsContent value="commits" className="mt-4">
            {commitsLoading ? (
              <p className="text-sm text-text-muted">{t('common.loading')}</p>
            ) : commits.length === 0 ? (
              <Card className="p-8 text-center text-text-muted">{t('repositoryBrowser.noCommits')}</Card>
            ) : (
              <Card className="overflow-hidden">
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>{t('repositoryBrowser.sha')}</TableHead>
                      <TableHead>{t('repositoryBrowser.message')}</TableHead>
                      <TableHead>{t('repositoryBrowser.author')}</TableHead>
                      <TableHead>{t('repositoryBrowser.date')}</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {commits.map((commit) => (
                      <TableRow key={commit.sha}>
                        <TableCell><code className="rounded bg-surface-raised px-1.5 py-0.5 text-xs text-accent">{commit.short_sha}</code></TableCell>
                        <TableCell className="min-w-64 font-medium">{commit.message}</TableCell>
                        <TableCell className="text-text-secondary">{commit.author}</TableCell>
                        <TableCell className="whitespace-nowrap text-xs text-text-muted">{formatDate(commit.date, i18n.language)}</TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              </Card>
            )}
          </TabsContent>

          <TabsContent value="code" className="mt-4">
            <CodeBrowser repo={repo} />
          </TabsContent>
          <TabsContent value="tags" className="mt-4">
            <TagsList repo={repo} />
          </TabsContent>
          <TabsContent value="releases" className="mt-4">
            <ReleasesList repo={repo} />
          </TabsContent>
          <TabsContent value="branches" className="mt-4">
            {refsLoading ? (
              <p className="text-sm text-text-muted">{t('common.loading')}</p>
            ) : refs.length === 0 ? (
              <Card className="p-8 text-center text-text-muted">{t('repositoryBrowser.noBranches')}</Card>
            ) : (
              <Card className="overflow-hidden">
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>{t('repositoryBrowser.branch')}</TableHead>
                      <TableHead>{t('repositoryBrowser.sha')}</TableHead>
                      <TableHead>{t('repositoryBrowser.target')}</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {refs.map((ref) => (
                      <TableRow key={`${ref.name}-${ref.sha}`}>
                        <TableCell className="font-medium"><GitBranch className="mr-2 inline h-4 w-4 text-accent" />{ref.name}</TableCell>
                        <TableCell><code className="rounded bg-surface-raised px-1.5 py-0.5 text-xs">{ref.sha.slice(0, 7)}</code></TableCell>
                        <TableCell className="max-w-96 truncate text-text-muted">{ref.target || '-'}</TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              </Card>
            )}
          </TabsContent>
        </Tabs>
      )}

      <Card className="flex items-center gap-3 border-dashed p-4 text-sm text-text-muted">
        <GitCommitHorizontal className="h-4 w-4 text-accent" />
        {t('repositoryBrowser.tip')}
      </Card>
    </div>
  )
}


function CodeBrowser({ repo }: { repo: string }) {
  const { t } = useTranslation()
  const [dirPath, setDirPath] = useState<string>('')
  const [filePath, setFilePath] = useState<string | null>(null)
  const gitRef = 'HEAD'
  const { data: entries = [], isLoading, isError, error } = useRepositoryTree(repo, gitRef, dirPath || undefined)
  const { data: blob } = useRepositoryBlob(repo, gitRef, filePath ?? '')

  if (isError) return <Card className="p-8 text-center text-text-muted">{error instanceof Error ? error.message : t('common.error')}</Card>

  if (filePath) {
    return (
      <Card className="overflow-hidden">
        <div className="flex items-center gap-2 border-b border-border px-4 py-2.5 text-sm">
          <Button variant="ghost" size="sm" onClick={() => setFilePath(null)}>
            <ArrowLeft className="h-4 w-4" />
          </Button>
          <FileText className="h-4 w-4 text-accent" />
          <span className="font-medium">{filePath}</span>
          {blob && <span className="ml-auto text-xs text-text-muted">{blob.size} B · {blob.sha.slice(0, 7)}</span>}
        </div>
        <pre className="max-h-[70vh] overflow-auto p-4 text-xs leading-relaxed">
          <code>{blob?.binary ? t('repositoryBrowser.binaryFile', 'Бинарный файл — предпросмотр недоступен') : blob?.content || t('common.loading')}</code>
        </pre>
      </Card>
    )
  }

  const crumbs = dirPath ? dirPath.split('/') : []
  return (
    <Card className="overflow-hidden">
      <div className="flex items-center gap-2 border-b border-border px-4 py-2.5 text-sm">
        <Folder className="h-4 w-4 text-accent" />
        <button className="hover:underline" onClick={() => setDirPath('')}>/</button>
        {crumbs.map((part, i) => (
          <span key={i} className="flex items-center gap-1">
            <button
              className="hover:underline"
              onClick={() => setDirPath(crumbs.slice(0, i + 1).join('/'))}
            >
              {part}
            </button>
            {i < crumbs.length - 1 && <span className="text-text-muted">/</span>}
          </span>
        ))}
      </div>
      {isLoading ? (
        <p className="p-4 text-sm text-text-muted">{t('common.loading')}</p>
      ) : entries.length === 0 ? (
        <p className="p-4 text-sm text-text-muted">{t('repositoryBrowser.emptyTree', 'Пусто')}</p>
      ) : (
        <ul className="divide-y divide-border">
          {[...entries].sort((a, b) => (a.kind === b.kind ? a.name.localeCompare(b.name) : a.kind === 'tree' ? -1 : 1)).map((entry) => (
            <li key={entry.path} className="flex items-center gap-3 px-4 py-2 text-sm hover:bg-surface-raised">
              {entry.kind === 'tree' ? (
                <button className="flex flex-1 items-center gap-3 text-left" onClick={() => setDirPath(entry.path)}>
                  <Folder className="h-4 w-4 text-accent" />
                  <span className="font-medium">{entry.name}</span>
                </button>
              ) : (
                <button className="flex flex-1 items-center gap-3 text-left" onClick={() => setFilePath(entry.path)}>
                  <FileText className="h-4 w-4 text-text-muted" />
                  <span>{entry.name}</span>
                </button>
              )}
              <code className="rounded bg-surface-raised px-1.5 py-0.5 text-xs text-text-muted">{entry.sha.slice(0, 7)}</code>
              {entry.size != null && <span className="w-20 text-right text-xs text-text-muted">{entry.size} B</span>}
            </li>
          ))}
        </ul>
      )}
    </Card>
  )
}

function TagsList({ repo }: { repo: string }) {
  const { t } = useTranslation()
  const { data: tags = [], isLoading } = useRepositoryTags(repo)
  if (isLoading) return <p className="text-sm text-text-muted">{t('common.loading')}</p>
  if (tags.length === 0) return <Card className="p-8 text-center text-text-muted">{t('repositoryBrowser.noTags', 'Тегов нет')}</Card>
  return (
    <Card className="overflow-hidden">
      <ul className="divide-y divide-border">
        {tags.map((tag) => (
          <li key={tag.name} className="flex items-center gap-3 px-4 py-2.5 text-sm">
            <Tag className="h-4 w-4 text-accent" />
            <span className="font-medium">{tag.name}</span>
            <code className="rounded bg-surface-raised px-1.5 py-0.5 text-xs text-text-muted">{tag.sha.slice(0, 7)}</code>
            <span className="ml-auto max-w-96 truncate text-text-muted">{tag.message}</span>
          </li>
        ))}
      </ul>
    </Card>
  )
}

function ReleasesList({ repo }: { repo: string }) {
  const { t } = useTranslation()
  const { data: releases = [], isLoading } = useReleases(repo)
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
      {isLoading ? (
        <p className="text-sm text-text-muted">{t('common.loading')}</p>
      ) : releases.length === 0 ? (
        <Card className="p-8 text-center text-text-muted">{t('releases.none', 'Релизов нет')}</Card>
      ) : (
        <ul className="grid gap-3">
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
        </ul>
      )}

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
