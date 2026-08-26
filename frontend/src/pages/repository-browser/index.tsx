import { useState } from 'react'
import { Link, useParams } from 'react-router'
import { useTranslation } from 'react-i18next'
import { GitBranch, GitCompareArrows, GitPullRequest, ChevronRight, GitCommitHorizontal } from 'lucide-react'
import { useRepositoryCommits, useRepositoryRefs } from '@/api/hooks'
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
