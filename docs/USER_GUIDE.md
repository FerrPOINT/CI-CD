# Руководство пользователя Forge CI/CD

Forge CI/CD - self-hosted control plane для Git-репозиториев и CI/CD. Руководство предназначено для разработчиков, владельцев проектов и администраторов инстанса. Технические контракты API принадлежат `docs/API.md`; проверенное текущее состояние - `docs/CURRENT_STATE.md`.

## 1. Обзор продукта и статусы

### Как читать статусы

- **Current verified** - возможность работает в текущей версии и подтверждена кодом.
- **Configuration only** - форму и хранение конфигурации можно использовать, но её исполнение или доставка ещё не реализованы.
- **Target approved** - возможность принята в ADR/контракте, но пока не реализована.

### Что доступно сейчас

| Возможность | Статус | Практическая граница |
|---|---|---|
| Проекты, pipelines, jobs, логи | **Current verified** | Создание, просмотр, отмена и повтор; embedded runner выполняет jobs в Docker или host shell. |
| Git Smart HTTP и auto-trigger | **Current verified** | Локальный bare-репозиторий, `clone`/`fetch`/`push`; push создаёт pipeline у связанного проекта. |
| Артефакты | **Current verified** | Локальное хранение, upload/download, до 50 MiB на файл. |
| Секреты проекта | **Current verified** | AES-256-GCM at rest; нет инъекции в job и маскирования логов. |
| Окружения и записи деплоев | **Current verified** | Метаданные окружения и история деплоев; выполнение деплоя определяется job. |
| Отчёты и аудит | **Current verified** | Сводка по проекту и последние 200 событий аудита. |
| Пользователи и API-токены | **Current verified** | Хранение ролей и токенов; проверка токенов/RBAC ещё не включена. |
| Расписания, webhooks, уведомления | **Configuration only** | Конфигурация сохраняется, но cron, доставка webhooks и отправка уведомлений не выполняются. |
| Вход, сессии, RBAC | **Target approved** | UI входа - заглушка; API и Dashboard сейчас не защищены middleware. |

> **Безопасность:** до внедрения auth/RBAC изолируйте API и Dashboard сетью или reverse proxy. Не публикуйте Git endpoint с пустым `CICD_GIT_TOKEN`; обязательно замените dev-значение `CICD_GIT_INTERNAL_TOKEN` в общем окружении.

![Дашборд](screenshots/02-dashboard.png)

## 2. Вход

**Статус процедуры: Target approved.**

1. Откройте `/login`.
2. В текущей версии поля email и password не отправляют auth-запрос: кнопка **Sign in** ведёт на главную страницу.
3. Не считайте эту страницу контролем доступа. Ограничьте сетевой доступ к инстансу до реализации сессий и RBAC.

![Страница входа](screenshots/01-login.png)

## 3. Создание проекта

**Статус процедуры: Current verified.**

Проект связывает репозиторий с pipelines, секретами, окружениями, отчётами и другими project-scoped ресурсами.

1. Откройте **Projects** (`/projects`) и выберите **Create project**.
2. Укажите уникальное имя, `repository_url` и default branch. Для локального Git-хостинга используйте URL вида `http://<host>:22802/git/<repo>.git`.
3. Создайте проект и сохраните его UUID: он нужен для CLI и прямых запросов API.
4. Откройте страницу проекта и перейдите к pipelines или соответствующему ресурсу.

Если bare-репозитория ещё нет, сначала создайте его в **Repositories** (`/repositories`) с именем `<repo>`. Имя допускает ASCII-буквы/цифры, `-`, `_` и `.`; суффикс `.git` можно не вводить.

```bash
# Создать bare-репозиторий.
curl -fsS -X POST http://127.0.0.1:22801/api/v1/repositories \
  -H 'content-type: application/json' \
  -d '{"name":"my-service"}'

# Связать его с проектом.
curl -fsS -X POST http://127.0.0.1:22801/api/v1/projects \
  -H 'content-type: application/json' \
  -d '{
    "name":"my-service",
    "repository_url":"http://127.0.0.1:22802/git/my-service.git",
    "default_branch":"main"
  }'
```

