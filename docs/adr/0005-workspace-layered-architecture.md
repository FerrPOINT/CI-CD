# ADR-0005: Cargo workspace и слоистая архитектура (ports & adapters)

## Status

Accepted

## Context

`cicd-server` — одиночный crate: `api.rs` (853 строки) содержит и HTTP-слой, и SQL; `platform.rs` (754 строки) аналогично смешивает DTO, валидацию и запросы; `git_host.rs`, `pulls.rs`, `runner.rs` держат бизнес-логику, инфраструктуру и HTTP в одном файле. Тесты API работают без БД (`app(None)`), поэтому персистентность не покрыта. CLI живёт как `src/bin/cicd-cli.rs` внутри серверного crate и линкует всю серверную инфраструктуру.

Task-tracker уже доказал на этом же стеке другой подход: Cargo workspace со слоями `api → app → domain → infra`, отдельные пакеты `server` и `cli`, миграции отдельным пакетом. Это даёт: юнит-тестируемый domain без I/O, подмену адаптеров в тестах, независимую сборку CLI, изоляцию инфраструктурных зависимостей.

Без перестройки добавление auth/RBAC, runner-протокола и миграций будет углублять связность: HTTP-хендлеры продолжат владеть SQL, а любые новые cross-cutting функции (аутентификация, аудит) придётся дублировать по файлам.

## Alternatives Considered

| Option | Pros | Cons |
|---|---|---|
| Оставить монолит и наращивать файлы | Нулевые затраты сейчас | SQL в хендлерах, нет границы для подмен в тестах, CLI тянет сервер, невозможен чистый unit-слой |
| Полный rewrite сразу во все слои | Идеальная целевая структура | Большой bang, высокий риск регрессий, длинный период красных тестов |
| Workspace + strangler-миграция по вертикалям | Сохраняет рабочее поведение, каждая фаза компилируется и тестируется, раннее выделение CLI и domain | Временные shim-модули и двойные пути; дисциплина миграции |

## Decision

Backend переводится на Cargo workspace со слоями по образцу task-tracker:

```text
backend/
├── Cargo.toml        # workspace + единые версии зависимостей
├── domain/           # чистые бизнес-типы и port-trait'ы; без axum/sqlx/fs
├── app/              # use-case'ы, политики, границы транзакций
├── infra/            # PostgreSQL-репозитории, Git/artifact/runner-адаптеры, миграции
├── api/              # HTTP DTO, роуты, middleware, OpenAPI
├── server/           # composition root; единственный, кто создаёт PgPool
├── cli/              # cicd-cli: HTTP-клиент (clap + reqwest), без серверных зависимостей
├── migration/        # версионные SQLx-миграции
├── tests/            # black-box интеграционные тесты
└── scripts/          # test DB, backup/restore/verify
```

Правила зависимостей:

- `domain` → только std/serde/uuid/chrono/thiserror.
- `app` → `domain`; без axum/sqlx/git2/Docker.
- `infra` → `domain`, `app`; владеет SQLx, git2, AES-GCM, процессами.
- `api` → `app`, `domain`; без бизнес-SQL.
- `server` — единственный binary, собирающий зависимости (ди, как в task-tracker `server/`).
- `cli` — отдельный пакет, общается только по HTTP; не линкует server/infra.

Миграция — strangler: сначала workspace + перенос `domain` и `cli` без изменения поведения (текущий этап), затем конфиг/ошибки/миграции, затем пофайловый перенос вертикалей (projects → jobs/logs → git/PR → platform → users/tokens) с реальными БД-тестами на каждый срез. Публичные REST-пути и JSON-контракты не меняются до перехода фронтенда на OpenAPI-генерацию.

## Consequences

- Domain-правила (`JobStatus::transition_to`) тестируются без I/O; shim `src/domain.rs` сохраняет старые пути `cicd::domain::*` до полного переноса вызовов на `cicd_domain`.
- CLI собирается и публикуется независимо от сервера (`cargo build -p cicd-cli`); контракты CLI-команд сохранены.
- Инфраструктурные зависимости (sqlx, git2) перестают быть видимы HTTP-слою после миграции вертикали — enforcement через review и `docs/CODE_STYLE.md`.
- Временный период сосуществования shim и новых пакетов требует, чтобы новые фичи писались сразу в целевые слои.
- CI переводится на `cargo test --workspace` / `clippy --workspace`.

## Migration Path

1. (готово) workspace: `domain` выделен, `cli` перенесён в `backend/cli`, корневой crate сохраняет прежний API через re-export.
2. `shared`-конфиг + `AppError` + SQLx-миграции + test-DB compose.
3. Перенос вертикалей в `app`/`infra`/`api` с real-DB тестами; удаление shim.
4. `server` как отдельный composition-root пакет; OpenAPI + генерация фронтового клиента.

## Related

- `docs/ARCHITECTURE.md`
- `plans/architecture-rebuild-plan.md`
- `task-tracker/docs/ARCHITECTURE.md` (референс)
- `docs/adr/0003-manual-job-transitions.md` (supersedes его «Migration Path» п.1 в части структуры)
