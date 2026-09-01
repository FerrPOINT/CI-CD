# Git Hosting — Forge CI/CD

## 1. Назначение

Forge CI/CD включает минимальный self-hosted Git-сервер. Он хранит bare-репозитории, реализует Git Smart HTTP, проверяет read/write доступ к private/write операциям и создаёт CI/CD-пайплайн после каждого push.

Это не замена GitLab или Gitea: в текущем объёме есть минимальные pull requests/compare, но нет issues, web editor, LFS API, отдельных Git-пользователей или прав на уровне репозитория. Цель MVP — замкнуть локальный цикл `git push -> pipeline` без внешнего Git-провайдера.

## 2. Реализованный поток

```text
Developer
  |
  | git clone / git fetch / git push
  v
Dashboard :22802 (nginx)
  |
  | /git/* proxy
  v
Backend :22801
  |
  | git upload-pack / git receive-pack --stateless-rpc
  v
bare repository volume: cicd_git_repos
  |
  | post-receive hook
  v
POST /api/v1/internal/git-push
  |
  v
Project lookup -> queued pipeline (build -> test -> deploy)
```

## 3. API управления репозиториями

| Method | Path | Назначение |
|---|---|---|
| `GET` | `/api/v1/repositories` | Список bare-репозиториев |
| `POST` | `/api/v1/repositories` | Создать bare-репозиторий |
| `DELETE` | `/api/v1/repositories/{name}` | Удалить bare-репозиторий |

### Создание

```bash
curl -fsS -X POST http://127.0.0.1:22801/api/v1/repositories \
  -H 'content-type: application/json' \
  -d '{"name":"platform-core"}'
```

Имя содержит только ASCII letters/digits, `-`, `_`, `.`; запрещены leading `.`, `..` и пробелы. Суффикс `.git` при передаче автоматически убирается.

### Git URL

```bash
git clone http://127.0.0.1:22802/git/platform-core.git
```

В production заменить хост и порт на публичный URL reverse proxy.

## 4. Smart HTTP

Backend использует системный Git для стандартного stateless RPC:

| Git operation | HTTP route | Git service |
|---|---|---|
| discovery | `GET /git/{repo}/info/refs?service=...` | `git upload-pack` / `git receive-pack --advertise-refs` |
| clone/fetch | `POST /git/{repo}/git-upload-pack` | `git upload-pack --stateless-rpc` |
| push | `POST /git/{repo}/git-receive-pack` | `git receive-pack --stateless-rpc` |

Nginx frontend контейнера проксирует `/git/` в backend. Внутренний backend также доступен на `:22801` для диагностики.

## 5. Связь с проектом и auto-trigger

При создании проекта укажите URL локального Git-репозитория:

```bash
curl -fsS -X POST http://127.0.0.1:22801/api/v1/projects \
  -H 'content-type: application/json' \
  -d '{
    "name":"platform-core",
    "repository_url":"http://127.0.0.1:22802/git/platform-core.git",
    "default_branch":"main"
  }'
```

Каждый созданный репозиторий получает executable `hooks/post-receive`. Hook отправляет имя репозитория, pushed ref, `old_rev` и `new_rev` на internal endpoint. Backend ищет первый проект, `repository_url` которого указывает на exact repo tail: `/{name}.git`, `:{name}.git` или ровно `{name}.git`, и создаёт queued pipeline с `git_ref` из `refs/heads/<branch>` или `refs/tags/<tag>`.

Повтор того же hook event с тем же `repository/ref_name/new_rev` не создаёт второй pipeline: backend хранит stable idempotency record в `pipeline_triggers` и возвращает существующий `pipeline_id`. Удаление ref (`new_rev` из нулей) не запускает pipeline.

Если проект не связан с репозиторием, push остаётся успешным, но pipeline не создаётся.

## 6. Конфигурация