Удаление проекта каскадно удаляет его pipeline-данные и project-scoped записи, но не удаляет bare-репозиторий. Репозиторий удаляется отдельно через **Repositories** или `DELETE /api/v1/repositories/{name}`.

![Проекты](screenshots/03-projects.png)

## 4. Push в репозиторий и авто-триггер pipeline

**Статус процедуры: Current verified.**

Auto-trigger работает для репозитория, созданного Forge, если `repository_url` проекта оканчивается на имя этого локального репозитория. `post-receive` hook отправляет pushed ref внутреннему API и создаёт queued pipeline. Ошибка hook-доставки не откатывает Git push.

1. Создайте репозиторий и связанный проект по предыдущему разделу.
2. Клонируйте URL, добавьте commit и отправьте его в нужную ветку.
3. Откройте **Pipelines** проекта. Для `refs/heads/main` pipeline получит `git_ref` `main`; для тега будет использовано имя тега.
4. Если pipeline не появился, проверьте точное соответствие локального URL проекта и имени bare-репозитория, затем health API и `CICD_GIT_INTERNAL_TOKEN`.

```bash
# Для local development пустой CICD_GIT_TOKEN разрешает доступ без Basic auth.
git clone http://127.0.0.1:22802/git/my-service.git
cd my-service
git switch -c main
echo '# My service' > README.md
git add README.md
git commit -m 'Initial commit'
git push -u origin main
```

Если задан `CICD_GIT_TOKEN`, передайте его как пароль Basic auth (username произвольный):

```bash
git clone http://any-user:<TOKEN>@<host>/git/my-service.git
```

Pipeline читает текущий legacy-файл `.forge-ci.yml`, если он доступен; иначе используется шаблон build/test/deploy. Поддерживаемая сейчас форма - `stages` с jobs и одиночным `command`. DSL v1 с `version`, DAG, `on`, secrets и declared artifacts - **Target approved**, см. `docs/contracts/PIPELINE_DSL.md`.

```yaml
stages:
  - name: build
    jobs:
      - name: compile
        image: rust:1.86
        command: cargo build --release
  - name: test
    jobs:
      - name: unit-tests
        image: rust:1.86
        command: cargo test
```

## 5. Просмотр pipeline, логов и артефактов

### Pipeline и jobs

**Статус процедуры: Current verified.**

1. Откройте **Projects**, выберите проект и перейдите к **Pipelines** (`/projects/{projectId}/pipelines`).
2. Выберите запуск, чтобы открыть карточки stages и jobs (`/pipelines/{pipelineId}`).
3. Следите за статусом: `queued`, `running`, `success`, `failed`, `canceled`.
4. При необходимости отмените pipeline; повторите весь pipeline или отдельную terminal job доступной кнопкой **Retry**.

Embedded runner каждые две секунды выбирает доступные queued jobs, выполняет их последовательно по stages, пишет stdout/stderr в append-only log и устанавливает результат по exit code. Статусы stage и pipeline агрегируются автоматически. Для диагностики можно вручную менять статус job через UI, API или CLI, но это не заменяет фактическое выполнение.

![Список pipelines](screenshots/05-pipelines.png)

![Детали pipeline](screenshots/06-pipeline-detail.png)

### Логи

**Статус процедуры: Current verified.**

1. В деталях pipeline выберите job и откройте **Logs**.
2. Лог отображается в порядке `sequence`; новая строка добавляется только в конец.
3. Для API используйте `GET /api/v1/jobs/{job_id}/logs`.
4. При анализе ошибки сопоставьте exit code/последнюю строку лога с image и command job.

> Значения секретов пока не инъецируются и не маскируются. Не выводите секреты в команды или логи.

### Артефакты

**Статус процедуры: Current verified.**

