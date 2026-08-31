# Third-party components and SBOM policy

> **Статус:** инвентарь current dependencies + Target approved policy генерации SBOM. Полный состав транзитивных компонентов берётся из lock-файлов, а не из этой сводки.
> Основание: [ADR-0009](adr/0009-canonical-registry.md), [DEPENDABOT](../.github/dependabot.yml), [RISK_REGISTER](RISK_REGISTER.md).

Этот документ описывает только сторонние компоненты и их лицензии. First-party код, документация, конфигурация и изменения FerrPOINT закрыты лицензией [FerrPOINT Proprietary Source-Available Evaluation License v1.0](../LICENSE).

## 1. Инвентарь прямых компонентов

Версии — зафиксированные constraints манифестов; точные resolved версии — `backend/Cargo.lock` и `frontend/pnpm-lock.yaml`.

| Контур | Компонент | Назначение | Лицензия (ожидаемая) | Критичность |
|---|---|---|---|---|
| Rust runtime | axum, tower, tower-http | HTTP/control plane | MIT | security-relevant |
| Rust runtime | tokio | async runtime/processes | MIT | critical runtime |
| Rust runtime | sqlx | PostgreSQL access/migrations | MIT OR Apache-2.0 | data boundary |
| Rust runtime | serde, serde_json, serde_yaml | API/DSL parsing | MIT OR Apache-2.0 | parser |
| Rust runtime | uuid, chrono | IDs/time | MIT OR Apache-2.0 | data integrity |
| Rust runtime | aes-gcm | secrets encryption at rest | Apache-2.0 OR MIT | cryptography |
| Rust target | argon2, jsonwebtoken, hmac | passwords/JWT/signatures | MIT OR Apache-2.0 | authentication/crypto |
| Frontend | react, react-dom | UI runtime | MIT | client runtime |
| Frontend | react-router | routing | MIT | navigation |
| Frontend | @tanstack/react-query | server-state cache | MIT | data consistency |
| Frontend | i18next, react-i18next | localisation | MIT | user-facing |
| Frontend | radix/shadcn primitives | accessible UI primitives | MIT | accessibility |
| Frontend | vite, typescript, vitest | build/test | MIT | build supply chain |
| Infrastructure | postgres:17.6-alpine | primary database | PostgreSQL License | critical data store |
| Infrastructure | nginx:1.27-alpine | SPA delivery/proxy | BSD-2-Clause | ingress |
| Toolchain | rust:1.86-bookworm | reproducible Rust build | Apache-2.0/MIT ecosystem | build |
| Toolchain | node:22-bookworm-slim | frontend build | MIT ecosystem | build |

Перед добавлением нового direct dependency владелец PR обязан проверить resolved license и advisory-status, а не доверять этой таблице.

## 2. Политика лицензий

- **Разрешены по умолчанию:** MIT, Apache-2.0, BSD-2-Clause/3-Clause, ISC, PostgreSQL License и их совместимые permissive-лицензии.
- **Требуют review tech lead:** MPL-2.0, LGPL, dual-license без очевидного permissive варианта, commercial/free tier terms.
- **Запрещены для linking/distribution без отдельного юридического решения:** GPL-* и AGPL-*; неизвестная/неопределённая лицензия.
- Новый компонент без доказанного SPDX license identifier не мержится. Исключение фиксируется отдельным ADR с причиной, distribution model и владельцем.
- License text и copyright notices для распространяемых бинарников собираются в release evidence (target).

## 3. Обновление и vulnerability management

1. Dependabot проверяет Cargo и npm weekly (`.github/dependabot.yml`).
2. Security advisory в crypto/auth/parser/HTTP/SQL компоненте: triage в течение 24 часов, patch или documented mitigation до следующего релиза.
3. SemVer-major обновление требует: compatibility review, changelog, тесты и, если меняется внешний контракт, API/ADR/RTM.
4. Lock-файлы коммитятся вместе с manifest; ручное редактирование lockfile запрещено.
5. Перед релизом: dependency update report, `cargo audit`/эквивалент и npm audit с разбором accepted findings (Target approved CI gate).

## 4. SBOM policy

**Target approved:** каждый release публикует CycloneDX SBOM (JSON) как release artifact. Формат выбран для security use case; SPDX допускается дополнительно для license/compliance потребителей.

Минимальный набор записи соответствует CISA SBOM Minimum Elements: supplier/author, component name, version, unique identifier (purl/CPE где применимо), hashes, dependency relationship, timestamp и SBOM author.

| Шаг | Инструмент / evidence | Статус |
|---|---|---|
| Rust SBOM | `cargo-cyclonedx` по Cargo.lock | Target approved |
| Frontend SBOM | CycloneDX npm/pnpm generator по pnpm-lock.yaml | Target approved |
| Merge | один root BOM с services (backend/frontend) и infrastructure inventory | Target approved |
| Validate | CycloneDX schema validation + diff нового release | Target approved |
| Publish | GitHub Release asset + checksum | Target approved |

SBOM не включает секреты, private repository URLs, токены или значения `.env`.

## 5. Проверяемые источники

- Rust direct dependencies: `backend/Cargo.toml`; resolved graph: `backend/Cargo.lock`.
- Frontend direct dependencies: `frontend/package.json`; resolved graph: `frontend/pnpm-lock.yaml`.
- Images/toolchains: Dockerfiles и `docker-compose.yml`.
- Security choices: `TECH_CHOICES.md`, `contracts/AUTHZ_CONTRACT.md`, `contracts/DATA_LIFECYCLE.md`.