| Переменная | Default | Назначение |
|---|---|---|
| `CICD_GIT_ROOT` | `/var/lib/forge/git` | Корень bare-репозиториев в backend container |
| `CICD_GIT_TOKEN` | empty | Legacy shared token Smart HTTP. Empty отключает shared bypass; без `CICD_AUTH_SECRET` это допустимо только для local development |
| `CICD_GIT_INTERNAL_TOKEN` | empty | Токен для `post-receive -> internal endpoint`; empty допустим только для isolated local development |

Данные репозиториев находятся в named volume `cicd_git_repos`, независимом от PostgreSQL volume. Удаление проекта не удаляет репозиторий; удаление репозитория через API удаляет и строку БД, и bare directory.

## 7. Аутентификация

Git Smart HTTP различает read и write:

- `git-upload-pack` для repository с `visibility = public` доступен без credential.
- Private `git-upload-pack` и любой `git-receive-pack` требуют credential.
- Legacy `CICD_GIT_TOKEN` остаётся operator bypass для совместимости.
- При непустом `CICD_AUTH_SECRET` можно передать JWT/PAT пользователя: `viewer+` читает связанный project repository, `developer+` может push, `admin` имеет bypass. PAT дополнительно должен иметь `git:read` для fetch/clone или `git:write` для push и, если задан `project_id`, совпадать со связанным проектом.

Связь repository -> project в текущем MVP выводится из `projects.repository_url`, который указывает на exact repo tail: `/{repo}.git`, `:{repo}.git` или ровно `{repo}.git`. Полноценная tenant-bound mapping table и scoped Git credentials остаются target.

Если используется legacy `CICD_GIT_TOKEN`, Git HTTP принимает один из вариантов:

```bash
# Basic auth: username произвольный, password = token или JWT/PAT
git clone http://any-user:<TOKEN>@host/git/platform-core.git

# Или для raw HTTP запросов
curl -H "x-git-token: <TOKEN>" ...
```

В trusted local development без `CICD_AUTH_SECRET` и без `CICD_GIT_TOKEN` проверка отключена. Нельзя запускать shared/public Git endpoint в таком режиме.

`CICD_GIT_INTERNAL_TOKEN` ведёт себя так же строго по границе окружения: пустое значение отключает проверку только для trusted-local hook traffic, shared deployment обязан задать уникальный токен. Устаревшее значение `forge-internal-dev-token` отклоняется при старте backend.

## 8. Ограничения MVP и следующий этап

- Нет organization/tenant-bound repository model и отдельного scoped Git credential class; current read/write RBAC строится на project membership и PAT scopes через `repository_url`.
- Нет Git LFS HTTP endpoints несмотря на наличие `git-lfs` в образе.
- Нет SSH transport: только Smart HTTP.
- Hook делает best-effort запрос; он не блокирует push при временном сбое CI/CD API.
- Embedded runner клонирует project repository в workspace перед выполнением job; external runner protocol MVP уже отдаёт `workspace.checkoutUrl`, а `forge-runner` использует его для checkout. Scoped repository credential class и production checkout policy ещё не реализованы.

Следующая фаза: per-project repository mapping вместо URL suffix lookup, scoped Git credentials, signed internal events с one-time event ID, Git LFS, SSH transport, отдельный production runner checkout boundary и stricter checkout/commit identity guarantees.

## 9. Проверка

```bash
# Создать репозиторий и убедиться, что discovery работает
curl -fsS -X POST http://127.0.0.1:22801/api/v1/repositories \
  -H 'content-type: application/json' \
  -d '{"name":"smoke-repo"}'
git ls-remote http://127.0.0.1:22802/git/smoke-repo.git

# Удалить после проверки
curl -fsS -X DELETE http://127.0.0.1:22801/api/v1/repositories/smoke-repo
```

## References

- [API](API.md)
- [Workflow](FUNCTIONAL_ARCHITECTURE.md)
- [Webhooks](contracts/EVENT_CONTRACT.md)
- [Security](SECURITY.md)
- [Storage](contracts/DATA_LIFECYCLE.md)
