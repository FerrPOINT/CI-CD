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

Общий воспроизводимый runner принадлежит Base:
`scripts/analyst_tools_qa.py`, фаза `products`. Фактические результаты
фиксируются в отчёте конкретного QA-прогона, а не подразумеваются этим документом.
