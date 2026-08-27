# Backend boundaries

> **Статус:** объяснительный документ. Канон имён и зависимостей — ADR-0005/0009 и `docs/contracts/*`.

## Текущие пакеты (verified)

| Crate | Роль | Запрещено |
|---|---|---|
| `cicd-server` (`backend/src`) | монолит: HTTP + SQL + runner + git | — (мигрирует) |
| `cicd-domain` (`backend/domain`) | чистые типы, `JobStatus::transition_to()` | axum/sqlx/docker/git/FS |
| `cicd-cli` (`backend/cli`) | HTTP-клиент control plane | линковка серверного кода |

## Целевые пакеты (approved, ADR-0005)

| Crate | Ответственность | Публичные порты | Зависимости разрешены | Запрещены |
|---|---|---|---|---|
| `domain` | агрегаты, события, state machine | типы + trait-ы портов | std, serde, thiserror, uuid, chrono | axum, sqlx, tokio-runtime(только типы), docker, git2, reqwest |
| `app` | use-case-ы, границы транзакций | `*UseCase` | domain | axum, sqlx напрямую, docker, git2 |
| `infra` | PostgreSQL-репозитории, git/artifact/secret адаптеры, outbox, runner client | реализации портов | domain, app(порты), sqlx, git2, aes-gcm | axum |
| `api` | DTO, роуты, middleware, OpenAPI (utoipa) | `router()` | app, domain | sqlx напрямую |
| `server` | composition root, config, запуск supervisor | `main` | все | — |
| `migration` | `cicd-migrate` (up/verify/adopt-legacy) | CLI | sqlx | axum, app |
| `cli` | пользовательский CLI | CLI | reqwest, generated DTO | серверные крейты |

Dependency flow односторонняя: `api → app → domain`; `infra → app/domain`; `server → all`. Тесты: `domain` unit; `app` unit c in-memory портами; `infra/api` — real-PostgreSQL (compose.test.yml); protocol compatibility — `migration`/`cli` smoke.

## Правила миграции (strangler)

1. Вертикали переносятся по одной: projects → pipelines/jobs/logs → git/PR → platform → users/tokens.
2. Старые пути (`cicd::domain` shim) живут до полного переноса вертикали, затем удаляются.
3. REST/JSON контракт не меняется до перехода на OpenAPI-генерацию (`contracts/API_CONTRACT.md`).
