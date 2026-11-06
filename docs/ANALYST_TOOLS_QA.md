# Проверка чтения Forge из Analyst

Проверяется отдельный комплект инструментов в PDLC1, без Fleet и production.
Analyst получает `repository_id`, `base_ref` и `base_commit` из контекста
исходной задачи Tracker. Исследуемая версия — `base_commit`, а не текущее
состояние ветки.

Активный SDK pin — `.namespace-base-revision`:
`8bfb35abdedf02c13ed64c2e270106311a0127e2`. UI и skills pins независимы.
Base предоставляет `ApiClient::request_segments`; Forge передаёт имя
репозитория одним сегментом URL. Это позволяет читать `group/repo`, сохраняя
проверки origin, API prefix и запрет traversal. HTTP API сервера не меняется.

```sh
cicd-cli repository get --id "$REPOSITORY_ID"
# REPOSITORY — public_name из этого ответа, не имя проекта и не URL clone.
cicd-cli repository tree --repo "$REPOSITORY" --git-ref "$COMMIT" --path docs
cicd-cli repository blob --repo "$REPOSITORY" --git-ref "$COMMIT" --path docs/webhooks.md
```

Адрес и отдельный read-only token задаются заранее конфигурацией CLI.
Токен не передаётся аргументом командной строки. Учётная запись ограничена
разрешённым проектом; проверка чужого репозитория должна завершиться отказом
доступа, а не сетевой ошибкой. Не выдавать разрешение на каталог всех
репозиториев ради чтения известного репозитория.

`GET /api/v1/catalog/repositories/{id}` разрешён project-bound service account
с `api:read` только для репозитория, связанного с этим проектом. Middleware
разрешает точный UUID route и передаёт его в существующую проверку project scope.
Коллекция каталога и операции записи остаются закрыты. Human access использует
свой действующий admission. Чужой настоящий UUID проверяется отдельно от
неизвестного UUID; отсутствие записи не является доказательством изоляции.

Обязательные проверки: дерево и файл закреплённого commit, сохранение старого
содержимого после изменения `main`, отказ для чужого репозитория и операций
записи. Исправность CLI отдельно от версии установленного сервера и прав.
Сборка нового CLI не означает обновление Fleet или установленной платформы.

CLI использует `ApiClient::request_segments` из Base для полного имени репозитория.
Собирать его с точной SDK-версией из `.base-revision`; прежний pin без этого
метода несовместим. SDK проверяется вместе с CLI и сервером, отдельно от
namespace/UI/skills pins.

Воспроизводимая подготовка принадлежит Base: `scripts/analyst_qa.py` создаёт
изолированные источники и назначения, а `scripts/analyst_tools_qa.py products`
проверяет CLI. QA не вызывает Workflow, Fleet, typed proposal или Complete.

Приёмка 2026-11-05 подтвердила metadata по UUID из Tracker, дерево и содержимое
закреплённого commit после обновления `main`, отказ настоящему чужому репозиторию,
глобальному каталогу и записи. Команды, runtime/CLI hashes и доказательства —
в [сводке Base](https://github.com/FerrPOINT/services-base/blob/e147727ae53b8752ea09edf70233e5d9f8f07a0a/docs/plans/analyst-tools-standalone-20261104.md).
Эта проверка не означает обновления установленной платформы или готовности Fleet.
