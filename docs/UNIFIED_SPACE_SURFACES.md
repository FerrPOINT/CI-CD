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
[CENTRAL_PROFILE_ROLES](CENTRAL_PROFILE_ROLES.md). Общий допуск всё ещё отсутствует
на перечисленных Git/public/machine/runner поверхностях; ни одна из них не
объявляется готовой по успешному profile test.

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

## Контракт следующего этапа

[Owner provenance, Git/CI границы и совместимость v2](SPACE_ADMISSION_CONTRACT_V2.md)
уточняют реализацию. Nullable CI repository_id и unbound resources не получают
выдуманный default Space: требуется явное разрешение происхождения до activation.