1. Откройте **Artifacts** для job (`/jobs/{jobId}/artifacts`).
2. Загрузите файл через UI либо отправьте raw body с заголовком `X-Artifact-Name`.
3. Скачайте сохранённый файл по ссылке в списке.
4. Учитывайте лимит 50 MiB на один файл и локальный характер хранилища `CICD_ARTIFACTS_DIR`.

```bash
# Загрузить файл артефакта.
curl -fsS -X POST "http://127.0.0.1:22801/api/v1/jobs/$JOB_ID/artifacts" \
  -H 'X-Artifact-Name: test-report.txt' \
  --data-binary @test-report.txt

# Посмотреть метаданные.
curl -fsS "http://127.0.0.1:22801/api/v1/jobs/$JOB_ID/artifacts"
```

Retention/TTL, S3 и multipart upload - **Target approved**.

![Артефакты](screenshots/21-artifacts.png)

## 6. Секреты

**Статус процедуры: Current verified.**

Секреты проекта хранятся в PostgreSQL, зашифрованные AES-256-GCM ключом `CICD_SECRETS_KEY`. API и UI никогда не возвращают значение секрета после сохранения.

1. Перед включением секретов сгенерируйте отдельный base64-ключ и передайте его сервису через защищённую конфигурацию: `openssl rand -base64 32`.
2. Откройте **Secrets** проекта (`/projects/{projectId}/secrets`) и добавьте пару `key`/`value` через UI. Не вводите значение в URL, repository URL, pipeline command или `.forge-ci.yml`.
3. Проверьте список: доступны только метаданные, например key и timestamps.
4. Для отзыва удалите секрет по его ID. Замена значения выполняется повторным `POST` с тем же key.

Для автоматизации используйте защищённый API-клиент, который отправляет JSON вида `{"key":"DEPLOY_TOKEN","value":"..."}` без записи значения в shell history или CI log.

**Ограничение:** injection значений в окружение runner, маскирование логов, master-key rotation и выборочные secrets из DSL - **Target approved**. До их реализации секрет существует только как безопасно хранимая запись, а не как credential, доступный job.

![Секреты проекта](screenshots/14-secrets.png)

## 7. Окружения и деплои

**Статус процедуры: Current verified.**

Окружение - metadata-объект проекта с именем, URL и состоянием `available`, `stopped` или `degraded`. Запись deployment связывает окружение с `git_ref`, опционально с pipeline и статусом `pending`, `running`, `success` или `failed`.

1. Откройте **Environments** (`/projects/{projectId}/environments`).
2. Создайте окружения, например `staging` и `production`, с URL, если он известен.
3. После выполнения вашей deploy-job добавьте запись deployment с реальным ref и статусом.
4. Используйте историю deployments как журнал факта доставки, а не как механизм запуска инфраструктуры.

> Forge не выполняет deployment автоматически только из создания environment/deployment. Развёртывание должно быть частью job или внешней процедуры; protected environments, approvals и policy checks - **Target approved**.

![Окружения](screenshots/15-environments.png)

## 8. Расписания, webhooks и уведомления

### Расписания

**Статус процедуры: Configuration only.**

1. Откройте **Schedules** (`/projects/{projectId}/schedules`).
2. Создайте запись с пяти-польным cron-выражением, `git_ref` и флагом enabled, например `0 2 * * *` для `main`.
3. Редактируйте, отключайте или удаляйте запись при изменении политики запуска.
4. Не ожидайте появления pipeline по времени: scheduler ещё не исполняет эти записи.

### Исходящие webhooks

**Статус процедуры: Configuration only.**

1. Откройте **Webhooks** (`/projects/{projectId}/webhooks`).
2. Укажите HTTPS URL получателя, список событий и enabled.
3. Сохраните конфигурацию и документируйте владельца получателя.
4. Не используйте эту запись как подтверждение интеграции: delivery worker, HMAC signature, retries и delivery history отсутствуют.

### Уведомления

**Статус процедуры: Configuration only.**

1. На странице **Webhooks** добавьте каналы уведомлений с полями channel, target и enabled.
2. Проверьте сохранённую конфигурацию через список каналов проекта.
3. Не ожидайте email, Slack или другое сообщение: sender и SSE не реализованы.

