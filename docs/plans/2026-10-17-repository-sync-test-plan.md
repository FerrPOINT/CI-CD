# Проверка кода и план приёмки синхронизации репозиториев

Дата: 2026-10-17. Проверяемый commit:
`178efff51a307665309901b700016d2a10385fb2`, Base:
`2ad0834d06d3aa703fa1791a932bfc76650c8e42`.
Объём ревью — все 53 файла изменения и затронутые границы авторизации,
Git Smart HTTP, PR merge, namespace admission, CI, worker и production UI.
Общая регрессия проверяет также существующие тесты приложения.

Установленный Forge: http://localhost:8712. Ниже сохранён исходный результат
аудита commit 178efff до исправлений. Результаты последующей расширенной проверки
и границы фактически выполненной приёмки фиксируются в
[отчёте о проверке возможностей](2026-10-17-repository-sync-capabilities-results.md).

## 1. Результат ревью

Это исходные находки ревью. Их исправления и повторные проверки перечислены
в отчёте; красные исходные reproductions сохранены как evidence.

| ID | Приоритет | Дефект и последствие | Проверка исправления |
|---|---|---|---|
| REV-04 | P1 | `backend/domain/src/repository_sync.rs:38–63,83–84`: origin проверяется через `url::Url`, но в Git передаётся исходная строка `instance_url`. Адрес вида `https://allowed.example.invalid\@127.0.0.1:9` проходит GitLab allowlist первого хоста, а Git/libcurl выбирает `127.0.0.1:9`. Таким образом обходится ограничение исходящих origins; credential transport не должен подключаться к отличающемуся хосту. | Отклонять неоднозначный raw URL либо передавать проверенный canonical URL с единым смыслом authority. Добавить differential tests Rust/Git для backslash, userinfo, encoding, control characters, ports/base paths. Текущий regression падает; Linux Git отдельно подтвердил другой хост без токена и без реального провайдера. |
| REV-01 | P2 | `backend/src/api/authz_mw.rs:201–207` запрещает service account все `/catalog/repositories...` до проверки repository/project scopes. Поэтому корректный machine token не достигает семи новых операций чтения и команд под `/sync`. `PgSyncStore::authorize` разрешает нужную привязку, но это не снимает ранний HTTP-отказ. | Полный production router: правильный project и scopes допускаются; чужой project, отсутствие scope, revoked/expired token отклоняются. Существующий store-тест и HTTP-тест с вручную установленными claims эту границу не проверяют. |
| REV-02 | P2 | `frontend/src/pages/repository-syncs/index.tsx:974–984`: переход `active=true → false` останавливает polling истории и refs без финального запроса. Если родитель раньше увидит завершение, заголовок показывает успех, а таблица остаётся running/planned. | Завершить запуск между ответами connection/runs/refs; без reload и переключения вкладок должны появиться terminal run и окончательные refs. Воспроизведение Vitest уже падает на отсутствии второго запроса. |
| REV-03 | P2 | `backend/src/api/router.rs:461–469`: явный CORS allowlist не разрешает `PUT`, используемый сохранением sync. При обращении браузера к API с другого origin preflight не разрешает сохранение. Same-origin proxy этот дефект скрывает. | OPTIONS нового PUT endpoint с разрешённым Origin, Authorization, Content-Type, Idempotency-Key; ответ обязан разрешать PUT. Запрос чужого origin по-прежнему не разрешается. |
| REV-05 | P2, тесты | `backend/tests/integration_db.rs:9059–9062,9094`: каталог ожидает только migrations 1–40,90–92. После добавления 0093 соответствующие общие integration tests красные. Восемь sync-тестов этот общий gate не заменяют. | Обновить ожидаемый каталог до 0093, сохранив независимые исторические SQL/checksum fixtures; повторить fresh/prior-schema upgrade, checksum/missing-version отказ и весь PG suite. Это дефект сопровождения тестов, а не доказательство повреждения runtime DB. |

