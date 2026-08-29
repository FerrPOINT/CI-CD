# Libraries — dependency policy Forge CI/CD

> **Статус:** policy-документ. Архитектурный выбор технологий находится в [TECH_CHOICES](TECH_CHOICES.md), фактические версии — в lock-файлах. Этот документ отвечает на вопрос, как добавлять и проверять зависимости.

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
| Contracts/serialization | `serde`, `serde_json`, `serde_yaml`, `utoipa` | DTO и OpenAPI обновляются вместе с implementation/tests. |
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

## 6. Planned candidates

Кандидаты из [TECH_CHOICES](TECH_CHOICES.md#3-target-candidates-by-phase) не добавляются заранее. Для каждого кандидата перед merge нужны:

- причина, почему существующий код/зависимость недостаточны;
- feature flags и минимальный набор enabled features;
- focused test на позитивный и негативный сценарий;
- обновление docs/ADR, если меняется архитектурная граница;
- проверка lockfile diff и transitive dependencies.

## 7. Запрещённые паттерны

- Shelling out из API/server для production execution.
- Dependency, которая требует broad filesystem/network access без изоляции в infra adapter.
- Новый глобальный singleton client без lifecycle/shutdown ownership.
- Unbounded parser или archive extractor без лимитов размера, глубины и времени.
- UI package, который добавляет собственную тему/палитру поверх project tokens.
