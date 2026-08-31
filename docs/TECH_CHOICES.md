# Выбор технологий и библиотек — Forge CI/CD

> **Статус:** архитектурный справочник. Фактические версии фиксируются в `backend/Cargo.toml`, `backend/Cargo.lock`, `frontend/package.json` и `frontend/pnpm-lock.yaml`. Новая зависимость не становится разрешённой только из-за упоминания здесь: её добавляет отдельный reviewable change с тестами.
> Проверено: `2026-08-28`.

## 1. Принципы выбора

- Базовое приложение остаётся self-hosted CI/CD control plane, а не универсальной платформой для всех инженерных процессов.
- PostgreSQL остаётся первым источником истины и первой очередью до измеренного требования к отдельному broker.
- API/control-plane не должен исполнять пользовательские команды и не должен иметь Docker socket в production.
- Готовая библиотека выбирается, когда она закрывает инфраструктурную область лучше локального кода: HTTP, SQL, OpenAPI, object storage, Docker API, telemetry.
- Готовый продукт не подменяет ядро Forge, если он не даёт control plane, audit, RBAC, leases, attempts, artifacts и Git lifecycle в нашей модели.

## 2. Current verified stack

| Область | Решение | Статус | Почему |
|---|---|---|---|
| HTTP API | Axum 0.8 + Tokio 1 + Tower HTTP | Current verified | Небольшой async API, middleware ecosystem, понятные extractors, простой OpenAPI слой. |
| Persistence | PostgreSQL 17 + SQLx 0.8 | Current verified | Compile-time-friendly SQL, async pool, migrations, явные запросы без ORM magic. |
| API contract | utoipa 5 + committed `openapi/openapi.yaml` | Current verified | Code-first спецификация рядом с Rust DTO и drift gate для frontend types. |
| Git storage | bare repositories + Smart HTTP + `git2` для локальных операций | Current verified | Достаточно для встроенного Git цикла MVP; полная code-review платформа не является целью. |
| Execution MVP | Embedded runner через Docker CLI или host shell | Current verified transitional | Полезно для локального MVP, но не является production execution boundary. |
| Secrets | `aes-gcm`, `argon2`, `hmac`, `sha2`, `subtle`, `jsonwebtoken` 9 | Current verified | Закрывает текущее хранение секретов, пароли, PAT/JWT и HMAC webhook signing. |
| Frontend | React 19 + Vite 6 + Tailwind 4 + shadcn/Radix primitives | Current verified | Рабочий SPA dashboard с typed API boundary, i18n и плотными операционными страницами. |

## 3. Target candidates by phase

