# AGENTS.md — Forge CI/CD

## Docker и Kubernetes

Поддерживаются обычный Docker/Compose без Kubernetes и Kubernetes. Общие API,
данные, настройки и правила исполнения сохраняют одинаковый смысл в обоих
режимах; особенности оркестратора принадлежат deployment-адаптерам.
Перед изменением runtime/config/storage/runner читать [стандарт Base](https://github.com/FerrPOINT/services-base/blob/29a709834fba54a823873b82050731333d7e3d97/docs/REPO_STANDARDS.md#поддержка-docker-и-kubernetes).
Затронутый сценарий проверять отдельно в обоих режимах на точном кандидате;
непроверенный режим оставлять открытым gate. Docs-only правки не требуют deploy
или пересборки. Ссылка фиксирует решение Base, не меняя SDK/UI/skills pins.

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
4. Frontend использует React, Tailwind и публичные компоненты Base; новые API-клиенты и DTO остаются типизированными. Vite dev proxy направляет `/api` на `http://localhost:22801`.
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
Работа с UI описана в разделе «Разработка UI со справочником Base» ниже.
Не переносите продуктовые запросы, DTO и права в общий UI. SDK/UI/skills pins независимы; новое имя платформы не меняет SDLC IDs/scopes или Hermes upstream.

## Общие поля выбора Base

Сначала искать общий UI в Base: search → show → example. Общие исправления
вносить в Base; продукт передаёт данные, права, преобразования и сохранение.
Для одного известного значения использовать Select, для нескольких — MultiSelect,
для каталога с поиском/пагинацией — SearchableSelect из публичного @pdlc/ui.
Контракт Select/MultiSelect: options, value/defaultValue, onValueChange;
HTML children/option/onChange/multiple/size больше не являются их API.
Видимые raw select и локальные копии оформления не добавлять. Технический form
bridge принадлежит только Base; SSR использует штатный macro и choice-runtime,
явный refresh после программных value/options и native fallback до успешного init.
Правила выбора и подключения: Base docs/platform/CHOICE_CONTROLS.md.
UI/SDK/skills pins независимы; установка и production — отдельные операции.

## Разработка UI со справочником Base

Перед созданием или изменением UI читать [маршрут разработки со справочником Base](https://github.com/FerrPOINT/services-base/blob/ae2b41291eee5af752daff89f760fba5aff41783/base-modules/ui/reference/guides/agent.md).
Из корня продукта с подготовленным frontend: `pnpm --dir frontend exec pdlc-ui`.
Сначала сверить `.base-ui-revision` и выполнить `check` один раз в начале задачи
или после смены UI pin; затем `search "задача"` либо адресный `show`, при
необходимости `--section props` или `--section setup`, и один `example`. Использовать точный
квалифицированный адрес из поиска; композиции доступны через `example recipes`.
Без подготовленного frontend руководство описывает CLI из отдельного checkout
того же UI SHA без npm install. Не загружать весь каталог в контекст и не
подменять принятую версию через npx/dlx/latest или скрытый HTTP fallback.
Проверить providers/Router/i18n/CSS и cleanup; после интеграции — scoped проверки
продукта и IAB затронутого поведения. Источники, hosted CI и runtime — разные gates.

Button, Input, Textarea, Checkbox, RadioGroup/Radio, Switch, меню, Card, Table, Disclosure,
Dialog и Toaster/toast использовать из публичного @pdlc/ui. Общий дефект
исправлять в Base; продукт сохраняет данные, права и сохранение. Прямой Sonner,
копии примитивов и продуктовые определения общих tokens не добавлять.
Брендинг применяется общим runtime; draft preview изолируется BrandingScope.
Канонический контракт — Base docs/platform/DESIGN_SYSTEM.md и docs/BRANDING.md.
UI/SDK/skills pins независимы; source-изменения не означают установку.
