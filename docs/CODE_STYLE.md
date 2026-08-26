# Code Style — Forge CI/CD

## 1. Overview

Единые соглашения по коду для backend (Rust) и frontend (TypeScript/React). Цель — читаемость, минимум дискуссий в PR, консистентность. Конвенции совпадают с task-tracker для кросс-проектной консистентности.

## 2. General

- Код и комментарии — **English**. Русский — для пользовательских строк и документации.
- Line ending: LF.
- Encoding: UTF-8.
- Max line length: 100.
- Indent: 2 spaces (frontend), 4 spaces (Rust).

## 3. Rust

### 3.1 Format

```bash
cargo fmt
cargo clippy --all-targets --all-features -- -D warnings
```

CI проверяет `cargo fmt --check` и `cargo clippy --all-targets -- -D warnings`.

### 3.2 Naming

| Type | Convention | Example |
|------|------------|---------|
| Modules / crates | snake_case | `store`, `api` |
| Types / traits / enums | PascalCase | `AppState`, `JobStatus` |
| Functions / methods | snake_case | `create_project`, `list_pipelines` |
| Variables | snake_case | `project_id`, `pipeline_id` |
| Constants | SCREAMING_SNAKE_CASE | `MAX_POOL_CONNECTIONS` |
| Error enum variants | PascalCase | `TerminalStatus`, `InvalidTransition` |
| Generic params | single uppercase | `T`, `E` |
| Enum variants | PascalCase | `Queued`, `Running`, `Success` |

### 3.3 Imports

```rust
// 1. std
use std::sync::Arc;

// 2. external crates
use axum::{Json, Router, extract::{Path, State}};
use sqlx::PgPool;

// 3. internal crates / modules
use crate::domain::JobStatus;
use crate::store::next_log_sequence;
```

### 3.4 Error Handling

- Использовать `?` оператор.
- Не использовать `.unwrap()` / `.expect()` в production коде.
- В тестах `.unwrap()` допустим.
- Кастомные ошибки через `thiserror::Error` derive.
- HTTP-ошибки — `ApiError` enum с маппингом в `IntoResponse`.

```rust
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (self.status, Json(serde_json::json!({"error": self.message})))
            .into_response()
    }
}
```

### 3.5 Async

- Все IO-bound операции — async.
- `tokio::spawn` только для background tasks (future phases).
- `#[tokio::main]` — точка входа.

### 3.6 Comments

```rust
/// Transitions job to a new status, validating the state machine.
///
/// # Errors
/// Returns `TransitionError::TerminalStatus` if current status is terminal.
/// Returns `TransitionError::InvalidTransition` if transition is not allowed.
pub fn transition_to(self, next: Self) -> Result<Self, TransitionError> {
    // Terminal statuses cannot change.
    if matches!(self, Self::Success | Self::Failed | Self::Canceled) {
        return Err(TransitionError::TerminalStatus);
    }
    // ...
}
```

### 3.7 Tests

```rust
#[tokio::test]
async fn health_endpoint_reports_service_ready() {
    // arrange
    let app = app(None);

    // act
    let response = app
        .oneshot(Request::get("/api/v1/health").body(Body::empty()).unwrap())
        .await
        .unwrap();

    // assert
    assert_eq!(response.status(), StatusCode::OK);
}
```

### 3.8 SQL

- Только parameterized queries: `sqlx::query("... WHERE id = $1").bind(id)`.
- Никакого string interpolation / concatenation в SQL.
- `sqlx::query_as::<_, T>` для маппинга в struct (через `FromRow`).
- Схема — `CREATE TABLE IF NOT EXISTS` в `store::migrate()`.

### 3.9 Module organization

- `lib.rs` — публичные модули (`pub mod api; pub mod domain; pub mod store;`).
- `main.rs` — точка входа, конфигурация, запуск сервера.
- `api.rs` — все HTTP-хендлеры, DTO, роутер.
- `domain.rs` — доменные правила, не зависящие от HTTP и БД.
- `store.rs` — SQL-схема и хелперы БД.
- `src/bin/cicd-cli.rs` — отдельный CLI бинарник.

## 4. TypeScript / React

### 4.1 Format

```bash
pnpm prettier --write .
pnpm eslint --fix .
```

### 4.2 Naming

| Type | Convention | Example |
|------|------------|---------|
| Files | kebab-case | `dashboard.tsx`, `theme-toggle.tsx` |
| Components | PascalCase | `Dashboard`, `StatusBadge` |
| Hooks | camelCase with `use` prefix | `useProjects` |
| Types | PascalCase | `Project`, `Pipeline`, `JobLog` |
| Constants | SCREAMING_SNAKE_CASE | `API_BASE_URL` |
| Boolean vars | is/has/should prefix | `isLoading`, `hasError` |
| Event handlers | handle prefix | `handleSubmit`, `handleJobStatus` |
| Functions | camelCase | `loadProjects`, `triggerPipeline` |

### 4.3 Imports

```tsx
// 1. React / external
import { useEffect, useState } from 'react'

// 2. internal absolute (when configured)
// (not yet configured — use relative)

// 3. relative
import { StatusBadge } from './dashboard'

// 4. types
import type { Project, Pipeline } from './dashboard'
```

### 4.4 Components

```tsx
interface StatusBadgeProps {
  status: Status
}

export function StatusBadge({ status }: StatusBadgeProps) {
  return <span className={`status status-${status}`}>{statusLabel(status)}</span>
}
```

