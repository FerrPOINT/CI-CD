# Forge CI/CD

Self-hosted control plane для Git-репозиториев и CI/CD: Rust (Axum + SQLx + PostgreSQL) + React (Vite + Tailwind). Env-префикс `CICD_`.

Forge CI/CD хранит bare Git-репозитории, отдаёт Smart HTTP для `clone` / `fetch` / `push`, автоматически создаёт пайплайны на `post-receive` и показывает их ход в Dashboard.

## Скриншоты

| Экран | Скриншот |
|---|---|
| Login | ![Login](docs/screenshots/01-login.png) |
| Дашборд | ![Dashboard](docs/screenshots/02-dashboard.png) |
| Проекты | ![Projects](docs/screenshots/03-projects.png) |
| Git-репозитории | ![Repositories](docs/screenshots/04-repositories.png) |
| Пайплайны | ![Pipelines](docs/screenshots/05-pipelines.png) |
| Детали пайплайна | ![Pipeline detail](docs/screenshots/06-pipeline-detail.png) |
| Настройки | ![Settings](docs/screenshots/07-settings.png) |
| Администрирование | ![Admin](docs/screenshots/08-admin.png) |

## Порты по умолчанию

| Сервис | Внешний порт | Описание |
|---|---:|---|
| Dashboard | `22802` | Nginx со статическим React SPA и proxy `/api` / `/git` |
| API + Git Smart HTTP | `22801` | REST API, Git clone/fetch/push, internal hooks |
| PostgreSQL | `22543` | Основная БД |

## Функциональность

### Git-хостинг

- Создание и удаление bare Git-репозиториев через Dashboard и REST API
- Git Smart HTTP: `git clone`, `git fetch`, `git push`
- Хранилище репозиториев в Docker volume `cicd_git_repos`
- Опциональная защита Git-трафика через `CICD_GIT_TOKEN` (Basic auth или `x-git-token`)
- `post-receive` hook создаётся автоматически и запускает пайплайн на push

```bash
# Создать репозиторий
curl -X POST http://127.0.0.1:22801/api/v1/repositories \
  -H 'content-type: application/json' \
  -d '{"name":"my-service"}'

# Клонировать и пушить
git clone http://127.0.0.1:22802/git/my-service.git
```

### Проекты и пайплайны

- Проекты: создание, редактирование, удаление с каскадным удалением пайплайнов
- Привязка проекта к URL Git-репозитория и default branch
- Ручной запуск пайплайна по любому `git_ref`
- Автозапуск после Git push, если `repository_url` проекта указывает на локальный репозиторий
- Шаблон пайплайна: `build` -> `test` -> `deploy`
- Статусы: `queued` -> `running` -> `success` / `failed` / `canceled`
- Агрегация статусов job -> stage -> pipeline

### Jobs и логи

- Управление статусами джоб из Dashboard и REST API
- Append-only логи с последовательной нумерацией
- Pipeline Detail: стадии, джобы, команды, Docker images и live-like log panel

### Dashboard и администрирование

- Dashboard с проектами и показателями CI/CD
- Страницы: Projects, Repositories, Pipelines, Pipeline Detail, Settings, Admin
- Три темы: `dark`, `gray`, `light`
- Локализация `ru` / `en`, язык по умолчанию — русский
- REST API с JSON error envelope и health-check

## Быстрый старт

```bash
# 1. Скопировать конфигурацию
cp .env.example .env

# 2. Поднять весь стек
# Docker создаст PostgreSQL, backend и frontend
docker compose up --build -d

# 3. Проверить сервисы
curl http://127.0.0.1:22801/api/v1/health
# {"service":"cicd","status":"ok"}

# 4. Открыть Dashboard
# http://127.0.0.1:22802
```

## Конфигурация

| Переменная | Default | Назначение |
|---|---|---|
| `CICD_API_PORT` | `22801` | Внешний порт API / Smart HTTP |
| `CICD_WEB_PORT` | `22802` | Внешний порт Dashboard |
| `CICD_DATABASE_PORT` | `22543` | Внешний порт PostgreSQL |
| `CICD_DATABASE_USER` | `cicd` | Пользователь PostgreSQL |
| `CICD_DATABASE_PASSWORD` | `cicd_local_only` | Пароль PostgreSQL только для local dev |
| `CICD_DATABASE_NAME` | `cicd` | Имя БД |
| `CICD_GIT_ROOT` | `/var/lib/forge/git` | Путь к bare-репозиториям внутри backend container |
| `CICD_GIT_TOKEN` | пусто | Токен Git Smart HTTP; пустой = local dev без auth |
| `CICD_GIT_INTERNAL_TOKEN` | dev token | Токен вызова post-receive -> pipeline hook |

