# Рабочие окружения проекта

Статус: реализация; квалификация и установка фиксируются отдельно.
Решение: [ADR-0043](adr/0043-managed-development-workspaces.md).
Общий контракт Base: [WORKSPACES](https://github.com/FerrPOINT/services-base/blob/67c15a3090afb05a44994c386405b23612b220a8/docs/platform/WORKSPACES.md)
и [operator runbook](https://github.com/FerrPOINT/services-base/blob/67c15a3090afb05a44994c386405b23612b220a8/docs/platform/WORKSPACE_OPERATOR.md).
Профиль продукта R1: `deploy/pdlc/workspace.yaml`; инструкции рядом с ним.
Пилот использует один существующий R1; creator принимается на disposable fixture
без второго постоянного R1 или нового DNS. Оставшиеся обязательные работы:
[план завершения R1](https://github.com/FerrPOINT/services-base/blob/67c15a3090afb05a44994c386405b23612b220a8/docs/plans/r1-managed-workspace-completion-20261101.md).
Ссылки на документы не меняют SDK/UI/operator pins установленного продукта.

## Модель и права

Workspace сохраняет identity, исходники, инструменты, данные и конфигурацию
между сессиями. Выключение завершает вычислительную часть. Завершение задачи
или сессии не удаляет окружение. Прежние TaskWorkspace/CI workspace API сохраняются.

`development_workspaces.project_id` — delivery configuration Forge; реальные
`tracker_instance_id/tracker_project_id` хранятся отдельно. Репозиторий и
environment проверяются по конфигурации, Tracker project — по project projection.
Один опубликованный target/reservation и один environment принадлежат одному
экземпляру. Соседние экземпляры имеют независимые identity и данные.

Читатель проекта видит карточку и доступные ему результаты. Разработчик начинает
сессию реальной задачи Tracker, подготавливает исходники и запускает проверки.
Подключение, создание и явное восстановление требуют сопровождающего проекта.
Service account не подменяет человека. Сохранённый владелец повторно проверяется
перед исполнением; принятие команды не сохраняет его права навсегда.

### Developer configured-cli-v1

Адаптер постоянной machine identity добавлен в исходники; его локальная
квалификация, поставка и настоящий initial/rework учитываются отдельно.
Он использует те же API и `cicd-cli dev-workspace`. Старые task-workspace API
и legacy delegated credentials сохраняют свой отдельный протокол.

Developer получает Central PAT со scopes `ci-cd:read` / `ci-cd:write` и явное
членство в нужной конфигурации Forge. Локальный профиль сохраняет тип
`service_account`; он не становится human administrator. Собственное назначение
проверяется через существующий registered namespace reader Tracker с
`include_execution=true`: текущие configured-cli-v1 Development/Rework,
project/task, agent, machine subject, assignment/execution UUID,
версия назначения и подтверждённые исходники. Tracker также перепроверяет
актуальный доступ назначенного исполнителя к проекту.
Текущее назначение в `Ready` разрешает открыть сессию, читать Git и подтвердить
подготовку точного SHA только при `execution.workspace_preparation_authorized=true`.
Отсутствующее или false поле закрывает подготовку; прежний timestamp не заменяет
этот допуск. Закрытие собственной сессии доступно для того же назначения,
пока оно видимо в Tracker. Проверки и push требуют `InProgress`.
Fleet сохраняет исходный ID, payload и ожидаемую версию до отправки команды.
После потери ответа оператор сверяет исходную операцию Forge. Неизвестный
результат удерживает сессию до сверки.
`source_binding.development_workspace_id` должен совпадать с UUID выбранного
окружения. Owner выбирает эту optional ссылку в существующем source policy;
Forge source reader проверяет реальный instance/repository/namespace перед
подтверждением. Старые source receipts без поля сохраняют прежние hashes и
человеческий сценарий, но не открывают Developer-доступ догадкой по проекту.
Перед выдачей членства owner проверяет identity через существующий
`GET /auth/principal`: ответ содержит только собственные local subject/scopes,
не проектный доступ. Этот local subject передаётся существующему
`cicd-cli member upsert --project ... --user ... --role developer` под owner
credential. Сам вход с машинным PAT не выдаёт членства или иных прав.

В central mode этот membership API принимает только уже проверенную машинную
identity: её локальная строка создаётся по действующему central PAT через
`GET /auth/principal`, имеет `role=service_account` и неизменный `central_sub`.
Human owner с текущими правами сопровождающего проекта выдаёт или отзывает
доступ прежними `member upsert` / `member remove`; машина не выдаёт доступ себе.
Новый grant отключённой машине запрещён, отзыв её прежнего членства разрешён.
Запись membership и audit коммитятся вместе и ограничены исходной authority
запроса. API по-прежнему не меняет central human grants: ими владеет Auth.
Auth directory содержит только людей и не служит регистрацией machine principal;
её ответ не содержит поля роли. Значения токенов owner и агента не передаются
друг другу и не входят в payload membership.

Тот же PAT используется для `cicd-cli pr create` и `cicd-cli pr for-task`.
Forge проверяет членство Developer и привязку PR к задаче. Отдельный
`CICD_PR_API_TOKEN` в конфигурации этой роли переопределил бы identity.
После дополнительного push Developer продолжает работу с тем же PR.
Текущий SHA его ветки показывает `cicd-cli pr review-context`; ссылка
на задачу сохраняет SHA на момент привязки.

Доступны чтение проекта, своя сессия, prepare и опубликованные Forge-проверки
без runtime-доставки. Create/register, Start/Stop, Build, recovery, reconcile,
произвольные pipelines и runtime deployment этим доступом не разрешаются.
Для публикации образа владелец может явно объявить Check с `kind: build`,
`executor: forge-build`, `requiresRuntime: false`, без `commands`. Его опубликованный
рецепт содержит ровно одну job только с `image_publish`; существующий typed spec
проверяет context, Dockerfile, registry repository и distinct artifact paths.
Произвольные shell jobs, зависимости, mixed recipes и `helm_deploy` запрещены.
Обычные `requiredDeveloperChecks` остаются проверками `forge`; publication Check
может дополнять их. Результаты jobs,
логи и artifacts читаются только от его собственных workspace-операций.

Сессия хранит неизменное назначение и source binding. Новая команда и worker
сверяют его заново; перед runner-эффектами повторно проверяются исходная identity
и то же назначение. В private execution plan хранится зашифрованная credential
исходной операции; API её не возвращает. Смена назначения не переписывает старую
сессию. Агент не завершает чужую или устаревшую сессию; explicit recovery остаётся
у сопровождающего. Readback точной исходной операции доступен после смены
назначения при сохранённом доступе к проекту и не повторяет эффект.

Перед первым сообщением агенту устанавливаются CLI, Base source helper,
постоянные secret references и обычный Git credential helper, читающий
`CICD_API_TOKEN` из окружения процесса только для принятого Forge Git origin.
Значение токена не записывается в URL, Git config, командные аргументы или задачу.
Git redirects выключаются; настройки helper передаются через штатные
`GIT_CONFIG_COUNT` / `GIT_CONFIG_KEY_n` / `GIT_CONFIG_VALUE_n`, чтобы источник
не зависел от пользовательского global config. Обычная подготовка человека
по-прежнему удаляет API token из source helper. Для текущего configured агента
CLI сохраняет его постоянное окружение и добавляет только nonsecret Git username
`workspace-<workspace UUID>` для конкретного prepare.

Git username выбирает окружение, но не является identity или разрешением.
Workspace hooks используют отдельный внутренний callback
`/internal/development-workspace-git-check`; прежний `/internal/task-git-check`
сохраняет свой request contract и не принимает workspace credential.
Сервер требует PAT текущего владельца живой сессии и заново читает назначение.
Push допускает один fast-forward ref — `candidate_ref` из Tracker source binding,
с сохранённым base ancestor. Проверка повторяется в native pre-receive и
reference-transaction/prepared; protected/custom hooks сохраняют право отказа.
Default/foreign refs, удаление, force push и автоматический запуск delivery
через post-receive запрещены. Hook callback использует прежний private endpoint
с отдельным зашифрованным purpose-bound ticket; это не агентский bootstrap.

Перед каждым новым назначением UUID окружения берётся из актуальной задачи и
сверяется с карточкой Forge, repository, установкой, branch и commit. Отсутствие
этой привязки — ошибка входных данных; имя каталога и прежний чат её не заменяют.
Windows checkout и постоянный checkout агента независимы; dirty-файлы между ними
не переносятся. Обычный push остаётся обычным Git:

```sh
git -c credential.username="workspace-$WorkspaceId" push origin "HEAD:$CandidateRef"
git -c credential.username="workspace-$WorkspaceId" ls-remote origin "$CandidateRef"
```

После commit/push, сверки remote SHA и terminal checks Developer завершает свою
сессию с явным session UUID до смены назначения. Затем публикует принятый отчёт Workflow,
обычный итоговый комментарий и typed result Tracker. Переход в Review выполняет
сервер после подтверждённого завершения run. Reviewer, Tester и DevOps сохраняют
свои обязанности; Developer не выполняет merge/deploy.

Задача ручной сессии проверяется существующим registered namespace reader
Tracker (`CICD_NAMESPACE__TRACKER_*`). Ключ разрешается в scoped catalog, затем
UUID/project/namespace/state перечитываются через namespace task context.
Отдельный SDLC execution или его credential для этого не требуются. Весь lookup
ограничен тремя секундами и 10000 metadata entries; известный UUID читается прямо.

Для машинной сессии профиль с `requiredDeveloperChecks` закрепляется в
`result.developerCheckPolicy` исходной операции `begin_session`. Квитанция
содержит хеш профиля и ключи рецептов для проверки результата Developer.

Одна сессия получает право записи. Таймаут не освобождает её. Stop не завершает
сессию и отклоняется при активной работе. Проверки, сборки, delivery, pending
leases и неподтверждённое native completion удерживают свой instance.

## API

Пути относительны `/api/v1`; DTO описаны в [OpenAPI](../openapi/openapi.yaml).
Ответы имеют `private, no-store`. POST возвращает сохранённую async операцию.

| Метод | Путь | Назначение |
| --- | --- | --- |
| GET/POST | `/projects/{project_id}/development-workspaces` | Список и register/create |
| GET | `/projects/{project_id}/development-workspace-profiles` | Опубликованные targets и право регистрации |
| GET | `/development-workspaces/{workspace_id}` | Карточка, сессия, операция, доступность действий |
| GET/POST | `/development-workspaces/{workspace_id}/operations` | История и новая команда |
| GET | `/development-workspaces/{workspace_id}/operations/{operation_id}` | Исходная операция и outcome |
| POST | `/development-workspaces/{workspace_id}/operations/{operation_id}/reconcile` | Явная сверка unknown |

Списки поддерживают limit/offset. Новая команда содержит operation UUID,
expected_version и command: observe/start/stop, begin_session/end_session/
recover_session, prepare/check/build. Create/register выбирают опубликованный
target UUID. API не принимает shell, pipeline, kubeconfig или путь сервера.

Состояния: queued/running/succeeded/failed/unknown. Actor, исходный owner, payload,
version и execution plan сохраняются. Точный повтор возвращает исходную запись
даже после изменения каталога или версии workspace. Иной payload/actor с тем же
ключом даёт 409. После потери ответа читать исходный UUID. Terminal history и
результат не переписываются; новый UUID не снимает unknown.

Prepare содержит полный commit каждого репозитория и ограниченный локальный
отчёт. Check выбирает опубликованный рецепт и подготовленный source. Подготовка
инструментов, runtime readiness, занятость и тесты показываются раздельно.

Typed jobs `image_publish` и `helm_deploy` исполняют закреплённые планы из
`repository` и `development_workspace`. Источник в записи `pipeline_plans`
должен совпадать с источником внутри плана. Для обоих источников сохраняются
проверки commit, hash плана и конфигурации, состава job и прав execution target.
`legacy_template` не допускается к typed jobs. Изменение каталога не меняет
уже сохранённый план и не требует повторной отправки pipeline.

Для опубликованного deploy-only рецепта владелец сессии передаёт ID
завершённой сборки через `dev-workspace check --source-pipeline UUID` (поле
`source_pipeline_id` команды `check`). Forge использует опубликованный рецепт
`forge-delivery` с одной Helm job и оставляет в плане только доставку.
Другие рецепты этот параметр не принимают. Настоящий publication pipeline
должен успешно завершиться для того же проекта и commit; Forge проверяет его
immutable plan/config hashes, typed publication job и исходный image descriptor.
Квитанция содержит `sourcePipelineId`.

Configured DevOps использует собственный Central PAT и собственную writer session
того же зарегистрированного workspace из sealed Tracker source binding. Штатный
`begin-session` разрешён для текущего Deployment assignment в Ready до dispatch,
без Developer preparation commit. Сам delivery Check требует InProgress,
опубликованный executor `forge-delivery`, явный `--source-pipeline` и точный HEAD
назначенного candidate ref. Prepare, Build, Git transport и lifecycle-команды
DevOps не выдаются. Developer сохраняет только `forge`, non-runtime checks без
source pipeline. Его дополнительный publication Check использует только явно
опубликованный `forge-build` и не получает Helm target rights. Перед созданием
pipeline Forge сверяет SHA с текущим candidate HEAD; последующие claim/effects
сохраняют этот immutable SHA и повторно проверяют тот же PAT, assignment и
publication-only shape сохранённого execution plan. Текущий machine subject, assignment/execution/version, source,
namespace, repository и project сверяются при записи authority и перед каждым
новым runner effect. Существующие human pipeline/session правила сохраняются.

Exact-key `dev-workspace operation --workspace UUID --operation-id UUID` сохраняет
оригинальные request, pipeline ID и результат. Для machine-authority delivery Check результат
добавляет `deploymentObservation` формата
`forge/kubernetes-deployment-observation/v1`: phase, deploymentId, pipelineId,
attemptId, leaseId, generation, sourceCommit, namespaceUid, releaseRevision и
вложенную receipt `forge/kubernetes-deployment/v1`. Receipt дополнительно связывает
sourcePipelineId, cluster/namespace/release, immutable images и health readback.
Forge читает только deployment исходной operation/pipeline и проверяет observation
тем же typed validator относительно сохранённого execution offer. Списки
последних deployments и агентский отчёт не выбирают квитанцию. Успех требует
успешного pipeline и phase `deployed`; отсутствующая или несовместимая observation
даёт `unknown`, `workspace_deployment_receipt_unconfirmed` и
`requiresReconciliation:true`, без пустого успеха. Human операции сохраняют прежние
result/state semantics; новое обязательное readback условие к ним не применяется.

GET оригинальных operations/pipelines остаётся read-only по project membership
и собственному PAT читателя: старое назначение и session не присваиваются DevOps.
Связь accepted Tester → Developer result/artifacts проверяет Fleet. Сам Forge
подтверждает publication/deployment execution, а не смысловое принятие отчётов.
Нужен настоящий опубликованный application target и delivery recipe; source-only
workspace не является доставленным приложением.

Временное окружение связывается с целью `workspace_target`. Start и Stop
управляют процессами, сохраняя checkout и данные. Сопровождающий может закрыть
подтверждённо остановленное окружение командой `dev-workspace close` или
перевести подтверждённо работающее в постоянный стенд через
`dev-workspace convert-to-stand`. Обе команды сохраняют историю и требуют
отсутствия сессий и незавершённых работ. После закрытия цель запрещает новые
доставки. После перевода цель `stand` принимает выпуски без рабочей сессии.

Владельца workspace delivery определяет исходная операция, связанная с этим
pipeline: её workspace, actor и subject должны совпадать с активной сессией;
version исходной команды должна принадлежать этой сессии, а source оставаться
подготовленным. Штатный `job retry` сохраняет успешные producer jobs и проверяет
владельца нового attempt. Завершённая квитанция исходной workspace-команды не
переписывается: текущий результат retry читается у pipeline и attempt.
Для обычного repository pipeline используется его human owner. Эти источники
не подменяют друг друга; отключённый владелец workspace не получает новый
deployment offer. Машинная сессия Developer не разрешает delivery. Runtime,
занятость и environment binding проверяются отдельно.

Для автоматического Developer Fleet открывает сессию и готовит checkout до
запуска агента. Forge проверяет текущее назначение Tracker и явный допуск
`workspace_preparation_authorized`, не срок исторической резервации.
В этот период доступны открытие и закрытие собственной сессии, чтение
назначенного Git-репозитория и подготовка точного SHA из Tracker. Проверки
кандидата и push требуют `InProgress`. Операции подготовки проверяют резервацию
повторно при исполнении. Forge возвращает исходную квитанцию по operation UUID.
При вызове `prepare` агент передаёт корень workspace, предоставленный Fleet.
Путь внутри Git checkout отклоняется до локальной подготовки исходников.

## CLI и интерфейс

```sh
export CICD_API_URL=https://pdlc.acm-ai.ru/forge
cicd-cli dev-workspace list --project "$ConfigurationId"
cicd-cli dev-workspace profiles --project "$ConfigurationId"
cicd-cli dev-workspace show --workspace "$WorkspaceId"
cicd-cli dev-workspace begin-session --workspace "$WorkspaceId" --operation-id "$BeginId" --expected-version "$Version" --task R1-7
cicd-cli dev-workspace prepare --workspace "$WorkspaceId" --operation-id "$PrepareId" --expected-version "$Version" --root "$LocalRoot" --source "r1=$Commit"
cicd-cli dev-workspace check --workspace "$WorkspaceId" --operation-id "$CheckId" --expected-version "$Version" --recipe backend-unit --commit "$Commit"
cicd-cli dev-workspace wait --workspace "$WorkspaceId" --operation-id "$CheckId"
cicd-cli dev-workspace end-session --workspace "$WorkspaceId" --operation-id "$EndId" --expected-version "$Version" --session "$SessionId"
cicd-cli dev-workspace stop --workspace "$WorkspaceId" --operation-id "$StopId" --expected-version "$Version"
```

Для новой команды читать текущую version. Переменные обозначают действительные
UUID/commit. Token передаётся существующим credential mechanism без значения
в argv. Base source helper устанавливается на машине разработчика; проверяет
Git/tools, клонирует новые checkout и сохраняет существующие dirty/index/untracked
файлы. Сервер не получает доступ к локальным каталогам.

Раздел проекта «Рабочие окружения» показывает тот же outcome и причины запрета
действий. Результаты и очищенные логи открываются через обычный pipeline.
Browser acceptance выполняется отдельно через Codex IAB на установленном приложении.
Карточка возвращает `busy`: активную сессию, неподтверждённую операцию либо
проверку/сборку/delivery, удерживающую окружение. Поле вычисляется сервером из
существующих записей; UI не выводит свободное состояние из отсутствия сессии.

## Operator и восстановление

Opt-in settings: `CICD_DEVELOPMENT_WORKSPACE_CATALOG_ROOT`,
`CICD_DEVELOPMENT_WORKSPACE_OPERATOR`, `CICD_DEVELOPMENT_WORKSPACE_CREATOR`.
Каталог/programs — абсолютные private Linux paths. Профиль/public metadata
фиксируются hash, execution inputs замораживаются до приёма команды. Пользователь
и приложение проекта не могут менять каталог. Creator имеет отдельные права;
обычный lifecycle использует namespace-scoped RBAC без create/cluster-admin.

Явный профиль `services: []` не объявляет приложение или Helm release. При
регистрации такого source-only workspace Forge сохраняет проверки target UUID
и замороженного runtime hash; Base сверяет реальный namespace UID и пустой
inventory. Application environment namespace/release в этом случае не нужны.
Для профиля с app services прежние environment fences сохраняются. Пустой
runtime остаётся stopped; session/prepare и runtime-independent checks не
запускают приложение.

Register наблюдает существующие ресурсы, не переустанавливает их. Перед первым
изменением существующих данных нужны свежий backup и проверенный restore.
Create использует независимые namespace/address/storage/credentials и пустые
dev-данные. Копирование данных действующего instance не выполняется.

Forge сохраняет custody до spawn. Base operator и дочерние процессы наследуют
lock/effect descriptors. Смерть worker и потеря ответа не доказывают завершение.
Reconcile читает original receipt; pipeline повторно не отправляется. SQL-only
эффект и terminal receipt коммитятся атомарно, recovery не повторяет его под
другим владельцем. Kubernetes reconciliation проверяет identity и CAS-fences
старые scale requests. В Compose без native receipt исход остаётся unknown до
подтверждённого operator recovery: snapshot не доказывает окончания daemon request.

Частичный Stop сохраняет желаемое выключенное состояние, даже при подтверждённом
running/degraded. Delivery блокирует ту же строку instance, проверяет target hash,
runtime/desired=running и владельца сессии. Manifest не включает выключенный
instance. Ручной Helm не должен обходить допуск.

После последней поставки собираются свежие source/image/API/runtime/data/IAB
доказательства. Rollback приложения сохраняет новые записи и историю. Down
migrations и удаление журналов/PVC/Secret для снятия блокировки исключены.

## Локальные интеграционные проверки

Итог текущего source gate и открытые установочные условия опубликованы в
[evidence от 2026-11-03](evidence/developer-workspace-source-20261103.json).
Пропущенные тесты не считаются успешными; source gate не заменяет приёмку R1.

[Disposable fixture](../scripts/workspace_fixture/README.md) проверяет готовые
Forge API/worker, PostgreSQL, CLI и Base operator без ожидания установки общего
стенда. Дополнительный Windows сценарий подключает helper R1 и настоящий CLI,
теряет ответ принятой команды и проверяет original-key readback/replay, один
writer, busy Stop, Stop/Start и сохранность данных/container identity.
Используются готовые образы, синтетические Auth/Tracker и свои volumes;
production credentials и данные не нужны. Проверка Kubernetes/R1 и IAB остаётся
отдельной обязательной приёмкой.


## Check для приложения под управлением Base

`forge-delivery` может проверять отдельно объявленный Base-native target.
Source workspace остаётся `services: []`: приложение, namespace, Deployment и
данные принадлежат существующей установке Base. Check профиля имеет
`kind: delivery`, `executor: forge-delivery`, `requiresRuntime: false` и не
содержит commands. Опубликованный recipe содержит только `version: 1` и
`base_resource_rollout`:

```json
{
  "version": 1,
  "base_resource_rollout": {
    "schema": "pdlc/base-resource-target/v1",
    "installationId": "<existing-installation>",
    "slot": "pdlc",
    "component": "task-backend",
    "namespace": "<existing-namespace>",
    "namespaceUid": "<existing-namespace-uuid>",
    "deployment": "<existing-deployment>",
    "deploymentUid": "<existing-deployment-uuid>",
    "container": "<existing-container>",
    "publicationJob": "publish",
    "publicationArtifact": "out/image.json"
  }
}
```

В том же recipe запрещены jobs, Helm и команды. Target фиксируется в исходном
execution plan; его identity не выводится из агентского отчёта. Собственная
Central PAT, текущая Deployment assignment, session, candidate HEAD, проект,
репозиторий и namespace проверяются перед результатом, в том числе при reconcile.
Изменённый профиль публикуется новым immutable catalog target. Для того же
environment он принимается существующей workspace через `adopt_profile`,
а не второй регистрацией; повторный Register не обновляет выбранные hashes.

После настоящего Base `ResourceRollout` с COMPLETE и нормальным readback оператор
экспортирует `delivery_receipt(component)` и публикует исходный envelope
`{receipt, sha256}` в `targets/<target>/deliveries/<source_pipeline_id>.json`.
Формат receipt — `pdlc/resource-rollout-delivery/v1`: hashes operation plan,
исходной/выбранной конфигураций, source catalog и normal receipt; installation,
slot, namespace/Deployment UID, container, точные immutable images и observed
readiness. Это доверенная публикация владельца каталога, не загрузка агента.

Forge проверяет точный успешный `source_pipeline_id` в том же проекте и SHA,
immutable pipeline/config hashes, typed publication job, исходные artifact bytes
и descriptor hash. Base image обязан совпасть с этим publication artifact.
Check не создаёт pipeline/job/deployment: его `pipeline_id` остаётся null.
Исходный operation result хранит `baseOperationObservation` с форматом
`forge/base-operation-observation/v1`, `workspaceOperationId`, `workspaceId`,
`sourcePipelineId`, `sourceCommit`, bounded `target` и исходным `deliveryReceipt`.
GET/replay возвращает именно сохранённый результат. Нет подмены Helm receipt,
сведений latest deployment, agent-held Base credential или пустого успеха.

При отсутствии owner receipt операция получает `unknown`,
`workspace_base_delivery_receipt_missing`, `requiresReconciliation: true`.
После настоящего owner apply/publish оператор вызывает существующий reconcile
той же операции; исходная machine authority проверяется повторно. Новую
модельную/CI-команду для снятия этой блокировки не отправляют.

Publication Check по-прежнему использует ровно один typed `image_publish` job:

```json
{"version":1,"jobs":{"publish":{"image_publish":{
  "context":".","dockerfile":"Dockerfile.forge",
  "repository":"<owner-approved-registry>/<project>/<application>",
  "descriptor_artifact":"out/build.json","published_artifact":"out/image.json"
}}}}
```

Forge экспортирует точное Git tree основного репозитория. Umbrella Dockerfile,
ожидающий соседний services-base, без self-contained locked-SDK context непригоден.
Для документального изменения допустим настоящий build от точного уже
квалифицированного runtime digest с COPY изменяемого payload; это явно reuse
бинарника, не утверждение о новой компиляции Rust. При изменении runtime source
или зависимостей требуется настоящий соответствующий build и квалификация.
Host/builder/registry grants и обычные проверки provenance сохраняются.


## Принятие новой редакции профиля

Один environment принадлежит одной workspace. Его действующий human Maintainer
принимает owner-published catalog target через существующий operations API:

```json
{
  "operation_id": "<original-operation-uuid>",
  "expected_version": 12,
  "command": {"action":"adopt_profile","catalog_target_id":"<published-target-uuid>"}
}
```

`POST /api/v1/development-workspaces/{workspace_id}/operations` проверяет права
и текущую version. Агентам действие запрещено. Активная сессия, queued/running/
unknown операция или незакрытая pipeline custody блокируют принятие. Новый
профиль обязан сохранить project/repository/environment/Tracker и физические
sections repositories/services/storage/secrets. Старый и новый native runtime
descriptor должны совпадать с точным runtimeHash исходной подтверждённой native
операции из текущей observation; порядок дат и latest-поиск не используются.

Workspace UUID, environment, physical target ID, данные, Stop intent, источники
и наблюдения сохраняются. В одной транзакции меняются текущие profile/catalog
hashes и создаётся terminal operation; local preparation сбрасывается, поэтому
новая работа требует обычного Prepare. Исторические request/execution_plan/result
не переписываются. GET и повтор исходного ключа возвращают прежний результат.
В новой операции сохраняются обе owner-published редакции профиля.

При новом catalog target для прежнего physical target оператор публикует тот же
проверенный descriptor в `instances/<stable-target-id>/runtime.json`, с прежними
`id`, adapter, namespace/namespaceUid либо Compose identity, services/storage/
secrets/inventory. Новое приложение, environment, namespace или PVC не создаются.
Вторая регистрация на занятый environment возвращает
`409 workspace_environment_already_owned`. Отдельная CLI-команда и новая route
для adoption не нужны; действие доступно оператору через существующий JSON API.
