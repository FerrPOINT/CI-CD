# Интеграция CI-CD с Base

## Текущая UI-поставка

UI закреплён на `fd5d1e78d064c7dac7bbf631d289c0c919aa8521` ([pin](../.base-ui-revision)); зависит от [Base #295](https://github.com/FerrPOINT/services-base/pull/295).
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