Входящие webhooks от GitHub/GitLab/Gitea также являются **Target approved**; текущий Git auto-trigger работает через локальный `post-receive` hook, а не через public inbound webhook.

![Расписания](screenshots/16-schedules.png)

![Webhooks и уведомления](screenshots/17-webhooks.png)

## 9. Отчёты

**Статус процедуры: Current verified.**

1. Откройте **Reports** проекта (`/projects/{projectId}/reports`).
2. Просмотрите total, successful, failed, success rate и average duration.
3. Используйте отчёт для быстрой оценки проекта; исходные данные - завершённые pipelines проекта.
4. Получите те же данные программно: `GET /api/v1/projects/{project_id}/reports/summary`.

Фильтры периода, графики, percentiles, failure trends, DORA-агрегаты и export - **Target approved**. Не интерпретируйте текущую summary как отчёт за выбранный период: такого фильтра ещё нет.

![Отчёты](screenshots/18-reports.png)

## 10. Аудит

**Статус процедуры: Current verified.**

1. Откройте **Audit log** (`/audit-log`).
2. Найдите действие по времени, actor, resource и action.
3. Для интеграции запросите `GET /api/v1/audit-log`; API возвращает не более последних 200 событий.
4. Сохраняйте критичные расследования вне Forge, если нужен срок хранения или экспорт больше встроенного окна.

Аудит append-only для текущих mutation-операций, включая runner, secret, artifact и token. Полный authorisation context, фильтры, pagination и export - **Target approved**. Он не заменяет историю delivery webhooks или execution attempts.

![Журнал аудита](screenshots/19-audit-log.png)

## 11. Пользователи и API-токены

### Пользователи и роли

**Статус процедуры: Current verified.**

1. Откройте **Users** (`/users`).
2. Создайте или измените пользователя с username, ролью `admin`, `maintainer`, `developer` или `viewer` и флагом enabled.
3. Используйте данные как подготовку к будущей policy-модели.
4. Не полагайтесь на роль для ограничения доступа в текущей версии: middleware auth/RBAC отсутствует.

Пароли сейчас не хранятся, а project membership и scope-policy относятся к **Target approved**.

### API-токены

**Статус процедуры: Current verified.**

1. На странице **Users** создайте токен и укажите понятное имя; при необходимости свяжите его с пользователем.
2. Скопируйте значение немедленно в password manager или secret manager: API показывает полное значение только один раз.
3. В дальнейшем сверяйте только hint в списке токенов.
4. Отзовите токен через UI или `DELETE /api/v1/api-tokens/{token_id}`, если владелец/интеграция больше не нуждается в нём.

Токены хранятся как SHA-256 hash, но **не проверяются на API-запросах**. Передавать их как действующий credential можно только после внедрения auth middleware - **Target approved**.

![Пользователи и API-токены](screenshots/20-users.png)

## 12. CLI-команды

**Статус процедуры: Current verified.**

CLI `cicd-cli` работает только через HTTP API и сейчас покрывает projects, pipelines, jobs и logs. Соберите его из корня репозитория, если binary ещё отсутствует:

```bash
docker run --rm --entrypoint /bin/bash -v "$PWD/backend:/workspace" -w /workspace \
  -e CARGO_TARGET_DIR=/workspace/target rust:1.86-bookworm \
  -lc '/usr/local/cargo/bin/cargo build -p cicd-cli'

export CICD_API_URL=http://127.0.0.1:22801
export CICD_CLI="$PWD/backend/target/debug/cicd-cli"
```

