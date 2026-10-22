# Человеческие роли и приложения Forge

Статус: feature-исходники, не установленная политика. Канон — Base
[HUMAN_APPLICATION_ACCESS](https://github.com/FerrPOINT/services-base/blob/651f20033b52493ded0939a97ca7872b3d62f7aa/docs/platform/HUMAN_APPLICATION_ACCESS.md)
и [ADR0011](https://github.com/FerrPOINT/services-base/blob/651f20033b52493ded0939a97ca7872b3d62f7aa/docs/adr/0011-human-roles-application-access.md).

Human user/admin определяется текущим Auth. Новый linked profile хранит
историческую локальную developer запись без auto-admin; response claims получают
текущиеuser/admin. Старые viewer/developer/maintainer и service contracts
сохраняются как технические/исторические значения и не являются новыми
человеческими назначениями.

Существующий exhaustive route registry отдельно объявляет Work,
Administration и Technical. Обычные Git/ветки/PR/разрешённыйmerge/CI/log/artifact
операции отделены от repository bindings, external sync, delivery configuration,
secrets/runners/environments/schedules/webhooks и production effects.
Чтение job logs/test-report/artifacts остаётся рабочим. Прямая legacy запись
этих evidence через job API требует admin; runner пишет через собственный
lease protocol. Выдача Forge не позволяет пользователю изготовить результат
исполнения. Retry/start остаются рабочими действиями.
HTTP middleware проверяет этот gate независимо от прежнего scope/project role.
Registered delegated credentials сохраняют собственные exact paths/fences;
они не превращаются в human principal.

Один live bridge proof используется на входе HTTP запроса. Synthetic claims
не получают новый15-минутный срок: максимум60с и не дольше исходного proof.
Auth errors/app denial не превращаются в legacy SSO fallback.

До активации остаются открытыми native Git/public/legacy transport admission,
точный effect/commit deadline, downloads, runner stages/current owner,
production delivery controls, frontend capabilities/cache, populated migration,
rollback и совместимый serving cohort. Одна route classification не доказывает
эти границы. Политика пространств независима и должна интегрироваться без
переписывания чужих worktrees. Source/tests/image/dev/IAB/real job различаются
в отчёте поставки.

## Совмещение с пространствами и native Git

Lifetime-aware identity SDK проверяет приложение в том же Auth readback.
Срок сохраняется в VerifiedRequestIdentity, response body и существующем
GitDeadline/ref-transaction hook. Namespace proof дополнительно сужает доступ;
не возникает второго обновляемого окна. Human request сохраняет central role,
а старые product role/provenance записи остаются историей.

Central human PAT использует `ci-cd:read/write` также для Git в составе Forge;
технические Git credentials сохраняют свои точные `git:*` scopes и project pin.
После обязательной app policy public Git и legacy operator token не обходят
проверку. Этот flag не включает space policy. Сам task V1 не считается
доказательством нынешнего human access: при app policy отдельный adapter
подтверждает текущего владельца через зарегистрированный Tracker readback
и Auth. При одной space policy без app policy прежний V1 остаётся запрещён.
Реальное установленное задание остаётся gate до активации.

Отдельные product PAT и HTTP/Git paths проверяются native fixtures; это не
полная квалификация всех credentials и installed cohort. Request/body
checks не заменяют все DB/effect fences при independently enabled app policy,
проверки pipeline/runner stages и полную функциональную матрицу.

## Подключение общего UI допуска

ProtectedRoute получает общий допуск ci-cd перед space boundary. Git/PR,
пайплайны, отчёты и артефакты остаются рабочими маршрутами. Синхронизации,
конфигурации, runners, secrets, environments, schedules, webhooks, настройки,
пользователи и общий аудит требуют admin. Закрытый configuration-компонент
не монтирует reader/mutation hooks. Навигация фильтрует административные и
недоступные cross-app ссылки. Клиент проверяет общий контекст после headers,
JSON/download body; потеря приложения не запускает новый SSO.

Это source-реализация; полный административный matrix, серверные эффекты,
миграционная квалификация и dev/IAB остаются отдельными этапами приёмки.

## Сохранённые продуктовые personal tokens

Существующий cicd_ credential проверяется по hash, active local user,
revocation, expiry и прежним scopes/project pin. Единственное соответствие
центральному владельцу — users.central_sub с корректным UUID. Email, username
и старый local role не восстанавливают полномочия. Владелец должен сейчас
иметь ci-cd; отсутствие/неопределённость соответствия закрывает доступ и
подлежит миграционному разбору, без создания нового пользователя или токена.

Для обязательной app policy сервер использует общий SDK owner reader через
CICD_AUTH__CENTRAL_APPLICATION_ACCESS_READER_TOKEN и настроенный issuer.
Это отдельный ограниченный machine credential, зарегистрированный в Auth
для application-access:read; пользовательский bearer им не заменяется.
Reader создаётся один раз, подтверждение текущего владельца получает каждый
запрос. Owner proof пересекается с исходным сроком продукта и не продлевается.

Git-only scope не открывает API. Project-bound token не открывает глобальные
экраны даже у admin. Текущий user не получает admin от сохранённой локальной
роли. Понижение/отзыв закрывает API/Git при следующем подтверждении; HTTP
handler и body используют общий RequestAuthority с тем же исходным сроком,
а Git сохраняет его до ref transaction.

Технические SAT/runner/child protocols остаются отдельными. Product PAT передаёт
его подтверждённого центрального владельца в общий SDK native-owner adapter
пространств. Он не создаёт bearer/session и сохраняет original owner/credential
clock. API и Git scopes остаются независимыми; каталог фильтруется по admission
до pagination. Эта source-композиция проходит отдельную native qualification;
полная квалификация всех SQL/effect fences, очередей и installed delegation
ещё является gate активации.

### Назначение credential при чтении центрального справочника

Сохранённый Forge PAT действует только в Forge. Текущая роль центрального
администратора не превращает его в credential Auth. При настроенном central Auth
GET /api/v1/users остаётся адаптером центрального справочника и отклоняет Forge PAT
до пересылки. Для этого адаптера требуется центральный credential; пользователями
управляют через Admin Console. Административные операции самого Forge, например
настройка runners, пересекают текущую роль владельца и scopes продуктового токена.


## Task Git child и текущий владелец

Adapter task_git сохраняет строгий GitContextV1 и независимую проверку
child introspection, exact scopes/grant, assignment/lease, зарегистрированного
Tracker SourceBinding, project/repository, ref и OID CAS. Из подтверждённого
SourceBinding берётся центральный owner UUID. Auth должен сейчас допускать
ci-cd, task-tracker и fleet-control. Header сам по себе owner не подтверждает;
reader token не подменяет child credential и не создаёт human principal.

При независимой space policy общий SDK owner read дополнительно подтверждает
точное пространство и активную ревизию локальной проекции. Сроки child, Tracker
lease, owner и пространства пересекаются до Git IO, callback и ref preparation.
Новый readback callback не продлевает исходный GitDeadline операции. Потеря Auth
даёт503; отсутствие приложения или пространства —403 без SSO fallback.

Native fixture task_git_owner использует реальные HTTP, PostgreSQL и Git:
разрешённый candidate push, отсутствие legacy pipeline, отзыв Fleet, отказ Auth,
пустой/разрешённый space admission и запрет позднего custom hook/ref update после
истечения исходного owner proof. Это изолированная qualification, а не dev job.


## Владелец CI и новые runner stages

Аддитивная migration0101 сохраняет pipeline_human_owners по центральному UUID
в той же транзакции, что pipeline/plan/jobs/queue. Owner берётся из проверенного
HTTP origin, а не из JSON, email или исторического local admin. Replay original
key не может заменить владельца. COMMIT ограничен исходным request proof.

При app policy обычный Git push передаёт доверенный in-memory origin владельцу
native receive. После успешного native outcome backend читает сохранённые hook
updates и точный текущий OID каждой ссылки, затем использует прежнее CI decision
и idempotency key. Новый owner header/credential не вводится. Task Git по-прежнему
не создаёт legacy pipeline. Самостоятельный legacy callback без подтверждённого
human origin не может поставить CI в очередь. Custom hooks сохраняются; unknown
Git outcome не повторяется автоматически и требует exact ref readback.

Перед embedded preparation/spawn и внешним offer подтверждается текущий ci-cd
владельца. Внешние ACK/renew, secrets и artifact phases дополнительно пересекают
native lease с текущим owner. ACK/renew и secret envelope не продлевают исходное
окно подтверждения. Runner credential продолжает доказывать только собственную
custody/scopes; он не обходит human admission. Task pipelines сохраняют отдельные
Tracker/child/assignment проверки; внешний runner не захватывает embedded-only
native task. Технический учёт результата не считается правом нового запуска.

Полная native fixture подтверждает Git/API ownership, успешную обычную shell
команду в disposable workspace, отказ до lease/workspace после отзыва, remote
poll rollback, ACK/renew/secrets при разрешённом доступе, отзыв и Auth503.
Срок secrets/lease ограничен исходным30s proof. Проверки old product PAT,
space composition и позднего SQL/Git эффекта сохранены в той же fixture.

Исторические pipeline без подтверждённого owner не получают первого admin
автоматически. Их старые attempts закрываются как migration pending. Полная
миграция, artifact publication, rollback и установленный cohort проверяются
отдельно; native fixture не заменяет dev-приёмку.

### Расписания и явные новые попытки

Migration0102 сохраняет центрального владельца настройки расписания и отдельно
владельца каждого materialized fire. Последующее редактирование расписания не
меняет владельца уже созданного fire. Перед постановкой очередного запуска Auth
заново подтверждает ci-cd и административную роль первоначального владельца.
Auth503 оставляет исходный fire/key для повторного подтверждения; отзыв роли
закрывает запуск. Историческое расписание без owner остаётся migration pending,
без выбора первого admin. Delivery approval/rollback требует текущего admin;
reconcile результата сам по себе не создаёт нового запуска.

Migration0103 отделяет инициатора повторной попытки от истории pipeline. Retry
и ручной старт являются новой операцией текущего пользователя: новый attempt
сохраняет его центральный UUID и исходный request clock транзакционно с queue.
Исторический pipeline owner сохраняется. Незапущенные старые queued attempts
при явном retry заменяются новыми, успешные результаты сохраняются. Повтор
production delivery требует admin; generic retry native task pipeline запрещён,
поскольку не доказывает новый Tracker/child grant.

Runner preparation, lease claim, ACK/renew, secrets и publication подтверждают
именно выбранный execution attempt. Между предварительным подтверждением и
claim нельзя подменить attempt; COMMIT claim ограничен его исходным proof.
Первый attempt использует owner pipeline, последующие — собственную запись.

Native HTTP/PG/Git fixture проверяет Auth503 и повтор одного fire, отзыв admin,
редактирование расписания и новый fire, historical missing owner; затем реальное
падение shell-команды, retry другим пользователем и ручной старт с новыми owner,
успешное исполнение команд и сохранение первоначальной истории.

### Публикация артефакта и upload-сессии

Публикация получает исходный clock текущего HTTP/runner этапа. Запись metadata,
аудита и завершение resumable session выполняются в одной транзакции с deferred
COMMIT fence. Begin/chunk/abort также сохраняют исходный clock подтверждения lease.
Неуспешный COMMIT удаляет только свой неподтверждённый файл. При неизвестном
результате COMMIT файл сохраняется до readback; ошибка транспорта не является
доказательством rollback и не разрешает удалить опубликованный артефакт.

Завершение сессии сериализовано по её записи и повтор возвращает прежний artifact
ID. Публикация native job использует fresh stage confirmation и его исходный срок;
image/deployment publication сохраняет существующий native authority deadline.
Чтение результата не создаёт право публиковать output от имени runner.
Native HTTP/PG fixture подтверждает разрешённую публикацию, истечение в реальном
COMMIT без metadata/audit/file, отзыв перед chunk, откат chunk high-water mark и
положительный повтор завершённой сессии без дублей.

Заполненная migration fixture использует прежнюю схему95, реальные проекты,
репозитории и pipeline history из закрытой копии dev. Она сравнивает прежние
колонки всех таблиц и SQLx receipts после миграций и повтора; новые owner/role
таблицы не заполняются догадками. Independent space activation остаётся OFF.
Восстановление pre-policy копии отдельно проверяет возврат исходной схемы;
это не доказывает совместимость прежнего binary после новой activation.

## Обычные SQL-записи и исходный срок подтверждения

Serving SQLx pool создаётся через `request_database::pool_options()`. При первом
открытии и повторном checkout соединение получает исходный clock человеческого
запроса. Технический checkout и release очищают это состояние; credential,
роль или разрешение из clock не возникают.

Аддитивная0104 ставит deferred COMMIT guard на все product tables. Миграции,
добавляющие таблицы, вызывают `forge_install_human_request_clocks()`; native
регрессия проверяет полноту покрытия. Обычные autocommit-записи больше не могут
сохраниться после истечения прав во время row lock. Ошибка guard возвращает503.

Один deferred trigger недостаточен: последующее deferred действие может
задержать уже начатый COMMIT. Поэтому SQLx checkout ограничивает PostgreSQL17
statement/transaction timeout остатком того же исходного proof, сохраняя более
строгие defaults. При release defaults восстанавливаются. Это использует
[штатные PostgreSQL timeouts](https://www.postgresql.org/docs/17/runtime-config-client.html#RUNTIME-CONFIG-CLIENT-STATEMENT),
а не новый TTL. SQL timestamp дополнительно проверяется при COMMIT.

Native HTTP/PG regression воспроизводит row-lock expiry и поздний deferred
COMMIT и проверяет свежую разрешённую запись после повторного использования
пула. Git/ref/files/runner effects сохраняют собственные ранее описанные guards.
