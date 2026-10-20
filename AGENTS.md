# AGENTS.md — Forge CI/CD

## Репозиторий

- **Путь**: `/opt/dev/CI-CD`.
- **GitHub**: `git@github.com:FerrPOINT/CI-CD.git`.
- **Назначение**: self-hosted CI/CD control plane. В MVP jobs выполняет embedded runner (Docker/shell) с `job_queue` + `job_leases` ledger/reconciliation; внешний runner protocol MVP покрывает register/heartbeat/poll с bounded `waitSeconds` wakeup через in-process signal + PostgreSQL `LISTEN/NOTIFY`/ack/renew/control/`secrets:resolve`/artifact upload/logs/complete, basic runner tag + executor capability matching, ack-timeout requeue, queue-timeout diagnostic для отсутствующего compatible runner-а, cancel signal delivery и stale-runner offline reconciliation, а `forge-runner` даёт отдельный shell-runner process поверх этого protocol с active-lease heartbeat. Production-grade dispatch policy/fairness, richer log chunks, resumable artifact sessions и sandbox hardening остаются target.
- **Стек**: Rust 2024, Axum 0.8, SQLx 0.8, PostgreSQL 17; React 19, Vite 6, Tailwind CSS 4, shadcn/ui.
- **Env prefix**: `CICD_`.
- **Порты**: API `22801`, Dashboard `22802`, PostgreSQL `22543`.

## Правила работы с репозиторием

1. Работать из `/opt/dev/CI-CD`; перед изменениями читать `docs/ARCHITECTURE.md`, `docs/DATA_MODEL.md`, `docs/API.md` и релевантный ADR.
2. До начала проверять `git status`. Не отменять и не перезаписывать чужие изменения; при неожиданном изменении во время работы остановиться и запросить указания.
3. Backend сохраняет слои `api -> domain -> store`. `JobStatus::transition_to()` — единственный источник правил перехода статусов. SQL-запросы параметризуются через SQLx.
4. Frontend использует React, Tailwind и shadcn/ui; новые API-клиенты и DTO остаются типизированными. Vite dev proxy направляет `/api` на `http://localhost:22801`.
5. Все секреты передаются только через `CICD_` переменные или secret manager. Не коммитить `.env`, токены, пароли и реальные данные.

## Коммиты и документация

- Использовать Conventional Commits: `feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`, `perf:`.
- Один коммит — одна логическая единица. Не выполнять amend/squash или push без явного запроса пользователя.
- При изменении API обновлять `docs/API.md`; при изменении схемы — `docs/DATA_MODEL.md`; архитектурное решение фиксировать новым ADR в `docs/adr/`.
- Изменения Git Smart HTTP, bare repository storage или post-receive hook синхронизировать с `docs/GIT_HOSTING.md`.
- Документация проекта и пользовательские строки — на русском; код и комментарии — на английском согласно `docs/CODE_STYLE.md`.

## Docker и команды проверки

```bash
# Запуск и управление локальным окружением.
cp .env.example .env
docker compose up --build -d
docker compose ps
docker compose logs -f
docker compose down

# Backend: cargo test в закреплённой среде Rust 1.88.0.
# Контейнерный запуск разрешён только через временный Compose-проект
# с owner/purpose и cleanup по правилам Base deploy/LOCAL_GROUPS.md.

# Frontend.
cd frontend && pnpm test
cd frontend && pnpm build
```

Для полного backend gate использовать также `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` и `cargo build --release --workspace` в том же Rust-контейнере. `justfile` дублирует основные операции: `just up`, `just down`, `just test-backend`, `just test-frontend`, `just build-frontend`, `just health`.

Не использовать `docker compose restart` для применения нового образа или env: пересоздавать сервисы через `docker compose up -d` либо `docker compose up --build -d`.

## Checklist перед завершением

- [ ] Изменения соответствуют существующей архитектуре и не затёрли чужую работу.
- [ ] Backend-проверки проходят: fmt, clippy, test, release build.
- [ ] Frontend-проверки проходят: `pnpm openapi:check`, `pnpm openapi:compat --base-ref origin/main`, `pnpm test`, `pnpm build`.
- [ ] `docker compose config` и `docker compose build` успешны при изменении контейнеров.
- [ ] Для новых endpoint выполнена curl-проверка; для UI-изменений сделаны актуальные screenshots при необходимости.
- [ ] Документация, `.env.example` и ADR обновлены, если изменились контракт, конфигурация или решение.
- [ ] В diff отсутствуют credentials, токены, `.env` и другие чувствительные данные.

