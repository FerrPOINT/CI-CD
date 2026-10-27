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

Поле с одной иконкой действия использует InputWithAction Base; SearchInput уже
использует его для очистки. Не копировать вокруг Input absolute trigger, padding
и focus-оформление. Значение секрета, состояние раскрытия, права и запись остаются
у Forge; сначала проверить доступность контракта в принятом UI pin.

Самостоятельные иконки поиска, навигации, скачивания и редактирования используют
Button size="icon" с доступным именем. Локальные w-10/p-0, padding, hover/focus
и disabled-скины не добавлять; submit, pending и обработчики остаются у Forge.

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

Свободная строка с локальными подсказками использует AutocompleteInput Base:
options/value/defaultValue/onValueChange(string). Произвольные значения остаются допустимыми;
закрытый Select для этого не подходит. Native input, ref, FormData, reset, popup и клавиатура
принадлежат Base; запросы и сохранение — продукту. Не добавлять list/datalist или локальный
popup.

Сначала искать общий UI в Base: search → show → example. Общие исправления
вносить в Base; продукт передаёт данные, права, преобразования и сохранение.
Для закрытого списка с одним значением — Select; для нескольких — MultiSelect.
Одно значение из всех видимых вариантов — RadioGroup, взаимноисключающие режимы — appearance="segmented".
Вкладки с панелями используют Tabs. Каталог с поиском/пагинацией — SearchableSelect.
Использовать публичный @pdlc/ui и сверять наличие API по точному UI pin.
Контракт Select/MultiSelect: options, value/defaultValue, onValueChange;
HTML children/option/onChange/multiple/size больше не являются их API.
Видимые raw select и локальные копии оформления не добавлять. Технический form
bridge принадлежит только Base; SSR использует штатный macro и choice-runtime,
явный refresh после программных value/options и native fallback до успешного init.
Правила выбора и подключения: Base docs/platform/CHOICE_CONTROLS.md.
UI/SDK/skills pins независимы; установка и production — отдельные операции.

## Разработка UI со справочником Base

Открытая страница справочника может быть новее зависимости продукта: сверять
контракт по принятому UI pin. Если поиск пуст, уточнить запрос/фильтры и
проверить рецепты; отсутствие API проверить отдельно в актуальном Base.
Общий контракт добавлять в Base вместе с metadata и примером, затем явно
обновлять UI pin потребителя. Локальная копия не заменяет эту интеграцию.

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
Dialog, ActionGroup и Toaster/toast использовать из публичного @pdlc/ui. Общий дефект
исправлять в Base; продукт сохраняет данные, права и сохранение. Прямой Sonner,
копии примитивов и продуктовые определения общих tokens не добавлять.
Брендинг применяется общим runtime; draft preview изолируется BrandingScope.
Размеры однострочных контролов и типографика/padding TabsTrigger принадлежат
Base; локальные h-10/min-h-10 и мобильные копии не добавлять. Textarea,
ширина предметной панели, внешний margin и компоновка сохраняют свой контракт.
Это относится и к классам в локальных const: компактное действие задаётся
Button size=icon, обычное — публичным size; justify-start и внешняя раскладка
остаются у продукта. Не скрывать копии метрик в условных className-константах.
Строки явных действий используют ActionGroup (SSR — action_group): перенос и gap
принадлежат Base, внешние margin/width/grid — экрану. Выбор значения использует
RadioGroup/Select; контекст ресурса не превращается в фиктивную строку действий.
Вертикальный список действий использует ActionGroup orientation="vertical";
сначала сверять наличие props в UI pin. Внешняя ширина rail остаётся у Forge.
Канонический контракт — Base docs/platform/DESIGN_SYSTEM.md и docs/BRANDING.md.
UI/SDK/skills pins независимы; source-изменения не означают установку.

Защиту несохранённых правок подключать через UnsavedChangesGuard Base с Data
Router и owner i18n. Guard живёт на уровне владельца черновиков, в том числе
сохраняемых при закрытии редактора. shouldBlockNavigation задаётся сроком жизни
этого состояния: переходы списка/вкладок с сохранённым вводом не требуют
подтверждения, уход из владельца — требует. Не копировать beforeunload/confirm.

В редакторе синхронизации защищаются также pending/unknown save и start;
read-only check без правок не блокирует уход. Сравнивайте нормализованный
namespace, а не сырую query-строку. Смена вкладки и project_scope при прежнем
ресурсе сохраняют ввод. Подтверждение всех правок снимает when; Base продолжает
исходный переход. Из диалога не повторять запросы или idempotency operations.

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

