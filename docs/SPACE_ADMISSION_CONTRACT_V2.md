# Forge: контракт общего пространства v2

Статус: согласованная цель и source trace перед реализацией. Это не installed
behavior и не cohort receipt. [Матрица текущих поверхностей](UNIFIED_SPACE_SURFACES.md)
сохраняет отдельный статус каждой границы.

## Identity и чтение

Admin владеет SpaceRef/политикой selected/all. Auth подтверждает immutable subject,
активность, session/PAT и scopes. Forge сохраняет свой UUID, роль и memberships.
Подтверждение общего допуска не выдаёт роль редактора/администратора, Git write,
secret resolve или машинную execution capability. Отсутствие политики после
активации закрывает доступ. Нельзя получать all из имени, email или role hint.

Human proof строится сервером через общий bounded SDK, не из query/body/header.
Срок — минимум исходных identity/admission/token подтверждений и60 секунд.
Downstream/cache/stream не начинают новый срок. Недоступность после expiry
закрывает защищённое чтение и новые эффекты. Машины имеют отдельные ports;
allowlist человеческого PAT не делает его service account.

## Происхождение ресурсов

| Ресурс | Фактический owner/path | Целевая граница |
| --- | --- | --- |
| Git group | git_groups → forge_namespace_bindings; новый Space mapping по ID | Конкретный SpaceRef; сохранение local group UUID и старых aliases |
| Репозиторий, tree/raw, PR | repository_catalog.group_id → Git group | Допуск именно к owner Space, затем прежние write/admin permissions |
| Обычный CI project/pipeline/job | projects.repository_id → repository_catalog; pipeline/job FK | Owner Space источника; internal projects.id не Tracker ProjectRef |
| CI config без repository_id | Прежняя nullable связь поддерживается source | Нужна явная подтверждённая принадлежность; не выбирать первый repo/project |
| Task source/execution | Existing exact intent/assignment/workspace journals | Явные ProjectRef и конкретный Space при Space-bound data; frozen V3 + child/lease restrictions |
| Logs/artifacts/download | Job/attempt → execution provenance | Нельзя открыть B output через видимый Project A+B по допуску толькоA |
| Staged/personal/global/internal | Собственный owner/route contract | Явная классификация; null Space не означает public |

Существующие nullable/unbound ресурсы сначала инвентаризируются и классифицируются
в миграционном отчёте. Их нельзя удалять или произвольно привязать, чтобы пройти
gate. Если принадлежность не подтверждена, активация остаётся закрытой.
Для каталога фильтрация и dedup выполняются до count/page; подпись/alias не ключ.

## Git transport и совместимость

Smart HTTP discovery/upload-pack/receive-pack, public repository, legacy token,
canonical alias и task Git входят в один audit. Public/legacy early return не
обходит допуск Space-bound ресурса. PAT git:read/git:write и проектные ограничения
сохраняются дополнительно. Git read permission не становится Git write.

Proof проверяется перед IO и изменением refs. Receive hooks сохраняют прежние
protected/default branch и exact CAS правила; более короткий deadline не теряется
в private invocation. Медленная передача не даёт записать refs по expired proof.
Дочерний процесс/stream и неизвестный результат publication учитываются отдельно.
SSH adapter не найден в рассмотренном source; фактические serving listeners и
ingress всё равно проверяются до activation. Операторский SSH не является Git API.

## Lifecycle и данные

Space создаётся без Forge group по умолчанию. Опциональный Forge resource
подключается отдельной версионированной owner командой. Старый owner v1 с
обязательной прежней схемой не ослабляется и не выбирает Project вместо Space.
Новое owner mapping сохраняет public Space ID, group/repo UUID, refs, aliases,
pipelines, attempts, leases, logs, artifacts и прежние receipts/checksums.

Admin владеет create/rename/archive/restore. Forge подтверждает свой resource
revision, original operation и реальный drain. Lease expiry, таймер, terminal
status без принятого completion receipt и успех другой попытки не являются ACK.
После archive fence новые starts/writes закрыты; старые данные не удаляются.
Повтор/lost ACK не создаёт группу/репозиторий/attempt повторно и не объявляет
успех до подтверждения owner.

## Приёмка перед включением

Проверяются соседние A/B пространства, read PAT и write roles, raw/public/legacy
Git и API, counts/page, artifacts/logs/download, scoped/delegated/runner paths,
истечение до effect и при idle IO, archive/start/role races, original-key replay,
unknown drain и restore. Нужны populated migration/access diff/role provenance,
совместимый serving cohort и rollback с новыми данными. Старый image без admission
не допускается после activation. Source/native/CI/image/dev/IAB/real job —
отдельные доказательства.
