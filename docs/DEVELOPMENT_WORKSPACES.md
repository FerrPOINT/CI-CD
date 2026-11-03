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
InProgress, project/task, agent, machine subject, assignment/execution UUID,
версия назначения и подтверждённые исходники. Tracker также перепроверяет
актуальный доступ назначенного исполнителя к проекту.
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

Доступны чтение проекта, своя сессия, prepare и опубликованные Forge-проверки
без runtime-доставки. Create/register, Start/Stop, Build, recovery, reconcile,
произвольные pipelines и выпуск этим доступом не разрешаются. Опубликованные
рецепты агента не содержат `image_publish` / `helm_deploy`. Результаты jobs,
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

[Disposable fixture](../scripts/workspace_fixture/README.md) проверяет готовые
Forge API/worker, PostgreSQL, CLI и Base operator без ожидания установки общего
стенда. Дополнительный Windows сценарий подключает helper R1 и настоящий CLI,
теряет ответ принятой команды и проверяет original-key readback/replay, один
writer, busy Stop, Stop/Start и сохранность данных/container identity.
Используются готовые образы, синтетические Auth/Tracker и свои volumes;
production credentials и данные не нужны. Проверка Kubernetes/R1 и IAB остаётся
отдельной обязательной приёмкой.
