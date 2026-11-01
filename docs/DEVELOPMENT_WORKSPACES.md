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
