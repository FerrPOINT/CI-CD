# Подготовка исправлений Git-синхронизации к merge

Статус 2026-10-20: **локальная квалификация объединённого кандидата — PASS**.
Backend, frontend, docs и browser-проверки завершились успешно; собственные
QA-ресурсы удалены. Перед публикацией остаются отдельные publication gates,
а hosted CI результирующего PR подтверждается после публикации. Полная внешняя
приёмка ограничена двумя случаями ниже. Runtime и pinned images этой задачей
не менялись.

## Изменения

При потере соединения, удерживающего namespace admission, worker отменяет Git,
дожидается дочерних процессов и сохраняет `reconciling` до наблюдения refs новым
исполнением. Общий mutation guard удерживается до завершения журналирования.
Дополнительные файловые слоты на общем Git storage наследуются Git-процессами:
потеря PostgreSQL advisory slot не освобождает физическую ёмкость живого child.
Порядок блокировок совпадает с receive-pack и PR merge.

Регрессии проверяют настоящий Git, живые PID, ожидаемые SHA, защищённые ветки,
сохранение истории и точный CI SHA. API/PG assertions дополнены проверками прав,
версии, идемпотентности, namespace filters и pagination. Frontend component и
transport tests проверяют callbacks, URL, запросы и безопасные сообщения об
ошибках. Upstream нормализация offsets и `OffsetPageNavigation` сохранены.

Исправлен контракт CI fixtures: PostgreSQL job задаёт
`CICD_NAMESPACE__PUBLIC_GIT_ORIGIN=https://forge-qa.invalid`, необходимый для
namespace-bound hosted readback. Отдельный Python regression проверяет job
scope. Правила сканера секретов не ослаблены.

## База и объём проверки

Кандидат подготовлен от `main` commit
`305f6e0371d6e23df571cd878bd6401940629cac` с текущими upstream изменениями.
Проверяемый backend source tree:
`5d97e36ad543c2ec078b1e0eb418bac60b825351`; frontend source tree:
`f880819889aa544a6ead6def24cdb959429acae8`. Rust 1.88.0, Node 22.20.0 и
pnpm 10.28.1 закреплены проектом. SDK pin:
`2ad0834d06d3aa703fa1791a932bfc76650c8e42`; UI pin:
`424bcc5a0f7706f48fc75210dd3fe5215acbe829`.

[Полный план](../plans/2026-10-18-repository-sync-complete-test-suite.md)
сохраняет 50 ID и 108 подкейсов, exact selectors и условия opt-in fixtures.
Он отделяет executed tests от ignored definitions, API spies от реальных
Git-эффектов и инструментальные counters от функциональной приёмки.

Независимое review backend/Git/worker и UI/API не обнаружило новых production
дефектов. Найденный пробел CI environment исправлен. Конфликт component tests
разрешён с сохранением обеих сторон. Новый frontend suite включает 55 sync
component tests и пять transport tests.

## Выполненные проверки объединённого checkout

- Backend на canonical export: fmt, Clippy, отдельные CLI integration Clippy
  и minimum-Rust check завершились успешно; workspace tests — 515 passed,
  0 failed, 10 ignored. Ignored definitions не считаются выполненными тестами.
  Normal PostgreSQL suite — 127 passed, 0 failed, 46 ignored за 377,60 секунды.
  Отдельные opt-ins прошли: renewal PG — 2, namespace DevOps evidence — 2,
  owner selection — 1, artifact observer — 1, populated migration
  `0094 → 0095` — 1, CLI real-API с integration feature и отдельной БД — 2,
  native TLS — 1. В каждом из этих запусков 0 failed и 0 ignored.
  Release build прошёл за 7 минут 57 секунд; OpenAPI drift отсутствует.
  Итоговый coordinator завершился с exit 0 и `MERGE_BACKEND_GATES_OK`
  за 1686,38 секунды. SHA256 полного лога:
  `3b325d3c18ac284662bd490d2ea7a094884b680217299794f5584fbc04d6b488`.
