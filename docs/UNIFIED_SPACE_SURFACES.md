# Поверхности общего пространства Forge

Статус: трассировка текущего source и согласованная цель. Реализация общего
admission не завершена; этот документ не является serving/cohort receipt.
Полная route coverage дополнительно проверяется существующим authz_route_coverage;
публичные и специальные маршруты включаются в аудит наравне с обычными API.

## Происхождение ресурса

SpaceRef владельца Git-группы/репозитория обязателен независимо от M:N Tracker.
Чтение проекта A+B через A не открывает репозиторий пространства B или snapshot
его закрытых метаданных. Внутренний projects.id Forge — CI-конфигурация; это не
Tracker ProjectRef. Их соответствия задаются явно, по ID.

Каталог общего пространства может существовать без Git-группы. Создание Space
не должно автоматически создавать обязательный Git resource по старому тройному
owner-v1 контракту. При миграции сохранить существующие group/repository UUID,
историю pipelines, leases, logs, artifacts и credentials.

## Реальные границы и открытые gates

| Поверхность | Текущая точка / источник | Обязательное правило |
| --- | --- | --- |
| Human REST, principal, project/config lists | api/authz_mw; local profile; project/repository resolution | Fresh identity, общий read admission до pagination, прежняя роль для записи |
| Git discovery / upload-pack / receive-pack | git_host; canonical aliases; task_git | Owner Space допуск до public/legacy fallback, PAT scope/assignment дополнительно, исходный deadline на IO |
| Public repositories и legacy Git token | git_host::check_repo_access early returns | Space-bound ресурс не становится доступным из-за public или legacy token |
| Repo catalog/tree/raw/PR | repository_catalog, pulls, project scope resolver | Проверять настоящий owner Space; update/merge/push сохраняют прежние роли и write barriers |
| CI configuration/pipeline/job/log/artifact/download | CI project -> repository catalog -> owner group/Space | Авторизация до выдачи текста/ссылки/страницы, поток ограничен исходным сроком |
| Registered namespace reader/owner | namespace | Отдельные узкие машинные полномочия; не impersonation и не human all |
| Delegated task/source/workspace/delivery | delegated_credentials, task_git, task_delivery | Exact original actor, frozen V3 assignment/project/Space, fresh policy и существующий grant |
| Embedded/external runner, lease ACK/renew/complete | runner/runner_protocol | Новые starts и protected outputs требуют допуска; terminal/control drain отдельный подтверждённый протокол |
| Global/personal settings и справочники | existing route policy and owner tables | Явная классификация; отсутствие Space само по себе не публичность |

Human central profile boundary описан и проверен отдельно:
[CENTRAL_PROFILE_ROLES](CENTRAL_PROFILE_ROLES.md). Opt-in human Git admission
реализован отдельным source slice ниже. Полный допуск REST/machine/runner и
serving cohort остаётся открытым; profile test их не заменяет.

Отдельного SSH Git server adapter в рассмотренном source inventory не найдено.
Перед активацией проверить фактический serving ingress/listeners: это наблюдение
по исходникам не доказывает отсутствие SSH transport у установленного cohort.
Операторский SSH Firebat2 остаётся инфраструктурным доступом вне human UI policy.

Lifecycle owner-v2 должен подтверждать catalog state и реальный drain. Lease
expiry, административный status и истечение таймера не заменяют accepted terminal
receipt. Совместимость всех обслуживающих путей, populated access/roles diff и
rollback с новой policy обязательны до dev activation.


## Подтверждённый drain: source candidate

Legacy owner v1 сохраняет wire schema/operation keys, но его drained readback
больше не принимает lease expiry и один terminal status за подтверждение.
Каждый начатый attempt требует принятого completion receipt; поздний success
другой попытки или группы не скрывает потерянный ACK. Не начатая явно отменённая
lease без ACK/started-at не выдаётся за исполненную. Пустая группа сразу drained.

Реальный embedded owner выставляет completion_received_at только после
наблюдённого process exit и durable workspace completion/ACK; теперь это действует
для обычных CI pipelines, а не только task pipelines. Fencing tuple и terminal
состояние перепроверяются при записи marker; нулевой update сохраняет Held.
Исторические unknown receipts не backfill. Их расследование входит в migration
report; статус/таймер нельзя использовать, чтобы разблокировать архив.

