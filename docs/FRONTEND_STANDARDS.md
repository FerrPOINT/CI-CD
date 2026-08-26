# Frontend Standards — Forge CI/CD

> Соглашения для frontend-подсистемы Forge CI/CD: именование, структура, работа с API, стилизация, тестирование.

## 1. Scope

Стандарты обязательны для всего кода в `frontend/src/`. Конвенции совпадают с task-tracker для кросс-проектной консистентности.

## 2. Общие правила

- Код и комментарии — **English**. Русский — для пользовательских строк (i18n) и документации.
- Line ending: LF.
- Encoding: UTF-8.
- Max line length: 100.
- Indent: 2 spaces.
- Файлы: kebab-case для имён файлов.
- Компоненты: PascalCase для имён компонентов и типов.

## 3. File Naming

| Тип файла | Convention | Пример |
|-----------|-----------|--------|
| Компонент (один) | `kebab-case.tsx` | `app-shell.tsx` |
| Страница | `kebab-case/` (директория) | `pages/pipeline-detail/` |
| Hook | `use-*.ts` | `use-projects.ts` (планируется) |
| Утилита | `kebab-case.ts` | `query-keys.ts` |
| Типы | `types.ts` | `api/types.ts` |
| Конфиг | `*.ts` | `config.ts` |
| Локаль | `*.json` | `ru.json`, `en.json` |

## 4. Component Naming

| Тип | Convention | Пример |
|------|-----------|--------|
| React-компонент | PascalCase | `DashboardPage`, `ProjectCard` |
| Props-интерфейс | `{ComponentName}Props` | `ProjectCardProps` |
| Type alias | PascalCase | `Status`, `PipelineDetail` |
| Enum-like union | PascalCase | `type Status = 'queued' \| ...` |
| Function (non-component) | camelCase | `formatDate`, `cn` |
| Constant | SCREAMING_SNAKE_CASE | `API_BASE`, `KEYS` |
| Hook | `use{Feature}` | `useProjects`, `usePipeline` |

## 5. Import Order

```ts
// 1. React / React Router
import { lazy, Suspense } from 'react'
import { createBrowserRouter, Navigate } from 'react-router'

// 2. Внешние библиотеки
import { useQuery, useMutation } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'

// 3. Внутренние модули (по слоям FSD: shared → widgets → pages)
import { api } from '@/shared/api/client'
import { AppShell } from '@/widgets/app-shell'
import { DashboardPage } from '@/pages/dashboard'

// 4. Типы
import type { Project, Pipeline } from '@/shared/api/types'

// 5. CSS / assets
import './styles.css'
```

Порядок внутри групп — alphabetical. Между группами — пустая строка.

## 6. Component Patterns

### 6.1 Функциональные компоненты

Только функциональные компоненты + hooks. Классовые компоненты запрещены.

```tsx
// ✅ Правильно
export function ProjectCard({ project, onSelect }: ProjectCardProps) {
  return (
    <div className="rounded-lg border border-border p-4">
      <h3 className="text-lg font-semibold">{project.name}</h3>
    </div>
  )
}

// ❌ Запрещено
class ProjectCard extends React.Component<Props> { ... }
```

### 6.2 Props-интерфейсы

```tsx
interface ProjectCardProps {
  project: Project
  onSelect?: (id: string) => void
}

export function ProjectCard({ project, onSelect }: ProjectCardProps) { ... }
```

### 6.3 Составные компоненты (compound)

```tsx
export function StatusBadge({ status }: { status: Status }) { ... }
StatusBadge.Icon = function StatusBadgeIcon() { ... }
```

### 6.4 Презентационные vs контейнерные

- Презентационные компоненты не должны знать о состоянии приложения.
- Контейнерные компоненты подключают hooks (TanStack Query) и передают данные вниз.
- В текущей MVP-версии страницы являются контейнерами, UI-компоненты — презентационными.

## 7. Hooks Naming

| Тип | Pattern | Пример |
|------|---------|--------|
| Query (чтение) | `use{Entity}` | `useProjects`, `usePipeline`, `useJobLogs` |
| Mutation (запись) | `use{Action}{Entity}` | `useCreateProject`, `useTriggerPipeline`, `useUpdateJobStatus` |
| Generic / utility | `use{Capability}` | `useDebounce`, `useMediaQuery` |
| i18n | `useTranslation` | (из react-i18next) |

## 8. Работа с API

### 8.1 Все запросы через TanStack Query