## References

- `docs/ARCHITECTURE.md`
- `docs/CODE_STYLE.md`
- `docs/TESTING.md`
- `docs/API.md`
- `docs/DATA_MODEL.md`
- `docs/CI_CD.md`

## Base и воспроизводимость

См. [BASE_INTEGRATION](docs/BASE_INTEGRATION.md). Обязательны pinned Base SHA,
locked/frozen зависимости и проверка актуальных checkout до сборки.
Rust build toolchain 1.88.0, Node 22.20.0, pnpm 10.28.1; MSRV отдельно.

## Локальные Docker jobs и helpers

Любой контейнер запускается настоящим Compose. Постоянные проекты только
sdlc1, sdlc2 и sdlc-common; временные sdlc-qa-/sdlc-build- имеют owner/purpose,
явный endpoint и scoped cleanup в finally. Прямые docker run/create, поддельные
labels и общий prune запрещены. Rootless runner проверяет TLS и daemon ID без
Desktop fallback. Pulse — приватный Git/pipeline только в CI/CD sdlc1.
Подробности: [Compose runner](docs/COMPOSE_RUNNER.md).

## Термины PDLC и общие компоненты

Перед изменением платформенных имён прочитайте [интеграцию с Base](docs/PDLC_BASE_INTEGRATION.md) и канонические TERMS/BRANDING.
Для UI: `pnpm --dir frontend exec pdlc-ui search "задача"` → `show ExportName` → один нужный `example`. В Workflow SSR используйте штатный exporter Base.
Не переносите продуктовые запросы, DTO и права в общий UI. SDK/UI/skills pins независимы; новое имя платформы не меняет SDLC IDs/scopes или Hermes upstream.

## Единое пространство PDLC

[Согласованный план](docs/plans/unified-spaces-20261019.md) вводит M:N и общий read admission. До полного
совместимого cohort это цель, не установленное поведение. Строгие V1/V2 и
applied migrations сохранять; ProjectRef/SpaceRef/V3 отдельны. Полный gate на
интегрированной вехе, в ходе реализации scoped checks. Runtime не активировать
по готовности одного продукта; не выбирать первый Project/Space неявно.
Git deadline передавать как private proof с исходным fixed clock до native IO
и ref preparation; не заменять его новой длительностью. Не освобождать lifecycle
lease до завершения native owner/cleanup. Custom hooks сохранять; unknown Git
publication требует exact ref readback без автоматического retry.
REST identity proof не сводить к claims до выдачи защищённых данных. Общий
допуск каталогов применять до count/page по owner ID, не по именам. Пустой
selected не превращать в unrestricted SQL NULL; local admin не visibility bypass.
Typed metadata accessor не считать human authorization: HTTP adapter передаёт
admitted filter. При изменении handler signature искать все внутренние callers
по всему backend; не передавать им фиктивный HTTP request ради обхода границы.

Read adapters должны разрешать framework-decoded owner params одинаково для ACL
и данных. До общего допуска читать только owner identity; kind/readiness также
защищены. Common visibility заменяет прежнее read membership, но не scopes,
project binding credential или права изменения. Read proof не является write
permit: PR mutation/merge требуют собственной effect/deadline границы.

PR write permit должен оставаться у typed/detached owner до SQL commit/native
ref preparation. После SQL wait повторно проверять исходный proof; роль/lifecycle
держать под существующими locks. Counter и создание PR атомарны. Timeout после
возможной публикации — unknown outcome с exact ref readback, без auto retry,
отката refs по предположению или признания PR status доказательством Git rollback.

Task-link replay не обходит текущий допуск: при активной политике проверять
write permit/роль/lifecycle до stored readback. Новая связь и проверки исходного
proof после SQL wait входят в одну транзакцию; прежний source commit при replay
не переснимать. OFF сохраняет исторический original-key readback.

При native timeout дать Git ограниченное время на собственный signal cleanup
ref locks перед SIGKILL остальной owned group. Это не продлевает proof. Сохранять
unknown outcome; не удалять locks по предположению. Fixture custom hook обязан
использовать фактический core.hooksPath и доказывать достижение hook.
