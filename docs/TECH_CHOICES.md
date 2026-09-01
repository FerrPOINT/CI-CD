# Выбор технологий и библиотек — Forge CI/CD

> **Статус:** архитектурный справочник. Фактические версии фиксируются в `backend/Cargo.toml`, `backend/Cargo.lock`, `frontend/package.json` и `frontend/pnpm-lock.yaml`. Новая зависимость не становится разрешённой только из-за упоминания здесь: её добавляет отдельный reviewable change с тестами.
> Проверено: `2026-09-01`.

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
| Pipeline DSL parser | `serde_yaml`-compatible API через пакет `yaml_serde` 0.10 | Current verified | Используется для текущего `.forge-ci.yml` и OpenAPI YAML dump без deprecated `serde_yaml`/`unsafe-libyaml` в resolved graph. Новые parser-capability требуют отдельного TDD-среза с diagnostics/limits. |
| Git storage | bare repositories + Smart HTTP + `git2` для локальных операций | Current verified | Достаточно для встроенного Git цикла MVP; полная code-review платформа не является целью. |
| Execution MVP | Embedded runner через Docker CLI или host shell | Current verified transitional | Полезно для локального MVP, но не является production execution boundary. |
| Secrets | `aes-gcm`, `argon2`, `hmac`, `sha2`, `subtle`, `jsonwebtoken` 9 | Current verified | Закрывает текущее хранение секретов, пароли, PAT/JWT и HMAC webhook signing. |
| Frontend | React 19 + Vite 6 + Tailwind 4 + shadcn/Radix primitives | Current verified | Рабочий SPA dashboard с typed API boundary, i18n и плотными операционными страницами. |

## 3. Target candidates by phase