Не используйте дефолтные секреты вне локальной разработки.

## CLI

```bash
# Собрать CLI
cd backend
cargo build --bin cicd-cli

# Настроить API URL
export CICD_API_URL=http://localhost:22801

# Проекты
./target/debug/cicd-cli project list
./target/debug/cicd-cli project create \
  --name my-service \
  --repository-url http://127.0.0.1:22802/git/my-service.git

# Пайплайны
./target/debug/cicd-cli pipeline list --project <PROJECT_UUID>
./target/debug/cicd-cli pipeline run --project <PROJECT_UUID> --ref main
./target/debug/cicd-cli pipeline show --pipeline <PIPELINE_UUID>

# Джобы и логи
./target/debug/cicd-cli job start --job <JOB_UUID>
./target/debug/cicd-cli job logs --job <JOB_UUID>
```

Подробности: [docs/CLI.md](docs/CLI.md).

## Команды

Основные операции доступны в `justfile`:

| Команда | Описание |
|---|---|
| `just up` | Собрать и запустить Docker Compose стек |
| `just down` | Остановить контейнеры |
| `just health` | Проверить API health-check |
| `just test-backend` | Backend unit и contract tests |
| `just test-frontend` | Frontend Vitest tests |
| `just build-frontend` | Production build frontend |

Либо напрямую:

```bash
# Backend checks через Rust image
cd backend
docker run --rm --entrypoint /bin/bash -v "$PWD:/workspace" -w /workspace rust:1.86-bookworm \
  -lc '/usr/local/cargo/bin/cargo test'

# Frontend
cd ../frontend
pnpm install
pnpm test
pnpm build
```

## Структура

```text
CI-CD/
├── backend/                 # Rust Axum API, domain, SQLx store, CLI, Git Smart HTTP
│   └── src/
│       ├── api.rs           # REST routes, pipeline/job orchestration
│       ├── domain.rs        # Status transition rules
│       ├── git_host.rs      # Bare repositories + Smart HTTP + post-receive hook
│       ├── store.rs         # PostgreSQL schema bootstrap
│       └── bin/cicd-cli.rs  # CLI client
├── frontend/                # React SPA (pages, widgets, typed API hooks)
├── docs/                    # Architecture, API, operations, roadmap and ADRs
│   └── screenshots/         # Dashboard screenshots used in this README
├── docker-compose.yml       # PostgreSQL + backend + frontend
├── justfile                 # Unified local commands
└── AGENTS.md                # Rules for AI coding agents
```

## Документы

- [Техническое задание](docs/TZ.md)
- [Архитектура](docs/ARCHITECTURE.md)
- [Дата-модель](docs/DATA_MODEL.md)
- [Доменная модель](docs/DOMAIN_MODEL.md)
- [API](docs/API.md)
- [Git-хостинг](docs/GIT_HOSTING.md)
- [Git webhooks](docs/WEBHOOKS.md)
- [Деплой](docs/DEPLOYMENT.md)
- [Локальный запуск](docs/LOCAL_SETUP.md)
- [Безопасность](docs/SECURITY.md)
- [Тестирование](docs/TESTING.md)
- [Roadmap](docs/ROADMAP.md)
- [Runbook](docs/OPS_RUNBOOK.md)
- [Правила для агентов](AGENTS.md)

Полный индекс: [`docs/`](docs/).

## Roadmap

- [x] MVP control plane: проекты, пайплайны, стадии, джобы, логи
- [x] Встроенный Git Smart HTTP + post-receive auto-trigger
- [ ] Auth и RBAC
- [ ] Реальные Docker runners
- [ ] Webhooks для GitHub/GitLab/Gitea и SSE events
- [ ] Encrypted project secrets
- [ ] Artifact storage
- [ ] CI/CD reports и audit log

## Лицензия

MIT.