- Никаких прямых `fetch` в компонентах — только через hooks из `shared/api/hooks.ts`.
- Ключи запросов централизованы в `KEYS` объекте.
- Мутации сопровождаются инвалидацией связанных query-ключей.

### 8.2 Пример hook

```ts
export function useProjects() {
  return useQuery({ queryKey: KEYS.projects, queryFn: () => api<Project[]>('/projects') })
}

export function useCreateProject() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (input: { name: string; repository_url: string; default_branch: string }) =>
      api<Project>('/projects', { method: 'POST', body: JSON.stringify(input) }),
    onSuccess: () => qc.invalidateQueries({ queryKey: KEYS.projects }),
  })
}
```

### 8.3 Retry-политика

- `retry: 1` — одна попытка повтора при ошибке.
- `refetchOnWindowFocus: false` — отключено.
- Ошибки 5xx — retry. Ошибки 4xx — без retry (планируется уточнение).

### 8.4 Ошибки

- Ошибки API обрабатываются в `shared/api/client.ts` — парсинг `{"error": "message"}`.
- Ошибки мутаций — через `sonner` toast.
- Ошибки загрузки — в компоненте с retry button.

## 9. Состояние

- **Серверное состояние** — `@tanstack/react-query` (единственный способ работы с API).
- **Глобальное клиентское состояние** — `zustand` (тема, UI-настройки).
- **Локальное состояние** — `useState` / `useReducer`.
- Side-effects — `useEffect` (минимально), кастомные hooks при усложнении.

## 10. Стилизация

### 10.1 Tailwind Utilities Only

- Только Tailwind utility-классы для стилизации.
- Никаких inline-стилей (`style={{ ... }}`) кроме динамических значений.
- Никаких CSS-модулей — только `index.css` для глобальных токенов.
- Кастомные классы — через `cn()` из `shared/lib/`.

### 10.2 CSS-токены

```css
/* src/index.css */
@theme {
  --color-background: #09090b;
  --color-surface: #18181b;
  --color-border: #27272a;
  --color-text-primary: #fafafa;
  --color-accent: #818cf8;
}
```

Использование в Tailwind: `bg-background`, `text-text-primary`, `border-border`, `text-accent`.

### 10.3 Условные классы

```tsx
import { cn } from '@/shared/lib/cn'

<div className={cn('rounded-lg p-4', isActive && 'border-accent', isError && 'border-danger')}>
```

### 10.4 Responsive

- Mobile-first подход.
- Breakpoints: `sm:`, `md:`, `lg:`, `xl:` (Tailwind defaults).

## 11. Формы (планируется)

- `react-hook-form` + `zod` для валидации.
- Схема валидации — рядом с формой.
- Состояния: pristine, submitting, submit error, success.
- Disabled всех полей при `isSubmitting`.

## 12. Тестирование

### 12.1 Unit-тесты

```bash
pnpm test   # vitest run
```

- Vitest + @testing-library/react.
- Имя файла: `*.test.tsx` или `*.test.ts`.
- Рядом с тестируемым модулем или в `__tests__/` директории.

### 12.2 Что тестировать

- Компоненты: рендеринг, взаимодействие (click, input).
- Hooks: TanStack Query hooks — с mock `api` функции.
- Утилиты: чистые функции — полный coverage.

### 12.3 E2E (планируется)

- Playwright.
- Smoke: загрузка страниц, создание проекта, запуск пайплайна.

## 13. Линтеры

```bash
pnpm lint    # eslint src
pnpm build   # tsc --noEmit && vite build (type check)
```

- ESLint с конфигурацией по умолчанию + React plugin.
- TypeScript strict mode.
- Не коммитить код с lint-ошибками.

## 14. Запрещено

- `any` в TypeScript (использовать `unknown` + type guard, или конкретный тип).
- `console.log` в production-коде (использовать logger при появлении).
- `dangerouslySetInnerHTML` без sanitization.
- Прямые `fetch` в компонентах (только через `shared/api/`).
- Inline-стили для статических значений.
- Классовые компоненты.
- `default export` для компонентов (использовать named exports).

## References

- `docs/FRONTEND_ARCHITECTURE.md` — архитектура, FSD-слои.
- `docs/ROUTING.md` — схема роутов.
- `docs/UI_UX.md` — дизайн-система, CSS-токены.
- `docs/I18N.md` — локализация.
- `docs/CODE_STYLE.md` — общие правила (Rust + frontend).