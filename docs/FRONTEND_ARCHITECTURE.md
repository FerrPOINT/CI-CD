# Frontend Architecture — Forge CI/CD

## 1. Overview

Frontend — одностраничное React-приложение на Vite 6 + TypeScript 5.9 + Tailwind CSS 4 + shadcn/ui. Архитектура построена по методологии **Feature-Sliced Design (FSD)**.

Цели:

- масштабируемость при росте функционала;
- высокая связность внутри фичи, низкая — между фичами;
- переиспользование UI-компонентов;
- чёткое разделение бизнес-логики и UI;
- типобезопасность end-to-end.

## 2. Tech Stack

| Слой | Библиотека | Версия |
|------|-----------|--------|
| Framework | react + react-dom | 19.1.0 |
| Build | vite | 6.2.0 |
| Language | typescript | 5.9.3 |
| Styling | tailwindcss + @tailwindcss/vite | 4.1.0 |
| Components | shadcn/ui + Radix primitives | — |
| State (server) | @tanstack/react-query | 5.74.4 |
| State (client) | zustand | 5.0.3 |
| Routing | react-router | 8.1.0 |
| i18n | i18next + react-i18next + i18next-http-backend | 25.1.0 / 15.5.0 / 3.0.2 |
| Icons | lucide-react | 0.487.0 |
| Toasts | sonner | 2.0.3 |
| Tests | vitest + @testing-library/react | 3.1.4 / 16.3.0 |
| Utils | clsx + tailwind-merge + class-variance-authority | — |

## 3. FSD Layers

Проект использует Feature-Sliced Design с упрощённым набором слоёв:

| Слой | Ответственность | Может импортировать из |
|------|-----------------|----------------------|
| `app` | Инициализация, роутер, providers, глобальные стили | `pages`, `widgets`, `shared` |
| `pages` | Страницы, композиция виджетов | `widgets`, `shared` |
| `widgets` | Самостоятельные блоки интерфейса | `shared` |
| `shared` | Переиспользуемые примитивы, API-клиент, i18n, utils, UI | — |

> **Примечание:** слои `features` и `entities` будут добавлены при росте функционала. В текущей MVP-версии бизнес-логика живет в `pages` и `shared/api`.

### 3.1 Правила импорта

- Слой может импортировать только из слоёв **ниже** или **равных** себе.
- `shared` не импортирует ничего из других слоёв.
- `app` — единственный слой, импортирующий из `pages`.
- Публичный API слайса — через `index.ts` (баррель-файл).

## 4. Folder Structure

```
frontend/src/
├── app/                        # Инициализация приложения
│   └── router.tsx              # Конфигурация роутов
├── pages/                      # Страницы приложения
│   ├── dashboard/              # Главная / дашборд
│   ├── projects/               # Список проектов
│   ├── pipelines/              # Список пайплайнов проекта
│   ├── pipeline-detail/        # Детали пайплайна (stages/jobs/logs)
│   ├── admin/                  # Админ-панель
│   └── login/                  # Страница входа
├── widgets/                    # Самостоятельные UI-блоки
│   └── app-shell.tsx           # Каркас приложения (header + sidebar + outlet)
├── shared/                     # Переиспользуемый код
│   ├── api/                    # API-клиент, типы, hooks
│   │   ├── client.ts           # fetch-обёртка
│   │   ├── types.ts            # TypeScript-интерфейсы (Project, Pipeline, ...)
│   │   └── hooks.ts            # TanStack Query hooks
│   ├── i18n/                   # Локализация
│   │   ├── config.ts           # i18next конфигурация
│   │   └── locales/
│   │       ├── ru.json         # Русская локаль (default)
│   │       └── en.json         # Английская локаль
│   ├── ui/                     # shadcn/ui primitives
│   ├── lib/                    # utils (cn, formatters)
│   └── auth/                   # Auth helpers (планируется)
├── index.css                   # Tailwind 4 @theme, CSS-токены
├── main.tsx                    # Точка входа
└── vite-env.d.ts               # Vite type declarations
```

## 5. App Layer

### 5.1 Router

Роутер объявлен в `app/router.tsx` с использованием `createBrowserRouter` (react-router 8). Все страницы lazy-loaded через `React.lazy` + `Suspense` (см. `docs/ROUTING.md`).

### 5.2 Providers

Композиция провайдеров в `main.tsx`:

```tsx
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import './shared/i18n/config'

const queryClient = new QueryClient({
  defaultOptions: {
    queries: { retry: 1, refetchOnWindowFocus: false },
  },
})

createRoot(document.getElementById('root')!).render(
  <QueryClientProvider client={queryClient}>
    <RouterProvider router={router} />
  </QueryClientProvider>,
)
```

### 5.3 QueryClient

Конфигурация TanStack Query:

- `retry: 1` — одна попытка повтора при ошибке запроса.
- `refetchOnWindowFocus: false` — отключить авто-рефетч при возврате фокуса.
- Ключи запросов централизованы в `shared/api/hooks.ts`.

## 6. Pages

### 6.1 Dashboard (`/`)

Главная страница: обзор проектов и последних пайплайнов.

### 6.2 Projects (`/projects`)

Список проектов с формой создания. Использует `useProjects()` и `useCreateProject()`.

### 6.3 Pipelines (`/projects/:projectId/pipelines`)

Список пайплайнов выбранного проекта. Использует `usePipelines(projectId)` и `useTriggerPipeline(projectId)`.