| Кандидат | Где использовать | Фаза | Решение |
|---|---|---|---|
| [`object_store`](https://docs.rs/object_store/latest/object_store/) | Artifact backend: local FS сейчас, затем S3/GCS/Azure-compatible adapter | Phase 4 | Предпочтительный кандидат. Даёт единый async API и conditional/object-store semantics без раскрытия provider в domain. |
| [`bollard`](https://docs.rs/bollard/latest/bollard/) | Docker executor внутри отдельного `forge-runner` process | Phase 3 | Кандидат вместо shell-вызовов `docker`. Не добавлять в API/server crate. |
| `tracing-opentelemetry` + `opentelemetry-otlp` | Distributed traces и export в observability backend | Phase E/operations | Добавлять после появления стабильных spans, metrics names и privacy policy. |
| Cron parser / [`tokio-cron-scheduler`](https://docs.rs/tokio-cron-scheduler/latest/tokio_cron_scheduler/) | Scheduler semantics | Phase automation hardening | Current MVP использует локальный strict 5-field UTC parser без новой зависимости; external crate рассмотреть для IANA timezone/DST, но dedup, claim и missed-fire recovery остаются в PostgreSQL. |
| [`tower-sessions`](https://docs.rs/tower-sessions/latest/tower_sessions/) | Cookie sessions для UI | Auth hardening | Рассмотреть только если browser-session policy переходит от stateless JWT к server-side session store. PAT остаются отдельной credential class. |
| [`async-nats`](https://docs.rs/async-nats/latest/async_nats/) / JetStream | Event wakeup, fanout или high-throughput delivery | Future | Не baseline. Добавлять только после load evidence, когда Postgres queue/outbox упирается в измеримый предел. |
| [`lapin`](https://docs.rs/lapin/latest/lapin/) / RabbitMQ | AMQP integration для существующей инфраструктуры заказчика | Future | Не baseline; выбирать вместо NATS только при внешнем AMQP-стандарте. |
| `kube` | Kubernetes executor adapter | Phase 5 | Добавлять за общим `ExecutionBackend` port после Docker runner protocol. |
| `gix` | Чисто Rust Git operations | Future | Не нужен в MVP. Рассмотреть, если `git2`/system Git станут ограничением для portability или security hardening. |

## 4. Что не добавляем в базовое приложение

- Отдельный broker до доказанной нагрузки: PostgreSQL queue, leases и outbox проще, проверяемее и соответствуют ADR-0004/0006.
- Полноценный workflow engine вместо собственного pipeline planner: Forge должен хранить свой immutable plan, attempts, policy snapshot и audit.
- Kubernetes executor до Docker runner protocol: иначе появятся две границы исполнения до стабилизации одной.
- General-purpose admin portal: экран появляется только вместе с реальным workflow управления instance/tenant/policy.
- Host shell execution в production: допустим только как local development adapter.

## 5. Готовые CI/CD решения на Rust

| Решение | Что даёт | Использование для Forge |
|---|---|---|
| [Pipelight](https://github.com/crocuda/pipelight) | Lightweight Rust CI/CD CLI с pipeline definition и triggers | Можно изучать CLI ergonomics и local pipeline execution, но не использовать как control plane. |
| [WRKFLW](https://github.com/bahdotsh/wrkflw) | Локальный запуск/валидация GitHub Actions/GitLab pipelines | Полезен как reference для local workflow validation; не заменяет Forge runtime model. |
| [Fluent CI](https://docs.fluentci.io/) | Local-first pipelines, registry/prebuilt pipelines, Web UI, Dagger/Wasm execution | Полезен как benchmark UX/integrations; не является прямой базой для self-hosted Forge control plane. |

На август 2026 нет очевидного зрелого Rust-продукта, который можно взять как готовую замену GitLab/Jenkins и одновременно сохранить требования Forge: встроенный Git, RBAC, audit, execution attempts, leases, artifacts, OpenAPI и self-hosted deployment. Поэтому правильная стратегия — не искать монолитную замену, а брать зрелые crates по инфраструктурным границам.

## 6. Upgrade policy

- `rust-version` workspace сейчас `1.86`; dependency upgrade, требующий больший MSRV, сначала меняет runtime policy и CI image.
- Minor/major upgrade backend dependency проверяется через `cargo tree`, `cargo test --workspace`, integration tests для затронутой области и OpenAPI drift.
- Frontend upgrade проверяется через `pnpm install --frozen-lockfile`, `pnpm test`, `pnpm build` и визуальный smoke для изменённых страниц.
- Любая dependency, влияющая на security boundary, storage, auth, execution или public API, требует строки в этом документе либо ADR.

## Related

- [ADR-0001](adr/0001-rust-axum-sqlx.md) — Rust + Axum + SQLx.
- [ADR-0004](adr/0004-postgresql-only.md) — PostgreSQL как primary store.
- [ADR-0006](adr/0006-postgresql-outbox.md) — transactional outbox.
- [ADR-0007](adr/0007-runner-security-boundary.md) — execution boundary.
- [DEVELOPMENT GUIDE](DEVELOPMENT_GUIDE.md) — команды разработки и quality gates.
- [LIBRARIES](LIBRARIES.md) — dependency review policy.