Дополнительная интеграционная работа: Main уже изменил те же `authz_mw`,
`git_host`, `pulls` и их тесты. В нём есть отдельный ADR-0031 про SAT receive
и ADR-0029 про snapshots PR. До объединения нужно сохранить обе реализации,
устранить коллизию номера ADR и повторить проверки на результирующем commit.
Зелёный OpenAPI compatibility не проверяет сохранность этих Git-гарантий.

Точки обязательной дополнительной проверки, пока без утверждения о дефекте:
смена защиты одновременно с применением ref; два занятых Git guard и продвижение
третьего репозитория в очереди; crash API во время check и уборка его scratch;
окончательное обновление истории при быстрых запусках; локальный сигнал в
automatic pull. Последний нужно сверить с обещанием «после изменений в Forge»:
сейчас `PgSyncStore::signal` помечает dirty только push/both.

## 2. Что подтверждают существующие проверки

| Уровень | Подтверждено | Граница доказательства |
|---|---|---|
| Domain | Три теста: часть направлений/ancestry, тег, новая защищённая ветка, allowlist URL/path. | Это не полная таблица комбинаций всех направлений, отсутствующих refs, тегов и защиты. |
| Crypto и Git infra | AES-GCM roundtrip/corruption, настоящий Git FF/CAS/divergence/tag conflict, наследование guard и orphan helper. | Нет систематического сбоя реального worker в каждой точке intent/effect/receipt. |
| PostgreSQL sync | Восемь тестов: upgrade, CAS/idempotency, coalescing/fencing/slots, disable, archive, ACL store, HTTP handlers, exact-SHA CI/dedupe, конкурентный merge одного PR. | HTTP sync fixture обходит общий auth middleware. Два supervisor в live-тесте работают задачами одного процесса. |
| Backend gates | Ранее прошли fmt, clippy workspace/all-targets с integration и `-D warnings`, workspace tests, release build; затем отдельно 8 PG sync tests. | `cargo test --workspace` без feature integration не запускает real-DB suite. Нельзя называть эти 8 тестов всем PG suite. |
| Frontend | Повторно прошли 236 тестов / 43 файла, lint, production build, OpenAPI drift/compatibility на Node 22.20.0 / pnpm 10.28.1. Production sync имеет 6 тестов, preview — 8. | Preview-тесты и screenshots с fixture transport не доказывают работу установленного API. Новый тест REV-02 падает. |
| GitHub и GitLab HTTPS | На обоих выполнен общий adapter-сценарий push/pull/annotated tags/CAS и worker-сценарий manual pull, protected main, partial, automatic both, два supervisor, восстановление unknown intent по SHA. | Lost ACK реконструирован записью журнала после успешного push. Реального обрыва подтверждения и kill worker при передаче в этом тесте не было. |
| Credentials/TLS | Неверные credentials обоих провайдеров отклонены; изолированный Git CLI отклоняет недоверенный TLS и принимает системный private CA. Точные использованные credentials не найдены в source, Git objects и артефактах. | CLI fixture не заменяет ошибку доступа/TLS через полный API → worker → UI; реальные read-only/revoked/expired PAT и custom CA adapter ещё требуют проверки. |
| Установка | Backup, миграция копии реальной БД, сохранение бизнес-строк, установка backend/web, GET списка/настроек/истории 200. | В установленном Forge ещё нет сохранённого подключения; полного UI → провайдер → refs → CI сценария нет. |

Отдельные воспроизведения REV-01/03/04 уже завершились ожидаемым отказом:
HTTP machine token получил 403 вместо 200, CORS не разрешил PUT, raw GitLab URL
прошёл проверку. Для REV-04 дополнительно сохранён независимый результат Linux
Git в `audit-url-parser-git.json`; настоящий credential в пробе не использовался.
Полный PG suite повторно выполнен с обязательным `forge_test_*` именем disposable DB
и настроенными namespace fixture IDs; отказы предыдущего запуска из-за отсутствия
этих настроек не считаются продуктовыми дефектами.

### Итог повторных прогонов

- Frontend: **236 passed / 43 files**; lint, build, OpenAPI drift и compatibility
  пройдены. Дополнительное воспроизведение REV-02 — **1 failed**.