| Задача | Команда |
|---|---|
| Список проектов | `$CICD_CLI project list` |
| Создать проект | `$CICD_CLI project create --name my-service --repository-url http://127.0.0.1:22802/git/my-service.git --branch main` |
| Список запусков | `$CICD_CLI pipeline list --project <PROJECT_UUID>` |
| Запустить pipeline | `$CICD_CLI pipeline run --project <PROJECT_UUID> --git-ref main` |
| Детали pipeline | `$CICD_CLI pipeline show --id <PIPELINE_UUID>` |
| Запустить job | `$CICD_CLI job start --id <JOB_UUID>` |
| Завершить job успешно | `$CICD_CLI job pass --id <JOB_UUID>` |
| Пометить job ошибочной | `$CICD_CLI job fail --id <JOB_UUID>` |
| Прочитать логи | `$CICD_CLI job logs --id <JOB_UUID>` |
| Добавить строку лога | `$CICD_CLI job log --id <JOB_UUID> --message 'diagnostic line'` |

CLI-команды для runners, secrets, artifacts, environments, schedules, webhooks, users и tokens - **Target approved**. Также пока нет `CICD_API_TOKEN`/`--token`, JSON/table formatting и pagination.

## 13. FAQ по типовым задачам

### Как вручную запустить pipeline для ветки?

**Статус процедуры: Current verified.**

Откройте pipelines проекта, выберите **Run pipeline** и укажите ref; либо выполните:

```bash
curl -fsS -X POST "http://127.0.0.1:22801/api/v1/projects/$PROJECT_ID/pipelines" \
  -H 'content-type: application/json' \
  -d '{"git_ref":"main"}'
```

### Почему после push нет pipeline?

**Статус процедуры: Current verified.**

Проверьте по порядку:

1. Репозиторий создан Forge и URL push имеет вид `/git/<name>.git`.
2. У проекта `repository_url` оканчивается на тот же `<name>.git`.
3. Backend доступен и hook имеет корректный `CICD_GIT_INTERNAL_TOKEN`.
4. Push прошёл успешно; hook best-effort и не прерывает операцию при сбое доставки.

Для внешних Git-провайдеров настройка incoming webhooks пока не является альтернативой: она **Target approved**.

### Как отменить зависший pipeline или повторить неудачный?

**Статус процедуры: Current verified.**

В деталях pipeline используйте **Cancel** для каскадной отмены нетерминальных jobs или **Retry** для нового запуска того же project/ref. Отдельную terminal job можно повторить её кнопкой **Retry**. Убедитесь, что не используете повтор для исправления секретов: injection/маскирование пока отсутствуют.

### Где найти логи и почему они не меняются?

**Статус процедуры: Current verified.**

Откройте job и **Logs** или вызовите `GET /api/v1/jobs/{job_id}/logs`. Логи append-only; у queued job может ещё не быть строк. Проверьте status job, image, command и доступность Docker/host shell для embedded runner.

### Как безопасно передать credential в job?

**Статус процедуры: Target approved.**

Сейчас безопасно доступно только encrypted-at-rest хранение секрета. Не помещайте credential в `.forge-ci.yml`, command, переменные shell, URL репозитория или логи. Используйте внешний механизм исполнения/secret manager до реализации secret injection и log masking.

### Можно ли запланировать ночной запуск или получить Slack-уведомление?

**Статус процедуры: Configuration only.**

Можно сохранить cron/ref и канал уведомления в UI/API. Pipeline по времени и доставка сообщения не произойдут, пока не будут реализованы scheduler и delivery worker.

### Почему роль пользователя или API-токен не запрещают доступ?

**Статус процедуры: Current verified.**

Это ожидаемое ограничение MVP: роли и токены хранятся, но auth/RBAC/token middleware ещё отсутствуют. Закройте сервис сетевыми средствами и не выдавайте внешним пользователям прямой API/Dashboard доступ.

## Связанные документы

- `docs/CURRENT_STATE.md` - проверенная карта возможностей и ограничений.
- `docs/API.md` - точные HTTP endpoints, запросы и ответы.
- `docs/GIT_HOSTING.md` - Smart HTTP, post-receive hook и защита Git-трафика.
- `docs/ENV.md` - переменные окружения и генерация ключей.
- `docs/contracts/PIPELINE_DSL.md` - нормативный target-контракт DSL.
- `docs/AUTHORIZATION.md` - target auth/RBAC policy.
