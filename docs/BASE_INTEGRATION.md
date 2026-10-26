# Интеграция CI-CD с Base

## Текущий source-кандидат UI

Точный UI SHA: `c2ca61094a904706fceeda8eb8f3b3cfd2e7cdfc` ([pin](../.base-ui-revision)).
Компоненты и reference берутся из этой версии Base; открытый справочник другой
версии не подтверждает доступность контракта в продукте. SDK/skills pins
независимы. Это исходники собственного checkout; установка и authenticated
приёмка runtime этим значением не подтверждаются. Датированные разделы ниже
описывают прежние квалификации и не заменяют проверку итогового кандидата.

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