Это исправление существующего readback не заменяет owner-v2 общий catalog,
frozen V3 assignment, admission всех Git/runner/machine routes и serving cohort.


Runner mutation failure lookup теперь привязан к уже аутентифицированному
runner_id, как основной lease query. Чужой runner получает 404 независимо от
active/expired/terminal состояния lease; собственный runner сохраняет прежние
fenced/expired/conflict ошибки. Completion, ACK, renew, control, secrets, log и
artifact paths используют один scoped error mapper. Это не выдаёт machine
полномочия чтения проекта и не заменяет общий admission job.


Адресная native qualification использует полную versioned SQLx схему, настоящий
аутентифицированный runner complete HTTP, две независимые группы и неизвестную
предшествующую попытку. Дополнительно реальный owned Git checkout и shell-процесс
обычного embedded CI доходят до process exit, workspace ACK, marker и drained.
Это изолированная QA без данных приложений, не модельное или установленное задание.

## Центральный PAT в Git Smart HTTP: source candidate

Git resolver сохраняет private VerifiedCentralToken рядом с локальными claims.
Отсутствие local token_id больше не превращает центральный PAT в unrestricted
session; scopes и original token deadline проверяются до и после ACL IO одним
общим завершением успешных веток. JWT wire schema и api_tokens не менялись.

Native PostgreSQL/HTTP проверяет private discovery: отсутствие git:read даёт403,
read PAT не пишет, git:write не заменяет membership developer, разрешённые read
и write advertisement доступны. Реальный membership SQL блокируется до expiry;
после снятия блокировки ответ401. Существующие profile/runner/drain проверки
также проходят. Следующий source slice сохраняет исходный deadline на native IO
и проверяется настоящим clone/push; общая Space policy всё ещё отдельный gate.

## Исходный deadline native Git: source candidate

Private GitAccess сохраняет исходные monotonic/wall deadlines VerifiedCentralToken
через ACL, lookup, owner waiter, native process и выдачу тела ответа. Discovery,
RPC и canonical aliases используют эту границу; срок не вычисляется заново
после SQL, lock wait или передачи в hook.

Receive сохраняет исходный reference-transaction hook и вызывает его с прежним
stdin/argv. После custom prepared hook проверяется исходный срок, перед native
ref preparation. Private invocation содержит только fixed boot-clock/wall bounds,
без bearer и новых credentials. Linux CLOCK_BOOTTIME учитывает suspend; округление
вниз не продлевает окно при переводе Instant в shell. Недостаток целой секунды
закрывает запуск. Это проверка перед подготовкой refs, не обещание жёсткого
реального времени для диска или отзыва уже отправленных клиенту байтов.

Native owner завершает только созданную им process group, ждёт native Git и
сохраняет lifecycle lease до завершения cleanup. Deadline охватывает наследуемые
stdout/stderr pipes, даже если parent process уже вышел. Timeout/неизвестная
публикация требует exact ref readback, автоматический повтор не разрешён.
Protected responses получают private, no-store и проверяют тот же срок при
выдаче body frames. Existing buffered IO и прежние legacy/public/task permissions
не превращаются этим исправлением в общий допуск.

Native HTTP/PG regression выполняет actual clone, разрешённый push, медленный
custom prepared hook до expiry, отсутствие новой ref и позднего эффекта hook,
сохранение соседней ref и bytes custom hook. Новый действующий proof снова
разрешает push; отказ custom hook по-прежнему запрещает публикацию. Отдельный
slow post-receive case подтверждает обратную границу: timeout после публикации
не означает rollback; actual ref readback обнаруживает опубликованную ref, а
поздний эффект hook останавливается. Исходные custom hook bytes сохраняются.
Это изолированная source qualification, не installed behavior или serving cohort.

## Opt-in human Git Space admission: source candidate

Аддитивная0097 создаёт выключенный forge_space_admission_state. Включающего API
нет: partial source нельзя активировать до полного совместимого cohort,
populated migration/access diff и rollback. Рабочая БД и runtime не менялись.

