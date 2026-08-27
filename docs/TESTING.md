# Стратегия тестирования Forge CI/CD

## 1. Принципы

- Каждый тест проверяет значимый путь и конкретное поведение.
- Backend: unit-тесты для domain-логики, интеграционные тесты для API contract и CLI.
- Frontend: unit-тесты на Vitest; E2E на Playwright (целевое).
- После изменений UI — скриншоты: desktop 1920×1080 (текущий стандарт), mobile 375×812 и 2K 2560×1440 — целевое.
- Все новые endpoint — curl-проверка.
- Docker compose smoke: `docker compose up --build -d` + `curl /api/v1/health`.

## 2. Backend тесты

### Unit-тесты

**Domain transitions** (`backend/tests/domain_transitions.rs`):

Тестирует `JobStatus::transition_to()` — конечный автомат переходов статусов:

```rust
#[test]
fn queued_job_can_start_and_finish_successfully() {
    assert_eq!(
        JobStatus::Queued.transition_to(JobStatus::Running),
        Ok(JobStatus::Running)
    );
    assert_eq!(
        JobStatus::Running.transition_to(JobStatus::Success),
        Ok(JobStatus::Success)
    );
}

#[test]
fn terminal_job_cannot_restart() {
    assert_eq!(
        JobStatus::Failed.transition_to(JobStatus::Running),
        Err(TransitionError::TerminalStatus)
    );
}

#[test]
fn queued_job_cannot_skip_directly_to_success() {
    assert_eq!(
        JobStatus::Queued.transition_to(JobStatus::Success),
        Err(TransitionError::InvalidTransition {
            from: JobStatus::Queued,
            to: JobStatus::Success,
        })
    );
}
```

Покрытые сценарии:
- `queued → running` ✅
- `running → success` ✅
- `failed → running` ❌ TerminalStatus
- `queued → success` ❌ InvalidTransition

### Integration-тесты

**API contract** (`backend/tests/api_contract.rs`):

Тестирует HTTP-слой через `tower::ServiceExt::oneshot` (без реальной БД):

```rust
#[tokio::test]
async fn health_endpoint_reports_service_ready() {
    let response = app(None)
        .oneshot(Request::get("/api/v1/health").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}
```

`app(None)` — создание роутера без БД (pool = None). Health endpoint работает без БД.

**CLI contract** (`backend/tests/cli_contract.rs`):

Тестирует CLI binary через `std::process::Command`:

```rust
#[test]
fn cli_exposes_project_pipeline_and_job_groups() {
    let binary = env!("CARGO_BIN_EXE_cicd-cli");
    let output = Command::new(binary).arg("--help").output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(output.status.success());
    assert!(stdout.contains("project"));
    assert!(stdout.contains("pipeline"));
    assert!(stdout.contains("job"));
}
```

### Запуск

```bash
# Через Rust контейнер (хост может не иметь cargo)
docker run --rm --entrypoint /bin/bash   -v "$PWD/backend:/workspace" -w /workspace rust:1.86-bookworm   -lc '/usr/local/cargo/bin/cargo test'

# Или через justfile
just test-backend

# Только конкретный тест
cargo test -p cicd-server --test domain_transitions
cargo test -p cicd-server --test api_contract
cargo test -p cicd-server --test cli_contract
```

### Тесты с БД (целевое)

Для endpoint, требующих PostgreSQL (projects CRUD, pipelines, jobs):

- Использовать testcontainers или Docker compose PostgreSQL.
- Каждое тестирование — изолированная БД или транзакция с rollback.
- `CICD_DATABASE_URL` — указывать на test-БД.

## 3. Frontend тесты

### Unit-тесты

Фреймворк: Vitest + `@testing-library/react` + `@testing-library/jest-dom`.

Конфигурация в `vite.config.ts`:
```typescript
export default defineConfig({
  plugins: [react()],
  test: { environment: 'jsdom' },
})
```