- Полный обычный PostgreSQL suite на Rust 1.88.0: **104 passed, 2 failed,
  4 ignored**. Падают `migration_catalog_contains_exact_historical_36_and_37`
  и `migration_catalog_fresh_and_prior_35_or_37_upgrade`: оба ожидают каталог
  без 0093. Все **8 sync PG tests passed**. Общий PG gate остаётся красным.
- Дополнительные boundary reproductions REV-01/03/04: **3 failed** на текущем
  коде, как и ожидается для подтверждения дефектов. Это отдельный запуск,
  не включённый в обычные 110 PostgreSQL tests.
- Четыре ignored PG checks не объявляются пройденными: live provider worker,
  task-pipeline custody/HTTP/migration scenarios имеют отдельные предусловия.
  Ранние отдельные live provider evidence сохраняются со своими границами.
- Повторный baseline secret scanner: **587 text files, 0 findings**;
  `git diff --check` пройден. Настоящие provider tokens в этом аудите не использовались.
- Все три собственных временных Compose-проекта очищены по журналам v2,
  disposable source и test helper удалены. Общий Docker audit вернул **exit 1**:
  на `sdlc2-runner` есть два завершённых OCI transport контейнера других проектов
  `sdlc-qa-forge-oci-35a0d154dc1847ea8f036396` и
  `sdlc-qa-forge-oci-115267b44ab74ef1b79c240d` без доступного job journal reference.
  Они не относятся к трём проектам этого аудита и не удалялись.
  Подробности: `audit-cleanup.json`, `audit-docker-groups.json`.

Первый некорректно настроенный общий PG запуск сохранён в `audit-postgres-all.log`
для диагностики harness. Для вердикта используется только
`audit-postgres-configured.log`; нельзя переносить его исправленные environment
ошибки в список продуктовых дефектов.

Локальные журналы находятся вне Git в `../evidence` относительно worktree:
`audit-frontend.log`, `audit-history-regression.log`,
`audit-postgres-configured.log`, `audit-boundaries-configured.log`, `audit-results*.json`.
Воспроизведения: `audit-history.test.tsx.txt`, `audit-machine-test.rs.txt`.
Это намеренно красные тесты текущего commit, а не пройденные release gates.

## 3. Репозитории и тестовые данные

Нужны две независимые пары: hosted repository Forge ↔ private GitHub и
другой hosted repository Forge ↔ private GitLab. Одно подключение на UUID
не позволяет одновременно проверить оба провайдера на одном Forge repository.

