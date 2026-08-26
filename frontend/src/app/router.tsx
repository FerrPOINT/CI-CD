import { lazy, Suspense } from 'react'
import { createBrowserRouter, Navigate } from 'react-router'
import { AppShell } from '@/widgets/app-shell'

const DashboardPage = lazy(() => import('@/pages/dashboard').then(m => ({ default: m.DashboardPage })))
const ProjectsPage = lazy(() => import('@/pages/projects').then(m => ({ default: m.ProjectsPage })))
const PipelinesPage = lazy(() => import('@/pages/pipelines').then(m => ({ default: m.PipelinesPage })))
const PipelineDetailPage = lazy(() => import('@/pages/pipeline-detail').then(m => ({ default: m.PipelineDetailPage })))
const AdminPage = lazy(() => import('@/pages/admin').then(m => ({ default: m.AdminPage })))
const RepositoriesPage = lazy(() => import('@/pages/repositories').then(m => ({ default: m.RepositoriesPage })))
const SettingsPage = lazy(() => import('@/pages/settings').then(m => ({ default: m.SettingsPage })))
const LoginPage = lazy(() => import('@/pages/login').then(m => ({ default: m.LoginPage })))

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
      { path: '/admin', element: withSuspense(<AdminPage />) },
      { path: '/repositories', element: withSuspense(<RepositoriesPage />) },
      { path: '/settings', element: withSuspense(<SettingsPage />) },
    ],
  },
  { path: '/login', element: withSuspense(<LoginPage />) },
  { path: '*', element: <Navigate to="/" replace /> },
])