Для PR merge сохранять durable intent до ref publication, затем повторно брать
role/lifecycle/PR SQL locks с исходным proof. MutationGuard берётся до SQL locks
и держится через intent commit и recovery. Prepared attempt не удалять и не
перезапускать по таймеру или старому target OID. Повтор требует текущих прав и
точных source/candidate refs; только metadata recovery, без native retry/ancestry.
[ADR-0038](docs/adr/0038-durable-pull-merge-recovery.md) определяет журнал и rollout.

## Подписи и карточки вариантов выбора

После проверки API в принятом UI pin используйте label или children Checkbox/Radio
для обычной подписи, appearance="card" — для варианта с рамкой и описанием.
Не копируйте label с border/rounded/padding и не оборачивайте такой выбор в
Card asChild. Base владеет hit target, focus, checked/disabled и нативной формой;
продукт — данными, правами и обработчиком. Внешняя grid/flex-компоновка допустима.
SSR использует checkbox_control/radio_control с appearance='card'; отдельный
click-handler или повторное input/change для оформления не нужен.
Контракт и пример: Base docs/platform/CHOICE_CONTROLS.md, pdlc-ui example Checkbox --case cards.


Видимый выбор файла использует `Input type="file"` Base: кнопка выбора,
focus и disabled следуют теме и branding. Native файловый диалог остаётся у ОС;
чтение, проверка содержимого/размера и отправка — у продукта. Скрытый технический
file input за Button допустим; видимый raw input не добавлять. Для обновления
оформления сверять UI pin; CLI/Telegram без web UI не получают новых зависимостей.

Подписи web-полей оформлять публичным Label; подпись/подсказку/ошибку одного
поля — FormField Base. Продукт сохраняет текст, htmlFor/id, required, данные
и обработчики; не копировать типографику подписи и фон/рамку/радиусы/padding
Input/Textarea. Внешняя компоновка, высота Textarea и моноширинный код допустимы.


## Композиция списков и страниц

Пагинация использует PageNavigation либо подходящий OffsetPageNavigation Base.
URL, курсоры, проверка допустимости страницы и callbacks принадлежат продукту.
Невалидное состояние блокируется disabled, запрос — pending; не создавать
фиктивные страницы или повторы кнопок. ResourceList linked-actions оформляет
сам anchor через interactive ResourceListRow, а независимое действие — рядом.
Сначала сверять наличие API по точному UI pin; локальные hover/focus/размеры
вместо общего контракта не добавлять.
Разделитель и внутренний отступ после содержимого задаются PageNavigation separated;
внешние margin, ширина и выравнивание остаются у продукта. Border/padding не копировать.

Границу перед группой полей задаёт Fieldset separated; native legend/disabled/FormData
и данные остаются у владельца. Для Disclosure используйте appearance, tone=muted/danger
и size=sm вместо локальных border/padding, цветов и типографики. Внешние margin,
ширина и предметное содержимое остаются у продукта; наличие props сверять по UI pin.
## Меню аккаунта Base

При принятом UI pin с AccountMenu использовать общую композицию аккаунта,
темы и выхода. AppShell применяет её сам; renderHeader требует явного
AccountMenu в actions общего PlatformHeader. Identity, permissions,
logout/pending и предметные пункты beforeTheme/children остаются у продукта;
не копировать trigger, popup или theme/logout layout. Контракт и пример
сначала читать через pdlc-ui; custom API/SSO и установленный runtime не меняются.

## Коллекции боковой навигации

После проверки UI pin используйте SidebarList Base для списка и separated
для его разделителя, nested — для отступа вложенной коллекции. asChild
сохраняет native nav/ul/li/section, hidden, ID и обработчики без DOM-оболочки.
Ключ mapped-узла передавать внешнему SidebarList. Данные, ACL, URL, раскрытие
и внешние margin/width остаются у продукта; внутренний base-sidebar-list и
его border/padding/gap не копировать. Сначала pdlc-ui show и один example.


Текст бокового пункта после проверки UI pin использует SidebarItemLabel Base.
Не копировать base-sidebar-item-label/truncate: compact и сокращение общие.
Иконка, полное aria-label/title, маршрут, current state и callbacks остаются
продукту. У текстового slot нет собственного SSR macro; отдельную кнопку
или focus target вокруг подписи не добавлять.

## Общий inline-код и выделение записи

После проверки UI pin используйте InlineCode для оформленного одиночного кода,
Prose для prepared текста, CodeBlock для многострочного блока и
ResourceListDescription technical для подписи без поверхности. Не копируйте
фон/padding/radius native code. ResourceListRow highlighted задаёт только
оформление: текущий ID, aria-current, ref/tabIndex и действия остаются у продукта.
SSR: inline_code и resource_list_row из штатного экспорта Base.
Каноническое руководство — Base docs/platform/DESIGN_SYSTEM.md и pdlc-ui.