Текущие тесты (`frontend/src/dashboard.test.tsx`):

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

Запуск:
```bash
cd frontend
pnpm test

# Или через justfile
just test-frontend
```

### E2E (целевое)

Playwright specs в `frontend/e2e/` (планируется):

- `integration.spec.ts` — smoke против Docker backend: health, create project, trigger pipeline, check stages.
- `screenshots.spec.ts` — мульти-вьюпортные скриншоты.

Запуск (целевое):
```bash
cd frontend
pnpm exec playwright test --project=chromium
```

### Screenshot набор

Скриншоты сохраняются в `/root/.hermes/cache/images/` (целевое).
Вьюпорты: 375×812 (mobile), 1920×1080 (desktop), 2560×1440 (wide).

## 4. Docker compose smoke

Полный smoke-тест через Docker Compose:

```bash
# 1. Сборка и запуск
docker compose up --build -d

# 2. Проверка контейнеров
docker compose ps
# Ожидание: postgres healthy, backend healthy, frontend running

# 3. Health check
curl -fsS http://127.0.0.1:22801/api/v1/health
# Ожидание: {"status":"ok","service":"cicd"}

# 4. Создание проекта
curl -sS -X POST http://127.0.0.1:22801/api/v1/projects \
  -H 'content-type: application/json' \
  -d '{"name":"smoke-test","repository_url":"git@github.com:org/repo.git"}'
# Ожидание: 200 + JSON с id

# 5. Список проектов
curl -sS http://127.0.0.1:22801/api/v1/projects
# Ожидание: массив с созданным проектом

# 6. Запуск пайплайна
PROJECT_ID=$(curl -sS http://127.0.0.1:22801/api/v1/projects | jq -r '.[0].id')
curl -sS -X POST "http://127.0.0.1:22801/api/v1/projects/$PROJECT_ID/pipelines" \
  -H 'content-type: application/json' -d '{"git_ref":"main"}'
# Ожидание: 200 + PipelineDetail с 3 stages

# 7. Смена статуса job
JOB_ID=$(curl -sS "http://127.0.0.1:22801/api/v1/projects/$PROJECT_ID/pipelines" | jq -r '.[0].id')
PIPELINE_ID=$JOB_ID
JOB_ID=$(curl -sS "http://127.0.0.1:22801/api/v1/pipelines/$PIPELINE_ID" | jq -r '.stages[0].jobs[0].id')
curl -sS -X POST "http://127.0.0.1:22801/api/v1/jobs/$JOB_ID/status" \
  -H 'content-type: application/json' -d '{"status":"running"}'
# Ожидание: 200 + job со статусом running

# 8. Добавление лога
curl -sS -X POST "http://127.0.0.1:22801/api/v1/jobs/$JOB_ID/logs" \
  -H 'content-type: application/json' -d '{"message":"Smoke test log"}'
# Ожидание: 200 + JobLog с sequence=1

# 9. Чтение логов
curl -sS "http://127.0.0.1:22801/api/v1/jobs/$JOB_ID/logs"
# Ожидание: массив с 1 логом

# 10. Frontend доступен
curl -fsS http://127.0.0.1:22802/
# Ожидание: HTML

# 11. Очистка
docker compose down -v
```

Через justfile:
```bash
just up
just health
just down
```

## 5. curl API verification

Полный end-to-end workflow через curl:

