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

## Проверка завершения и отдельный release gate

- [ ] Изменения соответствуют существующей архитектуре и не затёрли чужую работу.
- [ ] Пройдены scoped unit/static затронутых компонентов по `.quality-gate.json`; подходящие проверенные outputs/receipts переиспользованы.
- [ ] На интегрированной вехе проверен один application artifact. Config/build контейнеров выполняются при изменении соответствующих inputs, без повторной компиляции неизменного кода.
- [ ] Для релиза отдельно подтверждены полный backend/frontend набор, integration/slow/coverage и необходимые security/data/live gates на exact candidate; невыполненные проверки обозначены NOT RUN.
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

## Быстрые проверки и сборка

Перед тестами, build и CI читать обязательный [стандарт Base](https://github.com/FerrPOINT/services-base/blob/197ead9d9b6cf6901e8f9dd31f43f719885502c3/docs/BUILD_PERFORMANCE.md).
При разработке и сборке unit/static. Integration/slow/full coverage —
ручной gate при подготовке релиза по docs/TESTING.md.
Сохранять полезный cache, не переустанавливать неизменные dependencies и не
пересобирать приложение из-за test-only. До повторного тяжёлого запуска назвать
изменившиеся inputs/недостающее evidence; передавать один artifact между этапами.
Required security/data/compatibility/live checks и product toolchains сохраняются.
Компоненты и команды: `.quality-gate.json`; общие правила не копировать.

Уровни и команды: [TESTING](docs/TESTING.md). Полный `.github/workflows/release.yml`
запускается вручную на exact ref; зелёный unit build не означает release acceptance.

Root candidate producer может переиспользовать локальный public Compose 5.5.1
artifact через named context `forge-compose` и `FORGE_COMPOSE_SOURCE=preloaded`.
Он закреплён repository digest; recipe проверяет официальный SHA-256 до исполнения.
Самостоятельный build сохраняет default download с ограничением ожидания. Команда
подготовки — Base `scripts/compose_tool.py`; SDK/runtime/operator pins не меняются.

## Выбор сборки и повторное использование

Перед build/test/install определить нужные компоненты и изменившиеся реальные
inputs; проверить готовый artifact, environment и cache. Неизменные inputs и
проверенный artifact означают отсутствие повторных install/compile/source export.
Docs/test-only не пересобирают приложение без изменения его build inputs.
Во время работы выполнять scoped unit/static; полный gate — на согласованной
вехе. Integration/slow/full coverage — вручную перед релизом; необходимые
security/data/compatibility/live gates сохраняются. Полные команды в этом
документе — каталог проверок, а не требование выполнять всё после каждой правки.
Перед повторным heavy назвать changed inputs или missing proof; REUSED не
выдавать за новый тест. Полезный прогретый cache сохранять в его бюджете.
В source workspace читать `docs/BUILD_PERFORMANCE.md` из `qualityTooling.path`
в `../source-workspace.json` и [правила workspace](../AGENTS.md).
В standalone checkout использовать стандарт принятой версии Base.