| Провайдер | Состояние | Подготовка |
|---|---|---|
| GitHub.com | Существует private `FerrPOINT/forge-sync-qa-20261017-01a14808`, наличие и private проверены повторно. | Его можно повторно использовать для непустой истории, сохранив снимок refs. Для сценария первоначального заполнения нужен новый пустой private repository или отдельный свежий fixture. |
| GitLab `https://gt.wmtgroup.ru` | Пользователь указал [pdlc/forge-sync-qa-20261017-01a14808](https://gt.wmtgroup.ru/pdlc/forge-sync-qa-20261017-01a14808) как QA-репозиторий для этой приёмки. | Использовать этот проект; перед Git-обменом проверить visibility, доступ и исходные refs. Повторно создавать проект не требуется; пустота и права записи пока не проверены. |

Настройки GitLab fixture: `provider=gitlab`,
`instance_url=https://gt.wmtgroup.ru`,
`repository_path=pdlc/forge-sync-qa-20261017-01a14808`.
Git HTTPS URL: `https://gt.wmtgroup.ru/pdlc/forge-sync-qa-20261017-01a14808.git`.
Ранее удалявшийся проект в личном namespace `azhukov` — другой ресурс;
его cleanup не относится к указанному пользователем проекту `pdlc`.

Новый пустой fixture не должен зависеть от результатов предыдущего теста.
Текущий ignored live adapter предполагает пустой remote, а worker-тест — remote,
уже заполненный adapter-тестом. Перед повторной приёмкой эти предусловия нужно
явно оформить или разделить fixtures. Простой повтор seeding в существующий
GitHub repo создаст unrelated histories и даст ложный отказ.

Правила данных:

1. Уникальный run ID и префикс `forge-sync-qa-`; фиксировать provider ID,
   visibility, владельца и точный список принадлежащих проверке ресурсов.
2. Пустые или синтетические коммиты; `.forge-ci.yml` с безвредной командой
   вывода marker, без настоящих исходников и project secrets.
3. Каждый сценарий получает собственные refs `qa/<run>/<case>` и tags либо
   свежую пару репозиториев. Создать графы equal, A→B, A→B/A→C, разные tag
   objects на один commit, annotated tag, lightweight tag, blob/tree tag.
4. PAT передавать только через защищённую память/канал процесса, затем через
   форму/API в зашифрованное хранилище. Не писать в Git, URL, argv, logs,
   labels, Compose, screenshots, trace/video, browser storage или evidence.
5. Credential для создания проекта отделить от Git credential минимальных
   прав. GitLab Git-only scopes должны работать без полного API scope.
   Для GitHub отдельно проверить Contents и Workflows permissions.
6. После проверки отключить подключения, дождаться остановки/сверки, сохранить
   безопасную историю и удалить только собственные disposable resources.
   Не расширять GitHub auth scopes ради delete_repo: при отказе оставить
   точный список для владельца. Не переписывать существующие рабочие refs.

## 4. Матрица функциональной приёмки

Каждый сценарий F/G/A/CI ниже выполнить на обоих провайдерах, где не указана
локальная граница. Базовая матрица: 3 активных направления × 2 способа запуска ×
2 провайдера = 12 сочетаний, плюс disabled/manual defaults для обоих.

| ID | Сценарий | Ожидаемый результат и проверяемый oracle |
|---|---|---|
| F-01 | Создать подключение из общего списка; выбрать доступный hosted repository. | До сохранения записи нет; после — одна запись по UUID, disabled/manual, encrypted credential. Existing external catalog entry не преобразуется. |
| F-02 | GET/PUT/check/start/history/run/refs, valid и invalid DTO. | HTTP/OpenAPI совпадают; check не меняет settings/version/refs; start 202+UUID; disabled start 409; worker off 503. |
| F-03 | Два редактора сохраняют одну version. | Один успех, второй 409; draft второго остаётся; обновление версии не стирает edits; повтор с новой version работает. |
| F-04 | Потерять ответ PUT или POST runs и повторить прежний Idempotency-Key. | Одна settings version / один run; прежний readback. Другой payload, actor или repository с тем же ключом — 409. |
| F-05 | Поменять token, endpoint, provider, repository_path; отключить и включить. | Omitted token сохраняется, empty — 400; новый provider/instance требует token; вся история содержит исходные snapshots и UUID запусков. |
| F-06 | Менять настройки при queued/running/reconciling. | Запрещённые изменения — 409. Отключение отменяет queued, запрашивает stopping активного; история сохраняется. |
| G-01 | Equal branches/tags, повтор успешного запуска, disabled. | No-op; SHA, HEAD и default branch не меняются; disabled не выполняет передач. |
| G-02 | Внешний → Forge: пустой Forge, новая ветка, FF и внешний ref отсутствует. | Создание/FF только внутрь; локальные refs без внешнего аналога сохраняются; внешние refs не меняются. |
| G-03 | Forge → внешний: пустой remote, новая ветка, FF и локальный ref отсутствует. | Создание/FF только наружу; refs назначения без источника сохраняются. |
| G-04 | Both: Forge отстаёт, remote отстаёт; независимые новые ветки с двух сторон. | По каждому ref продвигается отстающая сторона; итоговые SHA равны. |
| G-05 | Diverged branch и попытка отката в каждом направлении. | Conflict; обе исходные истории и SHA сохранены; нет merge/rebase/force overwrite. |
| G-06 | Annotated/lightweight tags; одинаковый peeled commit, разные tag objects; blob/tree tags. | Сохраняется точный tag-object SHA; существующий другой tag — conflict; blob/tree передаётся без CI. |
| G-07 | Удалить branch/tag на одной стороне; добавить provider/PR/internal refs. | Удаление не распространяется; внутренние refs не переносятся; HEAD не меняется. В both отсутствующий ref может снова создаться из сохранившейся стороны. |
| G-08 | Защищённая ветка Forge: создание и FF; свободная feature рядом. | Protected ref не изменён, reason protected_branch, указан обычный PR; feature передана, общий итог partial. |
| G-09 | Защищённые refs на GitHub/GitLab, read-only token, правила workflow files. | Провайдер окончательно решает реальный push; отказ виден по ref, другие refs продолжаются. Dry-run не выдаётся за гарантию реальной записи. |
| G-10 | Конкурентный внешний push между наблюдением и публикацией; конкурентные receive-pack и PR merge в Forge. | Ожидаемый old SHA проверяется атомарно; чужое изменение не затёрто. Sync/receive/merge сериализованы по одному guard. Проверить также branch/tag с одинаковым коротким именем. |
| A-01 | Manual mode, local push, PR merge и ожидание более 5 минут. | Автозапусков нет; manual работает. |
| A-02 | Включить automatic и не нажимать запуск. | Сохранение не делает скрытый push; первая периодическая проверка через interval. Проверить настоящий production interval 300 секунд, а не только ускоренный fixture. |
| A-03 | Автоматический запуск по local receive-pack и по PR merge; затем потерять сигнал. | Сигнал даёт ближайший запуск по принятой политике направления; periodic восстанавливает работу без сигнала. Входящий обмен проверяется также без provider webhooks. |
| A-04 | Много сигналов до/во время запуска, два worker-процесса. | Один active run на repository, максимум одна следующая сверка от накопленных сигналов; общий concurrency не выше 2. |
| A-05 | Три и более repository, занятые guard/slots, перезапуск worker, изменение interval/concurrency. | Другие репозитории продвигаются; slots освобождаются; один репозиторий не исполняется параллельно; новая конфигурация валидируется. |
| A-06 | Архив до queue, между selection/admission и во время работы; разархивирование. | Новая запись приостановлена, история доступна; уже начатый эффект корректно сверяется; после восстановления работа продолжается без duplicate. |
| CI-01 | Принять A, затем ветка стала B до доставки события A. | Pipeline/config/checkout привязаны к commit A; не к более позднему head. |
| CI-02 | Повтор доставки; два worker; общий receipt с обычным push. | Ровно один pipeline на event key. После сбоя доставки событие остаётся и повторяется. |
| CI-03 | Push-policy disabled или выбрана одна из нескольких CI configurations; tag без commit. | При disabled — 0 pipelines; иначе выбран правильный project; blob/tree tag имеет терминальный ci_non_commit_tag. |
| CI-04 | Both с обратными сигналами и повтором одинаковых SHA. | Система выходит в no-op; нет бесконечных запусков или дубликатов pipelines. |

Oracle для Git — независимый `ls-remote` и чтение commit/tag objects до и после,
а не только текст «Успешно» из API. Для queue/CI — run/ref journal и точные
pipeline receipts/SHA. Доступ к runtime DB использовать только для безопасного
readback собственных QA UUID; raw credentials из БД не выгружать.

## 5. Сбои и восстановление

Эти сценарии выполняются в disposable Compose-проекте с отдельными DB/Git.
Не убивать установленный backend пользователя ради fault injection.
Контролируемый transport fixture воспроизводит точную фазу сбоя; затем
критические результаты подтверждаются с настоящими GitHub/GitLab endpoints.

| ID | Момент сбоя | Критерий |
|---|---|---|
| R-01 | Worker умер до claim / после claim, до записи плана. | Нет потерянной работы и лишнего эффекта; lease recovery продолжает запуск. |
| R-02 | Intent сохранён, push ещё не начат. | При old SHA допустим безопасный повтор; нет слепой перезаписи. |
| R-03 | Удалённый сервер принял push, ответ потерян до receipt. | Получатель сохраняет new SHA; recovery отмечает applied по реальному наблюдению; второго изменения нет. Требуется настоящий обрыв, а не только ручное создание unknown journal. |
| R-04 | После применения части refs / после receipt последнего ref, до terminal run. | Уже принятые refs сохраняются; остальные доводятся или получают отдельный отказ; truthful общий итог. |
| R-05 | Supervisor умер, Git/HTTPS helper жив, lease истёк; запущен второй worker. | Живой child удерживает guard. Второй worker не пишет; после завершения child сверяет реальное назначение. Повторить между отдельными OS-процессами. |
| R-06 | Provider недоступен при восстановлении unknown; затем доступен. | Reconciling, отсутствие нового push до наблюдения; после восстановления new/old/other SHA дают applied/safe retry/conflict соответственно. |
| R-07 | Отключить во время fetch, push, после принятия push, при timeout. | Stopping до подтверждения; принятый push не становится ложным canceled. Секреты и история сохраняются. |
| R-08 | Потеря DB/heartbeat/fence/slot во время Git. | Старый owner не подтверждает чужой generation; OS guard исключает параллельную запись; journal восстанавливается. |
| R-09 | Crash API во время check; disk full; Git missing/error/output limit; scratch cleanup failure. | Нет изменения live refs или секретов в артефактах; bounded завершение, безопасная диагностика; orphan scratch обнаруживается и убирается без чужих данных. |
| R-10 | Crash после создания pipeline до event_delivered; несколько повторов доставки. | Общий receipt возвращает прежний pipeline; event становится delivered; отказ CI не повторяет Git effect. |

Для каждого опыта сохранить только безопасные timeline, PID/run/generation,
expected/actual SHA, states, counters и итог cleanup. Утверждение «восстановлено»
требует readback после нового процесса, а не только завершения исходного future.

## 6. Авторизация, credentials и сеть

| ID | Проверки | Ожидаемый результат |
|---|---|---|
| S-01 | Anonymous, viewer/developer/admin, membership другого project, namespace active/archived, unbound legacy repo. | Права соответствуют действующей модели Forge; GET/history не расширяют write, чужой project binding не открывает repo. Полный HTTP middleware обязателен. |
| S-02 | Machine token: правильный/чужой/отсутствующий project; api read/write и git write по отдельности; expired/revoked/disabled service account. | Разрешены только нужные операции своего project. REV-01 закрыт; store-only fixture недостаточен. |
| S-03 | Omitted/empty/invalid/expired/revoked/read-only provider credential; смена провайдера/инстанса; потерянный ответ сохранения. | Понятный отказ без plaintext; сохранённый token не раскрывается; корректная ротация и идемпотентный replay. |
| S-04 | GitLab read_repository/write_repository без api scope; GitHub выбранный repo, Contents/Workflows. | Обмен требует только достаточных Git permissions, отказ избыточного/недостаточного доступа проверен реальным push. |
| S-05 | Запрещённый origin, HTTP, credentials/query/fragment в URL, path traversal/encoding, backslash/userinfo и различия разбора Rust/Git, redirect на разрешённый и запрещённый origin. | REV-04 закрыт; фактический Git endpoint соответствует проверенному origin; redirects не следуются; token не уходит на другой endpoint. |
| S-06 | Неверный TLS/hostname/expired certificate, private CA в системном trust store. | TLS не отключается; полный GitSession/API/worker повторяет ожидаемый результат, а не только внешний Git CLI. |
| S-07 | Ответы GET/ошибки, idempotency readback/HMAC, process argv/env, scratch config/objects, logs, frontend storage/URL, exports/build contexts/Git objects. | Точные canary credentials отсутствуют. Ciphertext в DB — ожидаемое исключение; plaintext не попадает jobs/runners/project secrets. Проверить и при аварийном завершении. |
| S-08 | CORS разрешённого/чужого origin, PUT preflight, malformed UUID/header/pager/filter. | REV-03 закрыт; invalid запрос даёт 4xx, не 500; разрешённый browser-origin может сохранять. |

## 7. UI и серверная пагинация

| ID | Сценарий | Ожидаемый результат |
|---|---|---|
| U-01 | Более 40 connections и available repositories, оба provider, несколько namespace, одинаковые имена. | Search/provider/state/group filters, offset/limit/total вычислены сервером; доступны поздние страницы; выбран правильный UUID. |
| U-02 | Все проекты ↔ конкретный проект; deep link, Back/Forward, вкладки, filters, run/ref offsets. | URL восстанавливает контекст; ссылка строки содержит настоящий namespace; чужой namespace не открывает editor. Токен/draft не появляются в URL/storage. |
| U-03 | Создание и редактирование, четыре направления, manual/automatic, ручной запуск при automatic, read-only. | Сохраняется настоящее состояние API, без fixture transport; недоступные команды заблокированы и объяснены. |
| U-04 | Более 20 runs и 50 refs; быстрый terminal, stopping, reconciling, paused archive/storage. | История и refs имеют окончательные статусы без reload; причины protected/access/conflict/unknown понятны; REV-02 закрыт. |
| U-05 | Ошибка первого GET и polling, CAS 409, потерянные ответы PUT/POST, повторное нажатие. | Ошибка не превращается в пустой список; draft сохранён; повторяется прежний command ID; controls восстанавливаются. |
| U-06 | 375/768/1026/1440 px, keyboard only, labels, focus, busy/error states, длинный URL/ref. | Нет недоступных действий и горизонтального разъезда формы; компактный верх; fixture screenshots дополнены реальной страницей. |

## 8. Последовательность запуска и критерий выхода

1. Сначала закрыть P1 REV-04, затем REV-01/02/03/05 постоянными регрессионными тестами. Уточнить поведение
   automatic pull с исходным контрактом. Сохранить Git receive/PR fencing из
   обновлённого Main при интеграции, устранить коллизию ADR, зафиксировать SHA.
2. На этом SHA выполнить Rust 1.88.0 fmt, clippy workspace/all-targets/integration
   `-D warnings`, workspace tests, полный normal PG suite и release workspace build.
   На Node 22.20.0 / pnpm 10.28.1 — frozen install, все frontend tests, lint/build,
   OpenAPI drift/compatibility к зафиксированному Main; проверить отсутствие
   preview entrypoint и transport replacement в production bundle.
3. Проверить migration 0093 на заполненной synthetic DB и восстановленной копии
   runtime DB. Сверить старые checksums и реальные bytes SQL из build source,
   включая Windows CRLF/LF. Сравнить бизнес-строки и Git refs; никаких пустых
   замен snapshots, down migration или удаления runtime volumes.
4. Подготовить две private пары repositories и минимальные credentials.
   Пройти F/G/A/CI на каждом провайдере и реальный UI → API → worker → Git →
   history/CI сценарий; сначала manual, затем automatic с настоящими 300 сек.
5. Пройти S/U и R fault matrix. Непроверенные клетки пометить NOT RUN с причиной,
   не заменять статус PASS ссылкой на похожий compound test.
6. Повторить release smoke на итоговом SHA. Проверить отключение worker и
   восстановление наблюдения без потери history. Downgrade старого бинарника
   допускается только после проверки его совместимости с applied migration 0093;
   простая подмена pre-0093 image не считается проверенным rollback.
7. Cleanup в finally, readback собственных остатков, аудит Docker groups и
   отсутствие credentials в Git/evidence. Сохранить итоговую матрицу:
   `case/provider/commit/fixture/run_id/expected/actual/evidence/status`.

Все контейнерные проверки — только Base ComposeHelper v2, уникальный
`sdlc-qa-*`/`sdlc-build-*`, owner/purpose, отдельные disposable volumes/network,
explicit daemon и finally cleanup. Перед новой локальной QA минимум 30 ГиБ.
Исходники экспортируются из immutable SHA; runtime, secrets, backups, rollback
images и shared caches не входят в build context и не удаляются при cleanup.

Готовность: нет открытых дефектов, нарушающих согласованный контракт; все
обязательные клетки обоих providers пройдены на итоговом commit; fault outcomes
подтверждены независимыми refs/receipts; полная installed UI цепочка проверена;
неизвестные результаты и отсутствие проверки явно отделены от успеха.