- Frontend на tree `f880819889aa544a6ead6def24cdb959429acae8`: 298 tests
  в 46 файлах, 0 failed. Frozen install, `pnpm audit --audit-level low`,
  lint с shell contract, production build, OpenAPI drift/compatibility
  относительно target main, неизменность lockfile/schema, packed Base consumer
  и effective-theme contrast прошли. Итоговый coordinator завершился с exit 0
  и маркером `MERGE_FRONTEND_GATES_OK` за 352,88 секунды. SHA256 полного лога:
  `3362b38f9ca503b5016392f39a6d7c8e406476f9cdd6fb34adb1125fd9a37b44`.
- Docs на canonical export backend tree: 35 regression tests, 0 failed,
  4 platform skips; отдельно 60 Native QA Compose ownership regressions,
  0 failed. Документация, SBOM drift и source secret scan прошли; последний
  проверил 675 текстовых файлов. Итог — exit 0 и `MERGE_DOCS_GATES_OK`.
  Итоговая проверка документации включает этот отчёт и переносимые ссылки ниже.

Безопасные coordinator summaries доступны в репозитории:
[backend](repository-sync-20261020/backend-gates.json),
[frontend](repository-sync-20261020/frontend-gates.json),
[docs](repository-sync-20261020/docs-gates.json).

## Browser-проверка собранного кандидата

[Browser proof](repository-sync-20261020/browser-proof.json) подтверждает
собственный release API и PostgreSQL с синтетическими fixture-записями.
Авторизация использует synthetic OIDC PKCE и настоящую human API session;
выполнены 12 PKCE exchanges и 12 JWKS requests. Runtime services/branding
metadata изолированы. Установка в принятый runtime, Git-эффекты, проверка
внешних credentials и запуск worker в этот browser scope не входят.

Все 31 продуктовых API responses успешны. Проверены список из 21 сохранённого
подключения, история из 21 запуска и 51 ref с Next/Back; четыре направления
выбираются в форме. Два настоящих сохранения disabled-подключения переключают
manual/automatic и увеличивают версию `1 → 2 → 3`. История и результаты refs
загружены из собственных синтетических записей БД.

Девять browser views на ширинах 375/1920/2560 px прошли axe без violations;
горизонтального выхода контента за viewport нет. Все девять PNG дополнительно
просмотрены визуально. Browser proof — `qualified: true`, 70,252 секунды;
полный запуск с установкой Chromium — 150,757 секунды.

| Экран | 375 px | 1920 px | 2560 px |
|---|---|---|---|
| Список | [PNG](../screenshots/repository-sync-20261020/sync-list-375.png) | [PNG](../screenshots/repository-sync-20261020/sync-list-1920.png) | [PNG](../screenshots/repository-sync-20261020/sync-list-2560.png) |
| Подключение | [PNG](../screenshots/repository-sync-20261020/sync-connection-375.png) | [PNG](../screenshots/repository-sync-20261020/sync-connection-1920.png) | [PNG](../screenshots/repository-sync-20261020/sync-connection-2560.png) |
| История | [PNG](../screenshots/repository-sync-20261020/sync-history-375.png) | [PNG](../screenshots/repository-sync-20261020/sync-history-1920.png) | [PNG](../screenshots/repository-sync-20261020/sync-history-2560.png) |

SHA256 проверенного frontend bundle:
`fe897faa4563f97bddb97da18fd21a3903bb85d59942a51be85b33cf8d9ac258`;
backend executable:
`2b33313e1a9a39f851db232f5f8e56d3fba5916011ec8406372e348c0d84d7b9`.

## История повторных проверок и пределы прежних результатов

Начальные неуспешные попытки сохранены как часть provenance. В первом
frontend запуске test истории превысил 5 секунд при параллельной backend
проверке. Поиск по всем accessible names строк заменён точным текстом ref
и scoped navigation; все фазы и assertions сохранены, timeout не повышался.
Изолированный повтор прошёл, затем полный suite прошёл на итоговом tree.
Отдельная промежуточная canonical попытка превысила тот же timeout в
неизменённом dev-proxy test; её ошибка не скрывается итоговым PASS.