| Кандидат | Где использовать | Фаза | Решение |
|---|---|---|---|
| [`object_store`](https://docs.rs/object_store/latest/object_store/) | Artifact backend: local FS сейчас, затем S3/GCS/Azure-compatible adapter | Phase 4 | Предпочтительный кандидат. Даёт единый async API и conditional/object-store semantics без раскрытия provider в domain. |
| [Apache OpenDAL](https://opendal.apache.org/docs/rust/opendal_service_s3/index.html) | Альтернатива artifact/object-storage adapter, особенно при широкой матрице storage services | Phase 4 evaluation | Рассматривать рядом с `object_store`; выбирать по feature set, MSRV, зависимостям и нужным provider-ам, не подключать оба без измеренной причины. |
| [`bollard`](https://docs.rs/bollard/latest/bollard/) | Docker executor внутри отдельного `forge-runner` process | Phase 3 | Кандидат вместо shell-вызовов `docker`. Не добавлять в API/server crate. |
| [`cargo-nextest`](https://nexte.st/) | Rust test runner для CI/local, JUnit/reporting и per-test isolation | Quality hardening | Хороший кандидат после разметки serial/heavy real-DB tests; не заменяет текущий `cargo test` для doc-tests и простого baseline до настройки профилей. |
| [`cargo-audit`](https://crates.io/crates/cargo-audit) + [`cargo-deny`](https://github.com/embarkstudios/cargo-deny) | Dependency advisories, licenses, bans, duplicate crates, source policy | Security/release hardening | `cargo audit --ignore RUSTSEC-2023-0071` уже current CI gate вместе с SQLx optional MySQL/RSA feature guard; следующим шагом добавить `cargo-deny` с documented exceptions для license/source/ban policy. |
| [`cargo-chef`](https://github.com/LukeMathWalker/cargo-chef) + [`sccache`](https://github.com/mozilla/sccache) | Ускорение Docker/CI Rust builds | Build hardening | Операционное улучшение, не runtime-dependency. Добавлять после baseline CI, с тем же Rust version во всех build stages. |
| [`serde-saphyr`](https://crates.io/crates/serde-saphyr) / [`yaml-rust2`](https://crates.io/crates/yaml-rust2) / parser wrapper поверх `yaml_serde` | Parser hardening для `.forge-ci.yml` | Phase 1 follow-up | Нужен parser contract: duplicate keys, aliases/anchors, unknown keys, size/depth limits, line/column diagnostics и fixture parity для legacy/v1 DSL. |
| [`apalis-postgres`](https://docs.rs/apalis-postgres/latest/apalis_postgres/) | Generic background jobs: notifications, cleanup, housekeeping | Automation evaluation | Может заменить часть generic workers. Core `job_queue`/runner leases не переносить, пока framework не сохраняет наши invariants: attempts, fencing, compatibility claim, cancel и audit. |
| [Testcontainers for Rust](https://rust.testcontainers.org/) | Programmatic integration/smoke fixtures для PostgreSQL/object storage/runner tests | Quality hardening | Кандидат для локального/CI harness после стабилизации Docker на dev-машинах; текущий GitHub Actions service остаётся baseline. |
| `tracing-opentelemetry` + `opentelemetry-otlp` | Distributed traces и export в observability backend | Phase E/operations | Добавлять после появления стабильных spans, metrics names и privacy policy. |
| Cron parser / [`tokio-cron-scheduler`](https://docs.rs/tokio-cron-scheduler/latest/tokio_cron_scheduler/) | Scheduler semantics | Phase automation hardening | Current MVP использует локальный strict 5-field UTC parser без новой зависимости; external crate рассмотреть для IANA timezone/DST, но dedup, claim и missed-fire recovery остаются в PostgreSQL. |
| [`tower-sessions`](https://docs.rs/tower-sessions/latest/tower_sessions/) | Cookie sessions для UI | Auth hardening | Рассмотреть только если browser-session policy переходит от stateless JWT к server-side session store. PAT остаются отдельной credential class. |
| [`async-nats`](https://docs.rs/async-nats/latest/async_nats/) / JetStream | Event wakeup, fanout или high-throughput delivery | Future | Не baseline. Добавлять только после load evidence, когда Postgres queue/outbox упирается в измеримый предел. |
| [`lapin`](https://docs.rs/lapin/latest/lapin/) / RabbitMQ | AMQP integration для существующей инфраструктуры заказчика | Future | Не baseline; выбирать вместо NATS только при внешнем AMQP-стандарте. |
| `kube` | Kubernetes executor adapter | Phase 5 | Добавлять за общим `ExecutionBackend` port после Docker runner protocol. |
| `gix` | Чисто Rust Git operations | Future | Не нужен в MVP. Рассмотреть, если `git2`/system Git станут ограничением для portability или security hardening. |
| [`cedar-policy`](https://docs.rs/cedar-policy/latest/cedar_policy/) | Fine-grained authorization policy engine | Future auth/tenant | Не baseline. Рассматривать только когда project RBAC/tenant policy перестанет помещаться в простую DB-backed модель. |

## 4. Что не добавляем в базовое приложение

- Отдельный broker до доказанной нагрузки: PostgreSQL queue, leases и outbox проще, проверяемее и соответствуют ADR-0004/0006.
- Полноценный workflow engine вместо собственного pipeline planner: Forge должен хранить свой immutable plan, attempts, policy snapshot и audit.
- Внешний background-job framework для core runner dispatch до доказанной совместимости с `job_queue`, attempts, leases, fencing, cancel и audit.
- Политический движок уровня Cedar/OPA в baseline auth: текущий project RBAC остаётся проще и проверяемее до появления tenant/ABAC требований.
- Считать совместимый `yaml_serde` swap завершённым parser hardening без parser contract и regression fixtures.
- Kubernetes executor до Docker runner protocol: иначе появятся две границы исполнения до стабилизации одной.
- General-purpose admin portal: экран появляется только вместе с реальным workflow управления instance/tenant/policy.
- Host shell execution в production: допустим только как local development adapter.

## 5. Готовые CI/CD решения на Rust

| Решение | Что даёт | Использование для Forge |
|---|---|---|
| [Pipelight](https://github.com/crocuda/pipelight) | Lightweight Rust CI/CD CLI с pipeline definition и triggers | Можно изучать CLI ergonomics и local pipeline execution, но не использовать как control plane. |
| [WRKFLW](https://github.com/bahdotsh/wrkflw) | Локальный запуск/валидация GitHub Actions/GitLab pipelines | Полезен как reference для local workflow validation; не заменяет Forge runtime model. |
| [Fluent CI](https://docs.fluentci.io/) | Local-first pipelines, registry/prebuilt pipelines, Web UI, Dagger/Wasm execution | Полезен как benchmark UX/integrations; не является прямой базой для self-hosted Forge control plane. |

На сентябрь 2026 нет очевидного зрелого Rust-продукта, который можно взять как готовую замену GitLab/Jenkins и одновременно сохранить требования Forge: встроенный Git, RBAC, audit, execution attempts, leases, artifacts, OpenAPI и self-hosted deployment. Поэтому правильная стратегия — не искать монолитную замену, а брать зрелые crates по инфраструктурным границам.

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
