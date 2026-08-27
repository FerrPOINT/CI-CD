# Changelog

Все значимые изменения в этом проекте документируются в этом файле.

Формат основан на [Keep a Changelog](https://keepachangelog.com/ru/1.1.0/),
и этот проект стремится соответствовать [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Дата release-записей: формат ISO 8601 (`YYYY-MM-DD`).

## [Unreleased]

### Added

- ADR-0008 реализован: `backend/migrations/` (baseline `0001_bootstrap_v1.sql` verbatim из startup bootstrap, `0002_runtime_role.sql` grants forge_runtime), crate `cicd-migrate` (dry-run/apply/verify, advisory lock `FORGE`), server стартует через embedded `sqlx::migrate!`; проверено на живой test-compose БД (apply → идемпотентный повтор).
- Тестовый контур: cicd-migrate прогоняется в docker-сети test-compose.

- ADR/API_CONTRACT реализованы частично (Current): utoipa-аннотации на контроллерах core-групп (health/projects/pipelines/jobs+logs), `GET /api/v1/openapi.json`, канонический артефакт `openapi/openapi.yaml` (546 строк, OpenAPI 3.1) через `cargo run --bin openapi-dump`, CI drift-гейт `diff` против коммита; фронт: `openapi-typescript` генерация `src/api/schema.d.ts` (`pnpm openapi:generate`), гейт `pnpm openapi:check`, DTO Project/Pipeline/Job/Stage переведены на generated-типы. Platform-группа (runners/secrets/…) — следующий шаг.

### Changed

- `git_host.rs`: `unwrap()` в тестах заменены на `expect` (прод-код unwrap-free и был).

### Added

- SDLC-набор по отраслевым стандартам (ISO/IEC/IEEE 12207/15289, IEEE 829, ASVS 4.0, CISA SBOM 2026): TEST_PLAN (уровни тестирования, SEV1–SEV4, coverage-политика), TRACEABILITY/RTM (REQ-ID ↔ контракты ↔ тесты ↔ evidence, 25 capability + NFR), THREAT_MODEL (STRIDE по границам доверия, маппинг на контракты), RISK_REGISTER, DISASTER_RECOVERY (tiers/RTO/RPO/3-2-1-1/дриллы), INCIDENT_RESPONSE (SEV-матрица, постмортемы, security-инциденты), THIRD_PARTY (инвентарь, license-политика, CycloneDX SBOM target), ACCESSIBILITY (WCAG 2.2 AA программа), SLO (SLI/SLO/error budget), METRICS (DORA + runtime).
- PRODUCT_REQUIREMENTS: все capability и NFR получили REQ-ID/NFR-ID; RTM-строка обязательна в PR.
- CODEOWNERS, .well-known/security.txt.

- Канонический реестр имён и authority matrix: ADR-0009; устранены конфликты migration path, outbox-имён и runner namespace.
- Нормативные контракты `docs/contracts/` (API, AUTHZ, RUNNER_PROTOCOL, PIPELINE_DSL, EVENT, DATA_LIFECYCLE, MIGRATION, UI_API) и narrative-слой `docs/architecture/` с sequence-флоу и transition map.
- Документация по аудиториям: USER_GUIDE, DEVELOPMENT_GUIDE, OPERATIONS, PRODUCT_REQUIREMENTS; CURRENT_STATE и DOCUMENTATION_GOVERNANCE; `scripts/verify_docs.py` (ссылки/канон/статусы/дубликаты скринов).
- Public repo surface: LICENSE (MIT), CONTRIBUTING, SECURITY (NOT production-safe предупреждение), SUPPORT, issue/PR-шаблоны, Dependabot.
- UI: мобильная навигация-drawer, карточные layout-ы (runners/users/tokens/environments), доступные confirm-диалоги вместо `window.confirm`, страница pull-запроса с «Посмотреть изменения», живые метрики дашборда.
- Evidence pipeline: deterministic seed (`pnpm seed:evidence`) и воспроизводимые скриншоты (`pnpm shoot:evidence`), реестр `docs/assets/screens/manifest.md` (26 скринов на живых данных).

### Changed

- README переписан: статус/границы доверия, capability matrix, тур по аудиториям.
- 44 legacy-дока консолидированы в гайды/контракты; оставлены redirect-stub-ы на один release-цикл.
- PostgreSQL в docker-compose публикуется только на 127.0.0.1; `CICD_RUNNER_MODE` пробрасывается в backend.
- Скриншоты пересняты единым прогоном: desktop 1920×1080, mobile 375×812, реальные пайплайны/PR/артефакты.

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