При включении discovery/RPC/canonical aliases требуют fresh central human
identity, точный git scope и Admin admission по repository UUID → group → SpaceRef.
Используется существующий bounded SDK/HTTP client с отдельным narrow registered
Admin reader credential (CICD_SPACES__ADMISSION_URL/TOKEN). Human bearer не
пересылается владельцу. Public и legacy token не обходят политику. Unbound,
missing v2 mapping и machine/старый task V1 human path закрыты без default Space.

Read admission открывает чтение без старого membership. Запись сохраняет прежние
роли/project restrictions, требует active owner confirmation и active local
catalog; revision mismatch закрывает запись до reconciliation. Read proof
для того же immutable SpaceRef сохраняет чтение при архивировании: переход
local revision не отзывает подтверждённый допуск раньше его исходного срока. Исходный срок
SDK/identity/owner передаётся native Git и hook, не начинается заново. Временная
недоступность владельца после expiry fail-closed. Local role не становится admin.

Scoped native HTTP/PG использует настоящий SDK client и mock owner только в
изолированном fixture: selected/all/future Space, viewer без membership,
actual clone, разрешённый push, public B/canonical alias/RPC/legacy/unbound и
machine/task fences. Настоящий owner archive/ACK сохраняет read-only доступ
при ещё действующем read proof предыдущей revision.
Это не приёмка с live Admin, Git push по V3, REST/catalog filtering или activation.

## Глобальный repository catalog read: source candidate

[ADR0035](adr/0035-repository-catalog-read-admission.md) сохраняет private verified
REST identity и original SDK proof до body handoff. Только
GET /api/v1/catalog/repositories фильтрует group UUID в SQL до count/search/page.
Его /{id} и group repositories read также защищены: hidden detail404, hidden
group items=[]/total=0, private/no-store, без преждевременного namespace lookup.
selected empty — пустой массив, OFF — NULL; local admin не bypass visibility.
all включает later owner mappings. Archived read сохраняется, sync_available
дополнительно требует active. Unbound rows не удаляются и не публикуются.

Scoped HTTP/PG cases проверяют counts/offset, hidden group/name, literal query,
selected/all/future/empty и original expiry после real count SQL wait.
Percent-encoded UUID не обходит допуск; actual clone использует clone_url из
допущенной карточки. Internal sync использует typed metadata accessor с прежним
ACL; это не новый human grant. Tree/blob/PR read вынесены в отдельный
[source candidate ADR0036](adr/0036-repository-browse-read-admission.md), пока без
native PASS из-за resource preflight. PR writes/CI/sync/machine остаются gates;
этот срез не делает весь REST или cohort готовым. Установка не менялась.

## Lifecycle owner v2: source candidate

[Необязательная Git-группа, original operation/revision, archive fence и real drain](SPACE_LIFECYCLE_V2.md) реализованы в source. Текущая native квалификация хранится отдельно в workspace report. Human Space admission, explicit new group attach и весь serving cohort ещё не завершены.

## Контракт следующего этапа

[Owner provenance, Git/CI границы и совместимость v2](SPACE_ADMISSION_CONTRACT_V2.md)
уточняют реализацию. Nullable CI repository_id и unbound resources не получают
выдуманный default Space: требуется явное разрешение происхождения до activation.

## PR write и task-link read: source candidate

[ADR0037](adr/0037-pull-request-write-admission.md) сохраняет private request
permit до SQL commit/native ref preparation. Common visibility не повышает роль.
Создание PR и counter атомарны; роль/lifecycle защищены прежними SQL/namespace
locks. Native owner сохраняет original fixed clock и custom hook, завершает
свою process group до освобождения lease. Unknown publication требует exact
readback без автоматического повтора; PR status не доказательство Git rollback.
GET catalog PR tasks теперь находится в repository read boundary.

[ADR0038](adr/0038-durable-pull-merge-recovery.md) добавляет durable intent до
publication. Повтор с текущими правами восстанавливает metadata по точным
source/candidate OID; concurrent recovery не повторяет native effect. Drift и
неподтверждённый исходный target остаются fail-closed, без ancestry/auto reset.

Полный cohort, реальный process restart/rollback rehearsal, task-link
write/machine evidence, CI/log/artifact/sync/V3 и serving routes остаются открытыми.
Существующие timestamps/receipts не backfill.
