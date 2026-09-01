# Libraries — dependency policy Forge CI/CD

> **Статус:** policy-документ. Архитектурный выбор технологий находится в [TECH_CHOICES](TECH_CHOICES.md), фактические версии — в lock-файлах. Этот документ отвечает на вопрос, как добавлять и проверять зависимости.
> Последнее dependency review: `2026-09-01`.

## 1. Источники истины

| Поверхность | Файлы | Что считается фактом |
|---|---|---|
| Rust workspace | `backend/Cargo.toml`, `backend/Cargo.lock` | Реальные crates, features, MSRV и transitive graph. |
| Frontend | `frontend/package.json`, `frontend/pnpm-lock.yaml` | Реальные npm packages и scripts. |
| Архитектурный выбор | `docs/TECH_CHOICES.md`, `docs/ADR.md` | Почему библиотека допустима и где её можно применять. |
| Security inventory | `docs/THIRD_PARTY.md`, `docs/assets/sbom.json` | SBOM, license/security review evidence. |

## 2. Правила добавления dependency

- Зависимость должна быть привязана к конкретной границе: API, persistence, runner process, object storage, auth, observability, frontend UI.
- Runtime dependency не добавляется ради одного helper, если стандартная библиотека или уже существующий crate закрывает задачу без ухудшения читаемости.
- Backend dependency не должна повышать `rust-version` выше workspace `1.86` без отдельного решения по CI/runtime image.
- Deprecated/unmaintained direct dependency не используется для новых capabilities. Если dependency уже находится в runtime path, она получает documented debt, replacement candidates и regression plan до production hardening.
- TLS для внешних HTTP-клиентов по умолчанию — `rustls`; OpenSSL допускается только с явной причиной.
- `default-features` отключаются, когда crate тянет лишние runtime, TLS, compression, native или cloud provider features.
- Любая dependency на execution, auth, crypto, storage или network path требует threat-model review и теста на негативный сценарий.
- Frontend package должен быть совместим с Vite/React/Tailwind baseline и не приносить новый state/data-fetching framework без ADR.

## 3. Команды проверки

```bash
cd /opt/dev/CI-CD/backend
cargo tree -e features
cargo tree -d
cargo test --workspace

cd /opt/dev/CI-CD/frontend
pnpm why <package>
pnpm test
pnpm build
```

Для security/release проверки добавляются SBOM refresh, dependency audit и secret scan по правилам [THIRD PARTY](THIRD_PARTY.md) и [DEVELOPMENT GUIDE](DEVELOPMENT_GUIDE.md#target-approved).

## 4. Current Rust dependency groups

| Группа | Crates | Правило применения |
|---|---|---|
| Async runtime/API | `tokio`, `axum`, `tower-http`, `tokio-stream` | Только HTTP/runtime границы; domain crate не зависит от Axum/Tokio. |
| Persistence | `sqlx`, `uuid`, `chrono` | SQL остаётся параметризованным; schema changes идут через versioned migrations. |
| Contracts/serialization | `serde`, `serde_json`, `serde_yaml`, `utoipa` | DTO и OpenAPI обновляются вместе с implementation/tests. `serde_yaml` — compatibility debt: crate deprecated/unmaintained, новые DSL/parser требования должны идти через отдельную миграцию. |
| Git/archives | `git2`, `flate2` | Git hosting и archive helpers; не использовать для обхода auth/RBAC boundary. |
| Auth/crypto | `argon2`, `jsonwebtoken`, `aes-gcm`, `hmac`, `sha2`, `subtle`, `base64`, `rand_core` | Не логировать secret material; rotation/key policy документируется до production use. |
| Errors/logging | `anyhow`, `thiserror`, `tracing`, `tracing-subscriber` | `thiserror` для domain/application errors, `anyhow` только на composition/CLI style boundary. |
| CLI/client | `clap`, `reqwest` | CLI работает только через public HTTP API, без server crate linkage. |

## 5. Current frontend dependency groups

| Группа | Packages | Правило применения |
|---|---|---|
| UI/runtime | `react`, `react-dom`, `react-router` | Feature pages через router, без server runtime. |
| Data | `@tanstack/react-query`, local `api/*` wrapper | Pages используют hooks из `frontend/src/api`, не raw fetch. |
| UI primitives | `@radix-ui/*`, `class-variance-authority`, `tailwind-merge`, `lucide-react`, `sonner` | Controls должны иметь accessible name, стабильные размеры и i18n strings. |
| Styling | `tailwindcss`, `@tailwindcss/vite` | Цвета через theme tokens из `frontend/src/index.css`. |
| I18n | `i18next`, `react-i18next`, `i18next-http-backend` | ru/en ключи в паритете. |
| Tests/build | `vitest`, `@testing-library/*`, `jsdom`, `typescript`, `vite`, `playwright`, `openapi-typescript` | Unit/build current; Playwright пока используется для evidence/smoke и станет CI gate позже. |

## 6. Priority candidates

| Область | Кандидаты | Зачем | Правило ввода |
|---|---|---|---|
| Supply-chain gate | `cargo-audit`, `cargo-deny`, npm audit, secret/container scan | Закрыть advisory/license/source policy и RISK-008 | Начать с advisory/license gate и documented exceptions; deprecated parser debt не прятать ignore-ом без remediation task. |
| Rust CI speed/diagnostics | `cargo-nextest` | Быстрее и удобнее гонять workspace tests, JUnit/reporting, flaky diagnostics | Сначала настроить профили для real-DB tests (`--test-threads=1`, serial/heavy), затем делать CI gate. |
| Rust Docker builds | `cargo-chef`, `sccache` | Сократить время build без runtime-кода | Только build tooling; Rust version должен совпадать во всех stages. |
| YAML parser hardening | `yaml_serde`, `serde-saphyr`, `yaml-rust2` | Уйти от deprecated `serde_yaml`, получить безопасные parser limits/diagnostics | Отдельный TDD-срез: fixture parity, unknown/duplicate keys, anchors/aliases, size/depth limits, line/column diagnostics. |
| Artifact storage | `object_store`, Apache OpenDAL | S3/GCS/Azure/S3-compatible adapter и retention lifecycle | Вводить через storage port, не протаскивать provider-типы в domain/API DTO. |
| Docker/test harness | `bollard`, `testcontainers` | Runner Docker API и disposable integration dependencies | `bollard` только в runner process; `testcontainers` только в tests/harness. |
| Generic workers | `apalis-postgres` | Cleanup/notification/background housekeeping | Не переносить core runner dispatch, пока не доказаны attempts/leases/fencing/cancel/audit invariants. |
| Future policy engine | `cedar-policy` | Fine-grained tenant/ABAC policy | Не baseline; рассматривать после роста auth-модели сверх project RBAC. |

## 7. Planned candidates

Кандидаты из [TECH_CHOICES](TECH_CHOICES.md#3-target-candidates-by-phase) не добавляются заранее. Для каждого кандидата перед merge нужны:

- причина, почему существующий код/зависимость недостаточны;
- feature flags и минимальный набор enabled features;
- focused test на позитивный и негативный сценарий;
- обновление docs/ADR, если меняется архитектурная граница;
- проверка lockfile diff и transitive dependencies.

## 8. Запрещённые паттерны

- Shelling out из API/server для production execution.
- Dependency, которая требует broad filesystem/network access без изоляции в infra adapter.
- Новый глобальный singleton client без lifecycle/shutdown ownership.
- Unbounded parser или archive extractor без лимитов размера, глубины и времени.
- UI package, который добавляет собственную тему/палитру поверх project tokens.
