# Routing — Forge CI/CD

## 1. Overview

Все frontend-роуты объявлены в `frontend/src/app/router.tsx`. Используется `react-router` 8.1.0 с `createBrowserRouter`. Все страницы lazy-loaded через `React.lazy` + `Suspense`.

## 2. Route Groups

| Group | Auth | Layout | Описание |
|-------|------|--------|----------|
| App | без auth (текущая версия) | `AppShell` | Основные страницы приложения |
| Public | без auth | — | Страница входа |
| Catch-all | — | redirect | 404 → `/` |

> **Примечание:** аутентификация не реализована в текущей версии (Phase 0). `RequireAuth` wrapper будет добавлен в Phase 1 (см. `docs/SECURITY.md`).

## 3. App Routes (AppShell)

Все app-роуты обёрнуты в `<AppShell />` layout (header + sidebar + `<Outlet />`).

| Route | Page | Lazy Import | Описание |
|-------|------|-------------|----------|
| `/` | `DashboardPage` | `@/pages/dashboard` | Главная / дашборд |
| `/projects` | `ProjectsPage` | `@/pages/projects` | Список проектов + создание |
| `/projects/:projectId/pipelines` | `PipelinesPage` | `@/pages/pipelines` | Пайплайны проекта + запуск |
| `/pipelines/:pipelineId` | `PipelineDetailPage` | `@/pages/pipeline-detail` | Детали пайплайна: стадии, задачи, логи |
| `/admin` | `AdminPage` | `@/pages/admin` | Админ-панель (планируется) |

## 4. Public Routes

| Route | Page | Lazy Import | Описание |
|-------|------|-------------|----------|
| `/login` | `LoginPage` | `@/pages/login` | Страница входа (планируется, Phase 1) |

## 5. Catch-all

| Route | Behavior |
|-------|----------|
| `*` | `<Navigate to="/" replace />` — редирект на дашборд |

Любой неописанный путь перенаправляет на `/` с заменой истории.

## 6. Route Configuration

```tsx
// frontend/src/app/router.tsx
import { lazy, Suspense } from 'react'
import { createBrowserRouter, Navigate } from 'react-router'
import { AppShell } from '@/widgets/app-shell'

const DashboardPage = lazy(() => import('@/pages/dashboard').then(m => ({ default: m.DashboardPage })))
const ProjectsPage = lazy(() => import('@/pages/projects').then(m => ({ default: m.ProjectsPage })))
const PipelinesPage = lazy(() => import('@/pages/pipelines').then(m => ({ default: m.PipelinesPage })))
const PipelineDetailPage = lazy(() => import('@/pages/pipeline-detail').then(m => ({ default: m.PipelineDetailPage })))
const AdminPage = lazy(() => import('@/pages/admin').then(m => ({ default: m.AdminPage })))
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
    ],
  },
  { path: '/login', element: withSuspense(<LoginPage />) },
  { path: '*', element: <Navigate to="/" replace /> },
])
```

## 7. Lazy Loading

### 7.1 Принцип

Все страницы загружаются через `React.lazy` — route-level code splitting. Каждый роут — отдельный chunk.

### 7.2 Suspense

Каждый lazy-элемент обёрнут в `<Suspense>` с `PageLoader` fallback:

```tsx
function PageLoader() {
  return (
    <div className="flex items-center justify-center py-16 text-sm text-text-muted">
      Loading…
    </div>
  )
}
```

### 7.3 Named exports

Страницы используют named exports, поэтому `lazy` требует `.then(m => ({ default: m.PageName }))`:

```tsx
// pages/dashboard/index.ts
export function DashboardPage() { ... }
```

```tsx
// router.tsx
const DashboardPage = lazy(() => import('@/pages/dashboard').then(m => ({ default: m.DashboardPage })))
```

## 8. Route Tree (визуально)

```
/                         ── AppShell ── DashboardPage
├── /projects             ── AppShell ── ProjectsPage
├── /projects/:projectId/
│   └── pipelines         ── AppShell ── PipelinesPage
├── /pipelines/:pipelineId ─ AppShell ── PipelineDetailPage
├── /admin                ── AppShell ── AdminPage
├── /login                ─────────────── LoginPage (public)
└── *                     ─────────────── Navigate to /
```

## 9. Navigation

### 9.1 В компонентах

```tsx
import { useNavigate, Link } from 'react-router'

// Link для декларативной навигации
<Link to="/projects">Проекты</Link>

// useNavigate для программной
const navigate = useNavigate()
navigate('/pipelines/123')
```

### 9.2 Параметры роутов

```tsx
import { useParams } from 'react-router'

function PipelinesPage() {
  const { projectId } = useParams<{ projectId: string }>()
  const { data } = usePipelines(projectId)
  // ...
}
```

## 10. Планируемые маршруты

При реализации Phase 1+ будут добавлены:

| Route | Page | Phase | Описание |
|-------|------|-------|----------|
| `/settings` | `SettingsPage` | Phase 1+ | Настройки пользователя |
| `/projects/:id/settings` | `ProjectSettingsPage` | Phase 1+ | Настройки проекта |
| `/secrets` | `SecretsPage` | Phase 7 | Управление секретами |
| `/runners` | `RunnersPage` | Phase 4+ | Управление runner-агентами |

### 10.1 RequireAuth (план)

```tsx
function RequireAuth({ children }: { children: React.ReactNode }) {
  const token = useAuthStore((s) => s.token)
  if (!token) return <Navigate to="/login" replace />
  return <>{children}</>
}
```

Все app-роуты будут обёрнуты в `<RequireAuth>`:

```tsx
{
  element: <RequireAuth><AppShell /></RequireAuth>,
  children: [ ... ]
}
```

## References

- `frontend/src/app/router.tsx` — исходный код роутера.
- `docs/FRONTEND_ARCHITECTURE.md` — архитектура frontend.
- `docs/FRONTEND_STANDARDS.md` — стандарты кодирования.
- `docs/SECURITY.md` — план аутентификации (RequireAuth).