- Один публичный компонент на файл.
- Function components only (no class components).
- Props interface над компонентом.

### 4.5 API client

```tsx
const api = async <T,>(path: string, init?: RequestInit): Promise<T> => {
  const response = await fetch(`/api/v1${path}`, {
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

### 4.6 Types

- Предпочитать `type` для объектов и union.
- `interface` только для public API / OOP shape.
- Никогда не использовать `any`. Использовать `unknown` + narrow.

```tsx
export type Status = 'queued' | 'running' | 'success' | 'failed' | 'canceled'

type Project = { id: string; name: string; repository_url: string; default_branch: string }
```

### 4.7 Comments

```ts
// Bad
// increment i
i++

// Good
// Compensate for zero-based index when displaying row number.
const rowNumber = index + 1
```

### 4.8 Tests

```tsx
import { describe, expect, it } from 'vitest'
import { render, screen } from '@testing-library/react'
import { statusLabel, StatusBadge } from './dashboard'

describe('pipeline statuses', () => {
  it('renders a readable success badge', () => {
    render(<StatusBadge status="success" />)
    expect(screen.getByText('Success')).toBeTruthy()
  })

  it('formats queued status for the dashboard', () => {
    expect(statusLabel('queued')).toBe('Queued')
  })
})
```

## 5. Commits

- Conventional commits.
- Формат: `type(scope): subject`.

| Type | Use |
|------|-----|
| `feat` | Новая фича |
| `fix` | Исправление бага |
| `docs` | Документация |
| `refactor` | Рефакторинг без изменения поведения |
| `test` | Тесты |
| `chore` | Сборка, deps, CI |
| `perf` | Производительность |

Примеры:

```
feat(api): add pipeline trigger endpoint
fix(domain): reject queued→success transition
refactor(store): extract next_log_sequence helper
docs(api): add job logs endpoint examples
test(domain): add terminal status transition tests
chore(ci): add docker compose build to CI
```

## 6. PR Rules

- PR должен быть небольшим (max 400–500 строк).
- Все CI checks green (`cargo test`, `cargo clippy`, `pnpm test`, `pnpm build`).
- Self-review перед запросом review.
- Merge только после approve.
- Squash merge предпочтителен.

## 7. File Organization

- Один публичный компонент/хук на файл.
- Стили — Tailwind utility classes, no CSS-in-JS.
- shadcn/ui компоненты в `frontend/src/shared/ui/`.
- Константы — рядом с использованием или в `shared/config`.

## 8. Lint Configs

### 8.1 Rust

CI (`.github/workflows/ci.yml`):
```yaml
- run: cargo fmt --check
- run: cargo clippy --all-targets -- -D warnings
```

### 8.2 TypeScript

- `@typescript-eslint/recommended` (целевое).
- `eslint-plugin-react-hooks` (целевое).
- Prettier — для форматирования.

## 9. Documentation

- Публичные API методы Rust — doc comments (`///`).
- Сложные frontend функции — JSDoc.
- Любые non-obvious решения — запись в `docs/`.
- При изменении API обновлять `docs/API.md`.
- При изменении схемы БД обновлять `docs/DATA_MODEL.md`.

## 10. Prohibited

- `unwrap()` / `expect()` в production Rust (кроме startup).
- `any` в TypeScript.
- `console.log` в production (использовать `tracing` на бэкенде).
- Inline styles (`style={{}}`) — использовать Tailwind classes.
- Magic numbers/строки без констант.
- Copy-pasted large blocks без выноса в функцию/компонент.
- String interpolation в SQL — только parameterized queries.

## 11. Git Workflow

- Main branch: `main`.
- Feature branches: `feat/pipeline-yaml-config`.
- Fix branches: `fix/job-status-transition`.
- Rebase before merge; no merge commits if possible.
- Force-push — только в своей feature-ветке.

## 12. API Versioning in Code

- Все REST endpoint под `/api/v1`.
- DTO именуются без версии: `Project`, `Pipeline`, `Job` (не `ProjectV1`).
- При breaking change — `/api/v2` параллельно с deprecation.

## 13. Configuration

- Никаких secrets в коде.
- Все env vars с префиксом `CICD_`.
- Валидация config при старте; fail fast.
- `.env.example` — шаблон, `.env` — в `.gitignore`.

## 14. Security

- SQL только через parameterized queries (`sqlx::query` с `$1`, `$2`).
- Никакого `eval` / `innerHTML` с пользовательским контентом.
- CORS — permissive для dev, whitelist для production (Phase 9).
- Все secrets — через env vars.

## 15. Performance

- Rust: avoid unnecessary clones, use `Arc` / references.
- React: use `React.memo` only after profiling.
- DB: always index query predicates (TODO: добавить индексы — см. `DATA_MODEL.md`).
- `fetch_all` для списков, `fetch_one` для единичных записей, `fetch_optional` для may-not-exist.

## 16. Accessibility

- All interactive elements focusable.
- Semantic HTML (`<button>`, `<nav>`, `<article>`, `<section>`).
- ARIA labels where text label отсутствует (`role="alert"` для ошибок).
- Color contrast ≥ 4.5:1.

## References

- `docs/ARCHITECTURE.md`
- `docs/AGENTS.md`
- `docs/TESTING.md`
