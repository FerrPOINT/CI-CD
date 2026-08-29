import { lazy, Suspense } from 'react'
import { createBrowserRouter, Navigate } from 'react-router'
import { AppShell } from '@/widgets/app-shell'

const DashboardPage = lazy(() => import('@/pages/dashboard').then(m => ({ default: m.DashboardPage })))
const ProjectsPage = lazy(() => import('@/pages/projects').then(m => ({ default: m.ProjectsPage })))
const PipelinesPage = lazy(() => import('@/pages/pipelines').then(m => ({ default: m.PipelinesPage })))
const PipelineDetailPage = lazy(() => import('@/pages/pipeline-detail').then(m => ({ default: m.PipelineDetailPage })))
const RepositoriesPage = lazy(() => import('@/pages/repositories').then(m => ({ default: m.RepositoriesPage })))
const RepositoryBrowserPage = lazy(() => import('@/pages/repository-browser').then(m => ({ default: m.RepositoryBrowserPage })))
const ComparePage = lazy(() => import('@/pages/compare').then(m => ({ default: m.ComparePage })))
const PullRequestsPage = lazy(() => import('@/pages/pull-requests').then(m => ({ default: m.PullRequestsPage })))
const PullRequestDetailPage = lazy(() => import('@/pages/pull-request-detail').then(m => ({ default: m.PullRequestDetailPage })))
const SettingsPage = lazy(() => import('@/pages/settings').then(m => ({ default: m.SettingsPage })))
const LoginPage = lazy(() => import('@/pages/login').then(m => ({ default: m.LoginPage })))
const RunnersPage = lazy(() => import('@/pages/runners').then(m => ({ default: m.RunnersPage })))
const SecretsPage = lazy(() => import('@/pages/secrets').then(m => ({ default: m.SecretsPage })))
const ArtifactsPage = lazy(() => import('@/pages/artifacts').then(m => ({ default: m.ArtifactsPage })))
const EnvironmentsPage = lazy(() => import('@/pages/environments').then(m => ({ default: m.EnvironmentsPage })))
const SchedulesPage = lazy(() => import('@/pages/schedules').then(m => ({ default: m.SchedulesPage })))
const WebhooksPage = lazy(() => import('@/pages/webhooks').then(m => ({ default: m.WebhooksPage })))
const ReportsPage = lazy(() => import('@/pages/reports').then(m => ({ default: m.ReportsPage })))
const AuditLogPage = lazy(() => import('@/pages/audit-log').then(m => ({ default: m.AuditLogPage })))
const UsersPage = lazy(() => import('@/pages/users').then(m => ({ default: m.UsersPage })))

function PageLoader() {
  return <div className="flex items-center justify-center py-16 text-sm text-text-muted">Loading…</div>
}

const withSuspense = (el: React.ReactElement) => <Suspense fallback={<PageLoader />}>{el}</Suspense>

export const router = createBrowserRouter([
  {
    element: <AppShell />,
    children: [
      { path: '/', element: withSuspense(<DashboardPage />) },
      { path: '/projects', element: withSuspense(<ProjectsPage />) },
      { path: '/projects/:projectId/pipelines', element: withSuspense(<PipelinesPage />) },
      { path: '/pipelines/:pipelineId', element: withSuspense(<PipelineDetailPage />) },
      { path: '/repositories', element: withSuspense(<RepositoriesPage />) },
      { path: '/repositories/:repo', element: withSuspense(<RepositoryBrowserPage />) },
      { path: '/repositories/:repo/compare', element: withSuspense(<ComparePage />) },
      { path: '/repositories/:repo/pulls', element: withSuspense(<PullRequestsPage />) },
      { path: '/repositories/:repo/pulls/:number', element: withSuspense(<PullRequestDetailPage />) },
      { path: '/settings', element: withSuspense(<SettingsPage />) },
      { path: '/runners', element: withSuspense(<RunnersPage />) },
      { path: '/projects/:projectId/secrets', element: withSuspense(<SecretsPage />) },
      { path: '/jobs/:jobId/artifacts', element: withSuspense(<ArtifactsPage />) },
      { path: '/projects/:projectId/environments', element: withSuspense(<EnvironmentsPage />) },
      { path: '/projects/:projectId/schedules', element: withSuspense(<SchedulesPage />) },
      { path: '/projects/:projectId/webhooks', element: withSuspense(<WebhooksPage />) },
      { path: '/projects/:projectId/reports', element: withSuspense(<ReportsPage />) },
      { path: '/audit-log', element: withSuspense(<AuditLogPage />) },
      { path: '/users', element: withSuspense(<UsersPage />) },
    ],
  },
  { path: '/login', element: withSuspense(<LoginPage />) },
  { path: '*', element: <Navigate to="/" replace /> },
])
