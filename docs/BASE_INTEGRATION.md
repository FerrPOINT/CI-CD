# Интеграция CI-CD с Base

## Текущий source-кандидат UI0.3.32

Producer `6f3c557698a8224c5a19e56bdc37b199ba076694`, reference
`3d36261c8fa88dcb116b0efbd03596237702f265aa479bbb5758bc2a5d4ba701`.
Frozen/offline preparation подтвердила реальный пакет0.3.32 и CLI integrity;
внешние resolutions и независимые SDK/skills pins сохранены. Base Textarea
получил общий пересчёт autoSize при настоящем увеличении текста браузера.
Native browser регрессия autoSize, полный Base unit и source/package checks
прошли. Product frontend build/unit и IAB остаются отдельными gates;
новый pin сам по себе их не подтверждает. Установленный runtime не обновлялся.

## Предшествующий source-кандидат UI0.3.30

Producer `d12f12e811282cadf881b01e2acd210b3561e88e`, reference
`0127aeb8c91ade94906908a0d0f7b66dea77e5129b360ebfbaf6617826b70bca`.
Общий resolver шрифта используется tokens, React/SSR знаком и Canvas favicon.
Образец вне BrandingScope получает шрифт своего документа; favicon ждёт загрузки
нужных символов выбранного встроенного семейства. Публичные API сохранены.

Frozen offline preparation и CLI integrity подтвердили фактический пакет0.3.30.
Внешние dependency resolutions, SDK/skills pins и продуктовые обработчики не
менялись. Base проверен из настоящего UI-архива вне workspace без Storybook.
Новые production frontend builds, hosted CI и продуктовая браузерная приёмка
учитываются отдельно; установка исключена. Доказательства другого приложения
или предыдущего UI pin не объявляются приёмкой этого продукта.

## Предшествующий source-кандидат UI0.3.29

Producer `b26cb497e4bdf797875301d5a2aa58f156cdd571`, reference `81c68bc7c8b473216b02de86731a786fe2ee5b8b0354aa70949aa514e6bffcc4`.
Исправлен общий runtime favicon: PNG получает правильный MIME, cleanup возвращает
исходные href/type. Для внешнего legacy URL тип определяет ответ. Публичные API,
SDK/skills pins и продуктовые обработчики сохранены. Source selection, frozen
preparation, artifact checks и IAB — отдельные доказательства; установка исключена.

## Предшествующий source-кандидат UI0.3.28

Точный UI SHA указан в [pin](../.base-ui-revision):
`7ad53fbee14c9c8df38d5aa17acd553288ef19d9`, `@pdlc/ui`0.3.28,
reference `2a78538e33c78db0d713ff81d4f4ff0c143a0783ea831c2b44238894ac36efe3`.
Base #384 содержит общий React/native SSR Toaster offset над fixed actions и
сохраняет независимые card/control/menu/dialog радиусы. Расчёт, skin и механизм
принадлежат Base; продуктовые данные, права, записи и callbacks остаются здесь.

Объявленный source pin, frozen preparation, build и браузерная приёмка — разные
этапы. Frozen offline install и CLI integrity нового snapshot PASS: версия0.3.28
и reference совпали с producer, внешние dependency resolutions сохранены.
Build и product browser gates нового комплекта ещё открыты; исторический
PASS0.3.27 ниже не переносится на него автоматически.
SDK/skills pins независимы. Установка и authenticated runtime этим документом
не подтверждаются; полная field/app матрица остаётся открытой.

## Квалификация прежнего UI0.3.27: независимые радиусы контейнеров


Текущий pin включает общий card-radius для Card, framed ResourceList,
Disclosure, preview frame и toast. Вложенные controls, меню и диалоги
сохраняют отдельные control/menu/dialog. Расчёт и skin принадлежат Base;
локальные копии оформления не добавлены.

