# Человеческие роли и приложения Forge

Статус: feature-исходники, не установленная политика. Канон — Base
[HUMAN_APPLICATION_ACCESS](../../services-base/docs/platform/HUMAN_APPLICATION_ACCESS.md)
и [ADR0011](../../services-base/docs/adr/0011-human-roles-application-access.md).

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
