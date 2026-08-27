# Changelog

Все значимые изменения в этом проекте документируются в этом файле.

Формат основан на [Keep a Changelog](https://keepachangelog.com/ru/1.1.0/),
и этот проект стремится соответствовать [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Дата release-записей: формат ISO 8601 (`YYYY-MM-DD`).

## [Unreleased]

### Planned

- Auth: сессии/JWT, RBAC-проверки и enforcement API-токенов (сейчас users/tokens хранятся, но middleware не проверяется).
- Runner: протокол удалённых runner-ов, leases/dispatch вместо embedded-исполнителя.
- Scheduler/outbox: доставка schedules и webhooks (сейчас — только конфигурация, execution/delivery не реализованы).
- Versioned SQLx migrations вместо `CREATE TABLE IF NOT EXISTS` bootstrap.
- Стабилизация API-контрактов: pagination/error envelope, idempotency keys.
- S3-совместимое хранилище артефактов, backup-скрипты, метрики.

## [0.1.0] — 2026-08-26

Первый публичный baseline MVP self-hosted CI/CD control plane (Rust/Axum + React).

### Added

- Проекты CRUD: `name` / `repository_url` / `default_branch`, удаление с CASCADE.
- Git-хостинг: bare-репозитории, Smart HTTP (`clone`/`fetch`/`push`), опциональная token-auth, `post-receive` → автоматический пайплайн.
- Пайплайны из `.forge-ci.yml` (stages/jobs/image/command) с fallback-шаблоном; отмена и повтор, pull requests (create/merge/close/reopen, compare).
- Embedded runner: Docker-контейнеры (`forge-job-<id>`) или host shell, стриминг stdout в `job_logs`, отмена через PID-map.
- Append-only логи джобов с sequence и поллингом; артефакты (upload/download до 50 MiB, локальная директория `CICD_ARTIFACTS_DIR`).
- Секреты проектов: AES-256-GCM at rest, значение не возвращается API.
- Environments/deployments, reports (success rate/duration), audit log (append-only, последние 200 записей).
- Users/roles и API-токены: хранение и управление (enforcement middleware — см. Planned).
- React Dashboard: 21 маршрут / 20 страниц + `/login` (заглушка без auth-запроса), i18n (ru/en).
- CLI `cicd-cli` (HTTP-only): project/pipeline/job; CI-пайплайн GitHub Actions; Docker Compose окружение.

### Known limitations (honestly)

- Нет auth/RBAC/TLS: API и Dashboard полностью открыты, CORS permissive — только доверенные сети, не для production (см. `SECURITY.md`).
- PostgreSQL в compose опубликован на все интерфейсы; `CICD_GIT_INTERNAL_TOKEN` обязательно менять для shared-деплоя.
- Schedules/webhooks/notifications — конфигурация без исполнения; login — UI-заглушка.
- Схема БД через bootstrap `store::migrate()`, versioned migrations ещё не введены.

[Unreleased]: https://github.com/FerrPOINT/CI-CD/compare/0.1.0...HEAD
[0.1.0]: https://github.com/FerrPOINT/CI-CD/releases/tag/0.1.0