```bash
# Setup
API=http://127.0.0.1:22801/api/v1

# 1. Health
curl -fsS $API/health

# 2. Create project
PROJECT=$(curl -sS -X POST $API/projects \
  -H 'content-type: application/json' \
  -d '{"name":"my-service","repository_url":"git@github.com:org/my-service.git"}')
PROJECT_ID=$(printf '%s' "$PROJECT" | jq -r .id)

# 3. List projects
curl -sS $API/projects | jq .

# 4. Trigger pipeline
PIPELINE=$(curl -sS -X POST "$API/projects/$PROJECT_ID/pipelines" \
  -H 'content-type: application/json' -d '{"git_ref":"main"}')
PIPELINE_ID=$(printf '%s' "$PIPELINE" | jq -r .pipeline.id)

# 5. Show pipeline detail
curl -sS "$API/pipelines/$PIPELINE_ID" | jq .

# 6. Job status transitions
JOB_ID=$(printf '%s' "$PIPELINE" | jq -r '.stages[0].jobs[0].id')

# Start
curl -sS -X POST "$API/jobs/$JOB_ID/status" \
  -H 'content-type: application/json' -d '{"status":"running"}' | jq .

# Append log
curl -sS -X POST "$API/jobs/$JOB_ID/logs" \
  -H 'content-type: application/json' -d '{"message":"Starting..."}' | jq .

# Complete
curl -sS -X POST "$API/jobs/$JOB_ID/status" \
  -H 'content-type: application/json' -d '{"status":"success"}' | jq .

# 7. Verify aggregated status
curl -sS "$API/pipelines/$PIPELINE_ID" | jq .pipeline.status
# Ожидание: "running" (остальные jobs ещё queued)

# 8. Negative tests
# Invalid transition
curl -sS -X POST "$API/jobs/$JOB_ID/status" \
  -H 'content-type: application/json' -d '{"status":"running"}'
# Ожидание: 400 {"error": "terminal status cannot change"}

# Empty name
curl -sS -X POST $API/projects \
  -H 'content-type: application/json' -d '{"name":"","repository_url":""}'
# Ожидание: 400 {"error": "name and repository_url are required"}

# Non-existent pipeline
curl -sS "$API/pipelines/00000000-0000-0000-0000-000000000000"
# Ожидание: 404 {"error": "resource not found"}
```

## 6. Dev commands

Все команды через `justfile`:

```bash
just up              # docker compose up --build -d
just down            # docker compose down
just logs            # docker compose logs -f
just test-backend    # cargo test in Rust container
just test-frontend   # pnpm test
just build-frontend  # pnpm build
just health          # curl /api/v1/health
```

## 7. CI (GitHub Actions)

`.github/workflows/ci.yml` — три job:

### backend
```yaml
- run: cargo fmt --check
- run: cargo clippy --all-targets -- -D warnings
- run: cargo test
- run: cargo build --release
```

### frontend
```yaml
- run: pnpm install --frozen-lockfile
- run: pnpm test
- run: pnpm build
```

### containers
```yaml
needs: [backend, frontend]
- run: docker compose build
```

## 8. Тестовое покрытие (целевое)

| Layer | Target |
|---|---|
| Domain (`domain.rs`) | ≥90% |
| API handlers (`api.rs`) | ≥80% |
| Store (`store.rs`) | ≥70% |
| CLI (`cicd-cli.rs`) | ≥60% |
| Frontend components | ≥70% |

## 9. Git hooks (целевое)

Lefthook (`lefthook.yml`) — планируется:
- `pre-commit`: rust fmt check, clippy, frontend test + lint.
- `pre-push`: backend tests, frontend build.
- `commit-msg`: conventional commits (`feat|fix|docs|...`).

## 10. Checklist перед завершением

- [ ] `cargo test` green
- [ ] `cargo clippy --all-targets -- -D warnings` clean
- [ ] `cargo fmt --check` clean
- [ ] `pnpm test` green
- [ ] `pnpm build` green
- [ ] Docker compose smoke пройден
- [ ] curl API verification пройден
- [ ] Документация обновлена

## References

- `docs/ARCHITECTURE.md` — архитектура приложения.
- `docs/API.md` — REST API спецификация (curl examples).
- `docs/CODE_STYLE.md` — конвенции кода.
- `justfile` — dev commands.
- `.github/workflows/ci.yml` — CI pipeline.
- `backend/tests/` — backend тесты.
- `frontend/src/dashboard.test.tsx` — frontend тесты.