Frozen offline install, CLI integrity, exact installed snapshot, TypeScript,
shared-shell contract и production build PASS на этом UI pin. SDK/skills pins
и внешние dependency resolutions сохранены. Consumer TS/JS и конфигурации
не изменились относительно собственного UI0.3.26 gate (с учётом исправления
Fleet prepend); прежние unit results переиспользованы по входам, а не объявлены
новым полным прогоном. В Base заново прошли8 native branding и9 native CSS
cases, в том числе три темы/оба порядка CSS и разные card/control значения.
Private evidence: ui027-card-radius-20261029/gate и unit-input-deltas.json.
Установка, hosted CI и полная authenticated browser matrix этим не подтверждены.

## Предыдущие source-квалификации


Финальный checkpoint 2de0f72b включает bounded fetch/body wait. Frozen offline
refresh, exact snapshot, types, production build и shell contract PASS.
Provider/продуктовые композиции относительно d7c1d8c3 неизменны; нижние auth cases
переиспользованы, не объявляются новым прогоном. Changed transport покрыт 96 Base
auth cases; исходный Admin integration file прошёл 18 cases на final pin.

Первый checkpoint d7c1d8c3: P1 clock-skew исправлен в Base, независимый JWT deadline использует owner clock
и monotonic time, не Date.now до owner readback. Frozen offline install,
exact installed snapshot, TypeScript/production build PASS; affected auth-provider
suite: 9 PASS. Production TS, API, SDK/skills, grants и runtime не менялись.
Независимое review через Main требуется до ready/merge; stand и PM03 pin не
затронуты. Остальная theme/browser matrix остаётся follow-up.

Предыдущая квалификация 0.3.25: frozen install, точный installed snapshot,
TypeScript/build, lint/shell guard и OpenAPI check PASS. В scoped unit run
54 cases /4 файла PASS; два catalog cases сначала выявили test stub, который
терял translation defaultValue. После согласования stub с соседним shell test
оба cases PASS с прежними assertions. Production TS не менялся; API и lockfile
сохранены. Live E2E требует реальных Auth/CI-CD и намеренно не запускался.

Audit, sync и webhook ячейки используют TableCell technical/size=sm Base; resource ID использует ResourceListDescription technical. Формат, retry, записи и ошибки остаются Forge.

## Исходная UI-поставка — 21 октября 2026

Объединённая поставка включает Base #298 и #297: общие формы/диалоги, защиту несохранённого ввода, адаптивную оболочку и независимую прокрутку редактора брендинга. На прежнем pin заново пройдены298 frontend-тестов, lint/semantic/shell, OpenAPI drift, TypeScript/Vite build, packed consumer и Chromium contrast для3 тем. SDK, skills, backend и установка runtime этим обновлением не меняются.