Первый export также преобразовал переводы строк. Guard обнаружил это;
backend coordinator был остановлен до release. Повторный canonical export
проверен против Git blobs и versioned attributes: CRLF выпущенных миграций
сохранены. Квалификация относится к повторным canonical gates, а не к
прерванной попытке или преобразованному export.

Для прошлого test-only tree выполнены 61 Rust live invocation на настоящих
провайдерах (GitHub 31, GitLab 30), workspace 447, normal PostgreSQL 127 и
frontend 278 tests. Реальные read-only и natural job-token expiry закрыты.
Это исторические результаты до объединения с новым `main`, не fresh PASS
данного кандидата. Production worker blob
`16bc791cb63581dd64dc1d00733873787c862ed3` и Git adapter blob
`7ca83a6fa0775cb32a70158703efca65f58640b8` совпадают с квалифицированной
реализацией: эти live-прогоны сохраняют доказательство именно данного
Git-алгоритма и границ worker. Они не заменяют проверку новой интеграции с
upstream, backend release и UI с новым Base pin. Прежние file-level Rust
counters включают inline test code; production-only и stable Rust branch
coverage не заявлялись.

## Очистка, публикация и идентичности

Контейнерные проверки используют только временный Compose-проект и Base
ComposeHelper v2 с finally cleanup. Preflight свободного места пройден,
обязательный порог 30 ГиБ не снижался. Итоговый readback текущего build-проекта
подтвердил 0 собственных containers/networks/volumes, удалённые exports и
сохранённый external Cargo cache. Browser helper также не оставил собственных
ресурсов; API fixture cleanup завершён, временная БД удалена, последняя
сохранённая версия подключения — 3. Cleanup другого стартового проекта не
используется как доказательство очистки текущего проекта.

Перед push требуется отдельный расширенный known-value credential audit
итогового tree и артефактов. Его результат здесь не заявляется. Проверка
frontend advisories и ранее выполненный source secret scan не объявляются
полным security/release gate или результатом Trivy image scan.

Полные coordinator JSON с source/pin SHA и exit code, логи и canonical export
identities сохранены как приватные локальные артефакты; загрузка полных логов
в CI/PR этим отчётом не подтверждается. Имена записей —
`backend-canonical-gates`, `frontend-canonical-final-gates`,
`docs-canonical-gates` и `canonical-export-identities`. Их безопасные summaries
и скриншоты приведены выше; source/pin/count/hash identities сохранены в этом
отчёте. Ссылка на свежий hosted CI результирующего PR добавляется после
публикации. Ссылок на каталоги машины исполнителя в документации нет.

SHA256 backend release archive:
`8222f9a9f27cf13dd5e3707f548f33c07ec9d2327f4c127cd5c752d8426100b7`.
Сохранённый integration binary содержит 71 642 728 байт, SHA256:
`4bc709d9aac1e993f155005060284671a06b6f698fc8fcee44ae55e75c2eb438`;
SHA256 его gzip:
`27e745041bc9d02ab24aa94ee537a82b8188edb21aece666d8428f60c63f469e`.
Build metadata привязывает этот бинарь к backend source tree, указанному выше.

## Ограничения внешней приёмки

1. Повторная read-only проверка 2026-10-20 подтвердила: GitHub QA repository
   `FerrPOINT/forge-sync-qa-20261017-01a14808` (ID `1422991769`) остаётся
   private, не архивирован, default branch — `main`. Чтение protection
   возвращает 403 с сообщением «Upgrade to GitHub Pro or make this repository
   public to enable this feature». Это внешнее ограничение, а не успешная
   проверка отказа protected push. Readback сохранён в артефакте
   `github-protection-current.json`; visibility и тариф не менялись.
2. Существующие разрешённые QA repositories непустые. Исторический empty
   external oracle и текущий empty Forge не подтверждают текущий первый push
   в действительно пустой внешний repository. Новые repositories и удаление
   исходных refs не создаются неявно.

Полная внешняя приёмка остаётся незавершённой. Default worker feature flag
остаётся выключенным. Установка в принятый runtime — отдельный этап.
Этот отчёт сам по себе не подтверждает публикацию ветки или merge.