### 6.4 Pipeline Detail (`/pipelines/:pipelineId`)

Детали пайплайна: стадии, задачи, логи. Управление статусами задач (start/pass/fail). Использует `usePipeline(id)`, `useUpdateJobStatus()`, `useJobLogs(jobId)`, `useAppendLog()`.

### 6.5 Admin (`/admin`)

Админ-панель (планируется).

### 6.6 Login (`/login`)

Страница входа (планируется, Phase 1).

## 7. Shared / API

### 7.1 Client

```ts
// shared/api/client.ts
const BASE = '/api/v1'

export async function api<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`${BASE}${path}`, {
    headers: { 'Content-Type': 'application/json' },
    ...init,
  })
  if (!response.ok) {
    const body = await response.json().catch(() => ({ error: response.statusText }))
    throw new Error(body.error || response.statusText)
  }
  return response.json() as Promise<T>
}
```

- Базовый путь `/api/v1` — Vite dev proxy проксирует на `http://localhost:22801`.
- Все ответы типизированы через TypeScript-интерфейсы из `types.ts`.
- Ошибки парсятся из `{"error": "message"}` формата.

### 7.2 Types

Все типы повторяют DTO бэкенда (snake_case):

```ts
export type Status = 'queued' | 'running' | 'success' | 'failed' | 'canceled'

export interface Project { id, name, repository_url, default_branch, created_at }
export interface Pipeline { id, project_id, git_ref, status, created_at, started_at, finished_at }
export interface Stage { id, pipeline_id, name, position, status, jobs: Job[] }
export interface Job { id, stage_id, name, image, command, position, status, started_at, finished_at }
export interface PipelineDetail { pipeline: Pipeline, stages: Stage[] }
export interface JobLog { id, job_id, sequence, message, created_at }
```

### 7.3 Hooks (TanStack Query)

Все API-запросы обёрнуты в TanStack Query hooks:

| Hook | Тип | Описание |
|------|-----|----------|
| `useProjects()` | query | Список проектов |
| `useCreateProject()` | mutation | Создание проекта + invalidate `projects` |
| `usePipelines(projectId)` | query | Пайплайны проекта |
| `useTriggerPipeline(projectId)` | mutation | Запуск пайплайна + invalidate `pipelines` |
| `usePipeline(id)` | query | Детали пайплайна |
| `useUpdateJobStatus()` | mutation | Смена статуса задачи + invalidate all |
| `useJobLogs(jobId)` | query | Логи задачи |
| `useAppendLog()` | mutation | Добавление лога + invalidate `logs` |

Ключи запросов:

```ts
const KEYS = {
  projects: ['projects'] as const,
  pipelines: (projectId: string) => ['pipelines', projectId] as const,
  pipeline: (id: string) => ['pipeline', id] as const,
  logs: (jobId: string) => ['logs', jobId] as const,
}
```

## 8. Widgets

### 8.1 AppShell

Каркас приложения — `widgets/app-shell.tsx`. Содержит:

- Header с навигацией и переключателем темы.
- Sidebar с навигацией.
- `<Outlet />` для рендеринга дочерних роутов.

Все protected-роуты обёрнуты в `<AppShell />` через router configuration.

## 9. Lazy Loading

Все страницы lazy-loaded для route-level code splitting:

```tsx
const DashboardPage = lazy(() => import('@/pages/dashboard').then(m => ({ default: m.DashboardPage })))
const ProjectsPage = lazy(() => import('@/pages/projects').then(m => ({ default: m.ProjectsPage })))
// ...
```

Каждая страница обёрнута в `<Suspense fallback={<PageLoader />}>`:

```tsx
function PageLoader() {
  return <div className="flex items-center justify-center py-16 text-sm text-text-muted">Loading…</div>
}
```

## 10. Styling

### 10.1 Tailwind CSS 4

- Подключён через `@tailwindcss/vite` plugin.
- CSS-токены определены в `src/index.css` через `@theme` directive.
- Темы: `dark` (default), `gray`, `light` — переключатель в header, значение в `localStorage`.
- Подробности — в `docs/UI_UX.md`.

### 10.2 shadcn/ui

- Компоненты на основе Radix UI primitives.
- Находятся в `shared/ui/`.
- Кастомизация через CSS-токены, не через Tailwind-конфиг.
- `cn()` утилита из `shared/lib/` для условных классов.

## 11. Vite Configuration

```ts
// vite.config.ts
export default defineConfig({
  plugins: [react(), tailwindcss()],
  test: { environment: 'jsdom' },
  resolve: {
    alias: { '@': fileURLToPath(new URL('./src', import.meta.url)) },
  },
  server: {
    proxy: { '/api': 'http://localhost:22801' },
  },
})
```

- Path alias `@` → `src/`.
- Dev proxy `/api` → `http://localhost:22801` (backend).
- Vitest environment: `jsdom`.

## 12. Build

```bash
cd frontend
pnpm install
pnpm build   # tsc --noEmit && vite build
```

Результат — `frontend/dist/`, раздаётся через nginx в Docker-контейнере.

## References

- `docs/FRONTEND_STANDARDS.md` — стандарты кодирования.
- `docs/ROUTING.md` — схема роутов.
- `docs/I18N.md` — локализация.
- `docs/UI_UX.md` — дизайн-система, CSS-токены.
- `frontend/vite.config.ts` — конфигурация сборки.