UI был закреплён на `94272dc5512be58ec6c88fb7c1d1d33c7599a3e4` ([pin](../.base-ui-revision)); зависит от [Base #297](https://github.com/FerrPOINT/services-base/pull/297).
SDK и skills pins независимы и этим обновлением не меняются. Общая типографика,
плотность и отдельные радиусы применяются в Base к React, Portal и SSR;
продукт сохраняет данные, права, запросы и действия. Штатные SSR-ресурсы Workflow
обновлены из того же UI SHA. Это source-кандидат, а не установка платформы.

Датированные разделы ниже описывают историю квалификаций. Текущий pin сам по себе не подтверждает установку или authenticated-приёмку.

Активный Base SHA закреплён в [`.namespace-base-revision`](../.namespace-base-revision).
При отсутствии этого файла CI и `scripts/build.py` используют legacy
[`.base-revision`](../.base-revision). `main` Base не является воспроизводимой
зависимостью. Checkout Base должен лежать соседним каталогом
`services-base`. Перед standalone build выполнить из корня продукта:

```sh
pin=.base-revision
if [ -f .namespace-base-revision ]; then pin=.namespace-base-revision; fi
python3 ../services-base/scripts/verify_base_revision.py --base ../services-base --revision "$pin"
python3 scripts/build.py
```

Docker standalone и umbrella используют соседние checkout как build context.
CI получает Base на том же SHA. Forge runner sources формируются штатным
`materialize_runner_sources.py` из чистого delivery candidate, с manifest происхождения.

Rust build toolchain: 1.88.0; Node: 22.20.0; pnpm: 10.28.1.
Cargo использует `--locked`; pnpm — `--frozen-lockfile`. MSRV проверяется отдельно.
Приёмка включает OpenAPI drift/compatibility, package consumer и effective-theme
проверки вместе с продуктовыми тестами. После frontend build запустить preview
и `pnpm theme:check http://127.0.0.1:4173`.

Base содержит технические механизмы; продукт сохраняет авторизацию, PAT scopes,
данные, ORM, миграции, HTTP статусы/коды/поля, env/defaults и навигацию.
Новых миграций БД в этой задаче нет. Откат — предыдущий проверенный набор SHA/images.

Текущая реализация и доказательства проверок учитываются в реестре Base
`docs/plans/base-unification-register.json`; незавершённые гейты не отмечаются пройденными.

Кнопка входа вызывает `beginSso(..., { interactive: true })`; автоматический
guard сохраняет неинтерактивный вызов. `isSsoNavigationInterruption` из Base
различает pending logout/отмену навигации и настоящий отказ Auth. Task Tracker
снимает блокировку кнопки при завершении или отмене promise. Identity, PKCE,
state, nonce, API и машинная авторизация не меняются. Source-регрессии покрыты
login-тестами; локальные fixture-проверки не заменяют живую SSO-приёмку.

Закреплённый Base сохраняет focus внутри общего `ConfirmDialog`, когда запрос
переводит все действия в pending: Tab/Shift+Tab остаются в диалоге, Escape не
закрывает незавершённую операцию. После её завершения действуют прежние правила
закрытия и возврата focus. Продуктовые операции и HTTP-контракты не меняются.
Обновление pin требует повторных frontend/package/theme и живых UI-проверок;
исторические результаты прежнего Base не подтверждают эту приёмку.

## Отдельный UI pin и размещение в Base

`.base-ui-revision` закрепляет frontend Base; `.base-revision` остаётся SDK pin.
Frontend job CI проверяет UI checkout независимо от backend jobs. Для frontend
сборки используйте `python scripts/build.py frontend` и соседний UI checkout
на точном SHA. Совместную пересборку backend и UI с разными pins выполняйте
отдельно, сохраняя исходный SDK checkout.

UI pin `e0759e070326c905f365182d0107f8c87a2e67f6` принимает Base #245:
до получения runtime-каталога меню сохраняет названия сервисов без ссылок.
Ошибка или пустой каталог не разрешают переходы по встроенным адресам другого
стенда. Полученный каталог задаёт точные URL; продуктовая навигация и project
scope сохраняются. Регрессия рендерит настоящий `AppShell` и Base provider,
проверяя ожидание, отказ и получение каталога. SDK/Rust pin, API, данные,
credentials и `VITE_NAMESPACE_ENABLED` этим UI-обновлением не меняются.
Прежний переход `/users` использует тот же каталог и переносит namespace в
Admin, сохраняя UI base path. Пока адрес не подтверждён, действие недоступно.
Неиспользуемый build argument `VITE_ADMIN_UI_URL` удалён: настройка назначения
выполняется через `VITE_PLATFORM_SERVICES_URL` и собственный каталог Admin.

`VITE_PLATFORM_ORIGIN` передаётся как Docker build argument. Пустое значение
сохраняет standalone; настроенное включает memory router, silent SSO и общий
bridge. Продукт сохраняет собственные API, профиль, permissions и формы.
[Оболочка Base](https://github.com/FerrPOINT/services-base/blob/2523e7ac1b56a14e9038f41472df4ce7a114dc1e/frontend/platform/README.md) описывает
конфигурацию, подготовку кадра и отдельную runtime-приёмку.

## Реорганизация source Base

Общие Rust SDK находятся в `services-base/base-modules/rust`, Python SDK —
в `services-base/base-modules/python/cli-core`, UI — в `services-base/base-modules/ui`.
Auth, оператор, host и справочник имеют собственные корневые каталоги приложений.
Имена пакетов, HTTP/wire-контракты, данные и credentials этим переносом не меняются.
UI/SDK/skills pins независимы; source-комплект проверяется по точным revisions.
[Карта Base](https://github.com/FerrPOINT/services-base/blob/a4dea438ec99103ca83b51540d11de548b1ee82d/docs/platform/ARCHITECTURE.md)
и [ADR 0006](https://github.com/FerrPOINT/services-base/blob/a4dea438ec99103ca83b51540d11de548b1ee82d/docs/adr/0006-base-repository-layout.md)
определяют расположение и направление зависимостей. Установка выполняется отдельно.

Privileged PostgreSQL consumer проверяет полный транзитивный операторский пакет
и общий compatibility bootstrap вместе с вызывающими скриптами. Builder получает
тот же source closure; изменение внутреннего файла должно отклоняться guard.
SQL transport подменяется на канонической границе, без дополнительной реализации.
Namespace `scripts` создаётся с единственным проверенным путём Base; обычный
поиск пакета не исполняет незакреплённые `scripts.py` или `scripts/__init__.py`.
Loader, ограниченный модулями Base, компилирует явные `.py` файлы. Он не читает
прежние `.pyc` и не выбирает незакреплённые native sidecars вместо источника.
Rust передаёт свой compiled manifest имён во внутреннем packet: добавленный
package-каталог не может заменить закреплённый module-файл. HTTP/policy DTO
этим служебным полем не расширяются.
Системные импорты сохраняют обычное поведение; временный cache не создаётся.
Регрессии используют настоящий coordinator и полный helper closure без Docker
или обращений к данным стенда.

## UI-кандидат 2026-10-18

Общий UI закреплён на `6f1c797a0b11380f7e36db5ced0360f25a0265a5`. SDK и skills pins остаются независимыми.
Четыре списка синхронизаций используют общий OffsetPageNavigation. Forge сохраняет offset/limit, фильтры, URL и запросы; некратный offset не округляется.
Проверки source-кандидата и установленного PDLC2 учитываются отдельно;
обновление pin не означает установку.

## Общие строки действий — 2026-10-26

UI producer: `c0d2245e40303efffc04c7d8c24eb3746afd1439`. В Base добавлен ActionGroup и SSR action_group;
PageHeader, SectionHeader, PageNavigation и ChatComposerActions используют общий
каркас. Данные, права, курсоры, submit/pending, неизвестный результат записи и
readback сохраняются у продукта. Для пояснений используется ResourceListDescription.
Внешние margin/width/grid placement и sticky-контейнер принадлежат экрану;
общие gap, разделитель и мобильная компоновка — Base. Нативные формы,
тип кнопок, hidden, IDs и callbacks сохраняются. SDK и skills pins не изменены.

Проверены шесть offline frozen installs, singleton React, CLI/reference,
frontend types, guards и штатный SSR fingerprint. Изменённые React-сценарии
имеют scoped regression checks; общий UI проверен в IAB на трёх темах и
ширинах320/375/1920/2560. Это source adoption; установленный runtime, SSO
и продуктовые данные не изменялись. Полный release/hosted/runtime gate отдельно.

## Строки действий — 2026-10-26

Количество строк действий, переведённых на ActionGroup Base: 10.
Внешнее размещение, видимость и предметные grid остаются у продукта; gap и
перенос — у Base. Callbacks, ref, права, native типы, URLs и сохранение
не меняются. UI producer остаётся c2ca61094a904706fceeda8eb8f3b3cfd2e7cdfc;
UI API, SDK/skills pins, зависимости и установленный runtime не обновляются.
Scoped проверки и компонентный IAB не заменяют полный integrated gate
и authenticated-приёмку страниц.

## Вертикальные группы действий — 2026-10-26

Принят UI producer `7294a7974a898f182c22034b511c6b4b15205cef`; SDK/skills pins сохранены.
Совместимый `ActionGroup orientation="vertical"` и SSR `action_group`
описаны в [контракте Base](https://github.com/FerrPOINT/services-base/blob/7294a7974a898f182c22034b511c6b4b15205cef/docs/plans/action-orientation-20261026.md).
Проверены архив UI, offline frozen install шести React-потребителей,
singleton React, CLI/reference, шесть frontend types и SSR fingerprint.
Forge дополнительно прошёл19 адресных тестов и lint двух изменённых модулей;
остальные backend/application builds не выполнялись.

IAB проверяет source-компонент Forge с синтетическими callbacks:24 сочетания
темы, ширины и направления; это не приёмка установленного приложения.
Данные, права, pending и подтверждения остаются у продукта; сетка корзины
Tracker связана с её данными и не получает общий breakpoint из Base.
Публикация, полный интеграционный gate и установка учитываются отдельно.

## Подготовленный HTML и клавиатура — 2026-10-27

Принят совместимый UI producer `a95d9fe7b31cca23e1f6432bae43246120859efb`; SDK/skills pins сохранены.
Общий Prose keyboardScroll документирован в [Base](https://github.com/FerrPOINT/services-base/blob/a95d9fe7b31cca23e1f6432bae43246120859efb/docs/platform/DESIGN_SYSTEM.md).
Проверены настоящий архив UI, offline frozen install, singleton React,
reference/CLI и typecheck данного frontend.
Backend build, hosted CI, общий интеграционный gate и установка здесь
не выполнялись; source adoption не меняет установленный runtime.

## Защита сохранённых в памяти черновиков — 2026-10-27

Repository sync dev-preview использует UnsavedChangesGuard Base. Локальный
beforeunload редактора удалён: он переставал работать при возврате в список,
хотя черновик оставался у родителя. Один guard у владельца коллекции защищает
и скрытый черновик другого репозитория. Data Router и штатный i18n подключены
в preview entry; список, фильтры и история сохраняют прежний query-контракт.

Внутренние переходы не требуют подтверждения; уход со страницы и переключение
ветки preview требуют его при наличии правок. После сохранения или сброса
предупреждение снимается. Равенство настроек, ввод и вымышленные credentials,
save/cancel, порядок запуска и transport остаются у preview. Production editor,
HTTP/DTO и backend этим изменением не менялись.

Проверены 12 сценариев: восемь прежних и четыре новых. После уточнения ожидаемого
fixture-значения и удаления неподдерживаемого TypeScript option повторены только
затронутые новые случаи. Types, lint, штатный shell/primitive/choice guard и
diff check прошли. Whole-module TypeScript projection ограничивает изменения
preview и entry заявленным подключением общего механизма.

IAB: три темы, 320/375/1920/2560, общий диалог, Enter/Escape, возврат focus,
внутренняя навигация, скрытый черновик, подтверждённый уход из списка и свободный
уход после сохранения/сброса. Использовались source-компоненты и AST-извлечённый
entry selector с синтетическим окружением; API/Auth и установленные стенды
не использовались. [Снимки и область проверки](assets/screens/preview-guard-20261027/README.md).

UI/SDK/skills pins, package/lockfiles и production bytes Base совпадают с прежним
архивом. Install/pack/generation, backend и production builds не повторялись.
Общий аудит, полный интеграционный gate, hosted CI и публикация остаются открытыми.

## Общая защита основного sync-редактора — 2026-10-27

RepositorySyncEditor использует UnsavedChangesGuard Base. Локальные правки,
замена токена и pending/unknown save/start защищены при уходе и смене namespace.
Вкладки, история, параметры выдачи и нормализованная запись того же namespace
сохраняют ввод; project_scope при прежнем ресурсе не становится ложной причиной
блокировки. Чистый read-only check также не блокирует уход. Дополнительную
границу размонтирования preview передаёт его родитель.

Base 0.3.3 продолжает исходный переход при подтверждении всех текущих правок.
Поздний ответ не возобновляет отменённый уход и не снимает защиту новых правок.
Диалог не повторяет записи; idempotency, readback, ACL и неизвестный результат
сохраняются у Forge. fallbackFocusRef возвращает focus на общий заголовок,
если инициатор исчез. Back/Forward тестов переведены на стабильный Data Router.

Локально проверены шесть тестов Base, 56 существующих сценариев sync и десять
сценариев ухода. При финальном прогоне один новый тест увидел промежуточный
status до завершения запроса: ожидание уточнено по feedback и доступности
кнопки; оба 403/409 случая повторно прошли. Types/lint и штатный UI contract gate
прошли. Проекция всего JS-модуля подтверждает сохранность прежних product
handlers вне объявленной интеграции guard.

Codex IAB: light/dark/gray × 320/375/1920/2560, Enter/Escape, focus, сохранность
ввода, подтверждённый уход и автоматическое завершение исходного перехода после
одного синтетического save. Самостоятельный пример Base проверен отдельно:
отмена, позднее подтверждение и новая версия. [Снимки и границы](assets/screens/editor-guard-20261027/README.md).
Это source-компоненты с синтетическим transport; API/Auth/installed runtime
не проверялись. Общий интеграционный gate, hosted CI и публикация открыты.

## Общая подпись навигации — 2026-10-27

Принят совместимый UI 0.3.4, producer `efa4babe176b42530a3e17fc25f10e8678f82d33`.
Подписи боковых ссылок используют SidebarItemLabel Base; native span,
полное доступное имя, title, маршруты, состояние и callbacks сохранены.
Ellipsis и compact visibility принадлежат Base; private CSS-класс не копируется.
Проверены настоящий архив, offline CLI и frozen adoption: шесть React consumers
с одним React, седьмой SSR consumer — через штатный export/check. SDK/skills
pins неизменны; это source-кандидат, без установки и hosted CI.

Итоговый producer проверен 22 scoped tests боковой навигации;
TypeScript, lint изменённых модулей и shell contract gate прошли отдельно.

## Нативные атрибуты SSR Button — 2026-10-27

Текущий UI 0.3.5: `8bdd8ecd54eb6a6f553bfbadcfaf07c99f4a801d`. Сохраняются React API и независимые SDK/skills pins.
React/JS/CSS и типы API не менялись; frontend/backend не пересобирались.
Новые SSR параметры доступны в этом точном Base, не являются новым React API.
Архив UI и offline frozen adoption проверены; общий интеграционный gate,
hosted CI, публикация и установка здесь не выполнялись.

## Общие нативные элементы SSR — 2026-10-27

Принят совместимый UI 0.3.6, producer `623deb7da5813fa9d1185ded7688c35bb15cd43e`.
Изменения принадлежат SSR macros и справочнику; React API, JS/CSS/types и
зависимости совпадают с 0.3.5. Повторная frontend/backend-сборка не нужна при
неизменных inputs. Настоящий архив из 868 файлов сверён с установленным UI,
offline frozen install, singleton React и CLI check пройдены. SDK/skills pins
неизменны. Это source adoption, без hosted CI и установленного runtime.

## SSR композиции и native reset — 2026-10-27

Текущий UI 0.3.8, producer `cf88f7094044a403e6bb7105cfa88b12c4208c96`.
Общие SSR parts принимают native attributes. В отдельном select-runtime
исправлено обновление trigger после нативного reset; API React и CSS/types
не менялись. Root UI/lib не импортируют этот runtime, прямых импортов
select-runtime в React runtime source продукта нет. Настоящий архив из
868 файлов сверён, offline frozen install, singleton React и CLI пройдены.
SDK/skills pins неизменны. Это source adoption, без установки и hosted CI.

## Рабочая область Base — 2026-10-27

Текущий UI 0.3.9, producer `bab93d2683367117784c2f01fcbed7f80fd3ef15`.
Workbench и семь частей добавляют общий каркас рабочей области внутри PageFrame:
высота по настоящим gutter, внутренний scroll, адаптивные Aside/AsideAction и
mobileFlow для длинной формы. SSR-аналога этого React-контракта нет.
Настоящий архив из 882 файлов сверён с установленным UI; offline frozen install,
singleton React и CLI check пройдены. SDK/skills pins сохранены. Это source
adoption; приложение, backend и Storybook не пересобирались, runtime не менялся.

## Заголовок CapabilityCallout — 2026-10-27

CapabilityCallout сохраняет предметные уровни current/mvp/configuration/target
и их соответствие семантическим Notice tones. Заголовок оформляет SectionHeader
Base, подпись уровня — Badge. Локальный flex только компонует текст и подпись;
он не определяет шрифт или геометрию общего контрола.

TypeScript, scoped ESLint, format, UI guard и AST-сохранность предметного модуля
проверены. В IAB импортирован настоящий CapabilityCallout с синтетическим текстом:
четыре состояния, light/dark/gray, 320/375/1920/2560, без overflow; снимок просмотрен.
Это проверка leaf UI, без backend, SSO и runtime. UI/SDK/skills pins неизменны.

## Пояснение секретов и pending preview — 2026-10-27

Статическое пояснение страницы секретов использует Notice appearance=inline
separated с прежней ShieldCheck и строкой локализации. Общие отступы, типографика
и цвет принадлежат Base; новый alert/live-region для статического текста не создан.
Сохранение, замена и удаление секретов остаются прежними.

В действующем preview синхронизаций при busy сохраняется прежняя надпись и
native disabled. RefreshCw остаётся в состоянии покоя; локальная animate-spin
удалена. Статический Loader рядом с уже содержащей многоточие надписью не нужен.
TypeScript, scoped ESLint/format, guards, 6 secrets и 12 preview cases пройдены.
IAB: точные JSX-композиции с CSS Forge, три темы и четыре ширины, без overflow,
мобильная кнопка минимум 44px. Это не backend/runtime acceptance. Pins неизменны.

## Общие секции и полосы данных — 2026-10-27

UI 0.3.10, точный producer `94f2ccd9e6f09eba9fbf67f4582a9fa7a91c44b9`.
Прозрачные области экранов используют ContentSection. Статическое
распределение отчёта использует DataBar, сохраняя исходные counts/scale/label
и точные форматтеры владельца. Responsive-сетка метрик остаётся у Forge.

Фирменные разделители, inset и плотность принадлежат Base. Данные, DTO,
права, формы, смысловые статусы и внешний grid/gap остаются у продукта.
В справочнике доступны самостоятельные ContentSection/Form и DataBar/basic;
для статических данных применяется DataBar, для выполнения операции — Progress.

Архив 895 файлов и установленный UI сверены; offline frozen refresh, React
singleton, CLI check, TypeScript, UI contract guard и scoped tests пройдены.
Изменённые TSX также прошли scoped ESLint.
Браузерные доказательства относятся к изолированным общим композициям,
не ко всем страницам приложения/API/Auth. Полный интеграционный gate и
установленная приёмка остаются отдельными. SDK/skills pins и runtime сохранены.

## Общие метки и inline-код UI 0.3.11 — 2026-10-27

Точный UI producer `755c9f9a27ba81ddd841b43c511bb372a3c93e40` принят отдельно от SDK/skills.
Девять оформленных code используют InlineCode; четыре SHA-подписи — ResourceListDescription technical с native code. Чип фильтра и dot окружения используют Badge, UTC-пояснение — Notice, readonly ключ — DescriptionItem. ResourceListRow highlighted сохраняет owner ref/tabIndex/aria-current. Оба header действующего repository-sync preview используют PlatformHeader без switcher; история использует ResourceList. Все 78 выбранных tests прошли.

Настоящий архив UI содержит 900 файлов; все production bytes сверены с
producer и шестью установленными frontend-пакетами. Offline frozen refresh,
React singleton и CLI check пройдены; runtime и зависимости backend не менялись.

IAB evidence относится к пяти изолированным композициям из настоящих owner/Base
исходников: 60 сочетаний темы/ширины, 18 branding-пресетов, три статуса Wiki,
readonly click и Enter/Space. Это не приёмка всех экранов, Auth или API продукта.
Полный интеграционный gate остаётся отдельной открытой стадией.

## Общая render-граница UI 0.3.12 — 2026-10-27

Точный UI producer `20633a6489dfd33e7afa782e0a06c23faea26b7a` принят отдельно от SDK/skills.
Предметные React-композиции не менялись; принят совместимый общий UI.

Настоящий UI-архив: 904 файла, offline CLI/reference и production bytes
проверены вне workspace без Storybook. Шесть frontend-пакетов сверены с архивом;
cached offline frozen refresh, React singleton и CLI check прошли.
Общий механизм и ограничения: Base docs/platform/DESIGN_SYSTEM.md,
раздел «Граница render-ошибки React». Данные, fallback, отчёт и reset identity
принадлежат владельцу. HTTP/async/event/SSR ошибки обслуживаются отдельно.

IAB evidence относится к фактическим Base/CatalogProviders source-композициям:
12 сценариев ввода/сбоя/reset и 12 длинных fallback в трёх темах и четырёх
ширинах; сохранены preview ready/error и черновик. Это не приёмка установленного
продукта или Auth/API. Интеграционный gate, hosted CI и публикация остаются
отдельными стадиями. Runtime не менялся.

## Общая responsive-подписка UI 0.3.13 — 2026-10-27

Точный UI producer `83c961379b25379e8c83ebc207eec60cfb4ebbef` принят независимо от SDK/skills.
Продуктовая компоновка не менялась. Общий AppShell/drawer получают один Base observer; шесть frontend прошли types и UI contract guard. Проверено 18 адресных тестов продуктовой оболочки.

Новый hook доступен через `@pdlc/ui/lib`; query задаёт владелец. Необязательный
callback выполняется до React layout update только на browser event, сохраняя
перенос focus из исчезающего control. Mount/query changes не запускают действие.
SSR/отсутствующий matchMedia дают false. Контракт, cleanup и standalone example
доступны через `pdlc-ui show useMediaQuery` и `pdlc-ui example useMediaQuery`.

Архив 907 файлов, offline CLI, шесть frozen refresh, React singleton и сравнение
установленных production bytes прошли. Lockfiles после подготовки не менялись;
backend, SDK/skills pins и установленный runtime не изменялись.
IAB source proof: 27 Tracker viewport transitions, 30 shell/drawer checks и
12 recovery cases в трёх темах. Ширина менялась без reload; ввод, focus и journal
сохранены. Это source-композиции, не приёмка полного authenticated приложения.
Прежний IAB attachment gap закрыт новым успешным run; ошибки сохранены в истории.
Полный интеграционный gate, Rust transport vectors, hosted CI и публикация отдельно.

## Общая геометрия истории UI 0.3.14 — 2026-10-27

UI producer `f3b99e3ce15a4a447d58790f118c6be70e931a18` принят независимо от SDK/skills.
Архив содержит 910 файлов; offline CLI, шесть frozen refresh и production bytes
проверены. В Workflow штатный SSR export остался byte-identical. Это обновление
source-поставки; установленный runtime не менялся.

JobLogPanel использует useScrollViewport и useDebouncedValue(300). Debounce
сохраняет raw-input lifetime; trim выполняется после него. Scope включает job,
attempt и поисковый запрос. Live append не завершает restore старой страницы;
CodeBlock получает stable host rows, сохраняя линейный текст. Tail/filter/unread
и загрузка страниц остаются у Forge. 12 owner tests, types и scoped lint прошли.
Сравнение с прежним source дало RED 250 вместо100 px; текущий вариант проходит.
IAB использовал извлечённый настоящий JobLogPanel с синтетическим query adapter,
а не полный продукт с backend. Native DOM воспроизвёл прежний сдвиг 0→18 px при
append. Текущий вариант сохраняет anchor в 12 theme/width contexts; ошибка,
явный jump и новая identity проверены отдельно. Установленная приёмка отдельно.

Base qualification: 17 hook tests, 69 primitive checks и 37 search assertions.
IAB Base/Forge source-compositions: 24 theme/width cases плюс три intent/error
cases; это не полная authenticated-приёмка продуктов. Полный интеграционный gate,
Rust search transport, hosted CI и публикация остаются отдельными стадиями.
