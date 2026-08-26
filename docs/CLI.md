# CLI — Forge CI/CD

Консольный клиент для работы с API контрольной плоскости. Бинарник: `cicd` (исходник: `backend/src/bin/cicd-cli.rs`).

## 1. Установка

```bash
cd backend
cargo install --path . --bin cicd-cli
```

Или запуск напрямую:

```bash
cargo run --bin cicd-cli -- <command>
```

## 2. Глобальные флаги

```
--api-url   Базовый URL API (env: CICD_API_URL, default: http://127.0.0.1:22801)
```

Переменная окружения `CICD_API_URL` используется автоматически, если флаг `--api-url` не передан.

## 3. Команды

CLI имеет три группы команд: `project`, `pipeline`, `job`.

### 3.1 Project

```bash
# Список всех проектов
cicd project list

# Создание проекта
cicd project create \
  --name "My Project" \
  --repository-url https://github.com/example/repo.git \
  --branch main
```

| Команда | API | Описание |
|---------|-----|----------|
| `project list` | `GET /api/v1/projects` | Список проектов |
| `project create` | `POST /api/v1/projects` | Создание проекта |

Параметры `project create`:

| Флаг | Обязательный | Default | Описание |
|------|--------------|---------|----------|
| `--name` | да | — | Название проекта (UNIQUE) |
| `--repository-url` | да | — | URL Git-репозитория |
| `--branch` | нет | `main` | Ветка по умолчанию |

### 3.2 Pipeline

```bash
# Список пайплайнов проекта
cicd pipeline list --project <project-uuid>

# Запуск пайплайна
cicd pipeline run --project <project-uuid> --git-ref main

# Детали пайплайна (со стадиями и задачами)
cicd pipeline show --id <pipeline-uuid>
```

| Команда | API | Описание |
|---------|-----|----------|
| `pipeline list` | `GET /api/v1/projects/{id}/pipelines` | Пайплайны проекта |
| `pipeline run` | `POST /api/v1/projects/{id}/pipelines` | Запуск пайплайна |
| `pipeline show` | `GET /api/v1/pipelines/{id}` | Детали пайплайна |

Параметры:

| Флаг | Обязательный | Default | Описание |
|------|--------------|---------|----------|
| `--project` | да (`list`, `run`) | — | UUID проекта |
| `--git-ref` | нет (`run`) | `main` | Git ref для запуска |
| `--id` | да (`show`) | — | UUID пайплайна |

При запуске пайплайна сервер автоматически создаёт три стадии (`build`, `test`, `deploy`) с одной задачей в каждой.

### 3.3 Job

```bash
# Перевод задачи в статус running
cicd job start --id <job-uuid>

# Перевод задачи в статус success
cicd job pass --id <job-uuid>

# Перевод задачи в статус failed
cicd job fail --id <job-uuid>

# Чтение логов задачи
cicd job logs --id <job-uuid>

# Добавление строки в лог задачи
cicd job log --id <job-uuid> --message "Build started..."
```

| Команда | API | Описание |
|---------|-----|----------|
| `job start` | `POST /api/v1/jobs/{id}/status` | `status: running` |
| `job pass` | `POST /api/v1/jobs/{id}/status` | `status: success` |
| `job fail` | `POST /api/v1/jobs/{id}/status` | `status: failed` |
| `job logs` | `GET /api/v1/jobs/{id}/logs` | Чтение логов |
| `job log` | `POST /api/v1/jobs/{id}/logs` | Append строки лога |

Параметры:

| Флаг | Обязательный | Описание |
|------|--------------|----------|
| `--id` | да | UUID задачи |
| `--message` | да (`log`) | Текст сообщения лога |

## 4. Transition-правила

Команды `job start / pass / fail` проверяют доменные правила переходов на стороне сервера (`JobStatus::transition_to`):

| Из → В | Допустимо |
|--------|-----------|
| `queued → running` | ✅ |
| `queued → canceled` | ✅ |
| `running → success` | ✅ |
| `running → failed` | ✅ |
| `running → canceled` | ✅ |
| `success → *` | ❌ `TerminalStatus` |
| `failed → *` | ❌ `TerminalStatus` |
| `canceled → *` | ❌ `TerminalStatus` |
| `queued → success` (skip) | ❌ `InvalidTransition` |

При недопустимом переходе CLI вернёт ошибку:

```
API returned 400 Bad Request: invalid status transition from Queued to Success
```

## 5. Пример рабочего процесса

```bash
# 1. Создать проект
cicd project create --name "web-app" \
  --repository-url https://github.com/example/web-app.git \
  --branch main

# 2. Запустить пайплайн (возвращает UUID)
cicd pipeline run --project <project-uuid> --git-ref feature/login

# 3. Посмотреть детали (найти job UUID)
cicd pipeline show --id <pipeline-uuid>

# 4. Управлять задачами
cicd job start --id <job-uuid>
cicd job log --id <job-uuid> --message "Cloning repository..."
cicd job log --id <job-uuid> --message "Running tests..."
cicd job pass --id <job-uuid>

# 5. Проверить финальный статус пайплайна
cicd pipeline show --id <pipeline-uuid>
```

## 6. Формат вывода

CLI выводит JSON-ответ от API напрямую в stdout. Для форматирования можно использовать `jq`:

```bash
cicd project list | jq '.[] | {name, repository_url}'
cicd pipeline show --id <uuid> | jq '.stages[] | {name, status, jobs}'
```

## 7. Обработка ошибок

- Неверный UUID → `404 Not Found` от API.
- БД недоступна → `503 Service Unavailable`.
- Нарушение UNIQUE-ограничения → `500 Internal Server Error` (с сообщением об ошибке SQLx).
- Сеть / API недоступен → `anyhow::Error` с описанием.

## Ссылки

- `docs/API.md` — полная спецификация REST API.
- `docs/ARCHITECTURE.md` — архитектура backend.
- `backend/src/bin/cicd-cli.rs` — исходный код CLI.