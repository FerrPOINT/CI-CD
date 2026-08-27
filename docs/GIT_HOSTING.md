# Git Hosting — Forge CI/CD

## 1. Назначение

Forge CI/CD включает минимальный self-hosted Git-сервер. Он хранит bare-репозитории, реализует Git Smart HTTP и создаёт CI/CD-пайплайн после каждого push.

Это не замена GitLab или Gitea: в текущем объёме нет pull requests, issues, web editor, LFS API, пользователей или прав на уровне репозитория. Цель MVP — замкнуть локальный цикл `git push -> pipeline` без внешнего Git-провайдера.

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

Каждый созданный репозиторий получает executable `hooks/post-receive`. Hook отправляет имя репозитория и pushed ref на internal endpoint. Backend ищет первый проект, `repository_url` которого оканчивается на `{name}.git`, и создаёт queued pipeline с `git_ref` из `refs/heads/<branch>` или `refs/tags/<tag>`.

Если проект не связан с репозиторием, push остаётся успешным, но pipeline не создаётся.

## 6. Конфигурация

| Переменная | Default | Назначение |
|---|---|---|
| `CICD_GIT_ROOT` | `/var/lib/forge/git` | Корень bare-репозиториев в backend container |
| `CICD_GIT_TOKEN` | empty | Токен Smart HTTP. Empty допустим только для local development |
| `CICD_GIT_INTERNAL_TOKEN` | dev token | Токен для `post-receive -> internal endpoint` |

Данные репозиториев находятся в named volume `cicd_git_repos`, независимом от PostgreSQL volume. Удаление проекта не удаляет репозиторий; удаление репозитория через API удаляет и строку БД, и bare directory.

## 7. Аутентификация

Если `CICD_GIT_TOKEN` задан, Git HTTP требует один из вариантов:

```bash
# Basic auth: username произвольный, password = token
git clone http://any-user:<TOKEN>@host/git/platform-core.git

# Или для raw HTTP запросов
curl -H "x-git-token: <TOKEN>" ...
```

В local development пустой `CICD_GIT_TOKEN` отключает проверку. Нельзя запускать публичный Git endpoint с пустым токеном.

## 8. Ограничения MVP и следующий этап

- Нет пользователей/организаций и repository-level RBAC.
- Нет Git LFS HTTP endpoints несмотря на наличие `git-lfs` в образе.
- Нет SSH transport: только Smart HTTP.
- Hook делает best-effort запрос; он не блокирует push при временном сбое CI/CD API.
- Runner пока не клонирует репозиторий для фактического выполнения job; pipeline создаётся как control-plane запись.

Следующая фаза: Auth/RBAC, per-project repository mapping вместо URL suffix lookup, signed internal events, Git LFS, SSH transport и Docker runner с checkout commit SHA.

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
