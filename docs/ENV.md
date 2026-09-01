# Переменные окружения (префикс CICD_)

> **Source of truth:** код приложения (`backend/src/*`) и `docker-compose.yml`. Этот файл — справочник для локального запуска и деплоя.

## Основные (задаются в docker-compose)

| Переменная | Default (compose) | Назначение |
|---|---|---|
| `CICD_DATABASE_URL` | нет вне compose | Полный URL PostgreSQL для прямого backend/test запуска |
| `CICD_BIND` | `0.0.0.0:22801` | Адрес API + Git Smart HTTP |
| `CICD_GIT_ROOT` | `./.forge/git` вне compose | Корень bare-репозиториев при локальном запуске |
| `CICD_GIT_TOKEN` | — | Legacy shared token для Git Smart HTTP; пусто отключает только этот token, а при непустом `CICD_AUTH_SECRET` private/read-write Git routes всё равно требуют JWT/PAT + project membership |
| `CICD_GIT_INTERNAL_TOKEN` | — | `X-Internal-Token` для post-receive hook; пусто допустимо только для изолированного local development |
| `CICD_CORS_ALLOWED_ORIGINS` | — | Comma-separated allowlist browser origins для API/Git Dashboard CORS; пусто сохраняет permissive trusted-local режим, explicit `*` запрещён |
| `CICD_SECRETS_KEY` | — | Base64 32-byte ключ AES-256-GCM (обязателен для secrets) |
| `CICD_ARTIFACTS_DIR` | `/var/lib/forge/artifacts` | Локальное хранилище артефактов |
| `CICD_EMBEDDED_RUNNER_ENABLED` | `true` | Включает embedded runner внутри backend; поставьте `false`, когда работу должен забирать внешний `forge-runner` |
| `CICD_RUNNER_MODE` | `host` в compose | Режим embedded runner: `host` для локального evidence/dev; `docker` только если Docker executor/socket подключены явно |
| `CICD_RUNNER_KEEP_WORKSPACE` | `false` | Не удалять workspace после job для embedded runner и `forge-runner` |
| `CICD_RUNNER_REGISTRATION_TOKEN` | — | Bootstrap token для `POST /api/v1/runner/register`; пусто отключает регистрацию внешних runner-ов |
| `CICD_RUNNER_CREDENTIAL` | — | Bearer credential уже зарегистрированного `forge-runner`; если пусто, runner регистрируется через `CICD_RUNNER_REGISTRATION_TOKEN` |
| `CICD_RUNNER_NAME` | `forge-runner` | Имя внешнего runner process |
| `CICD_RUNNER_TAGS` | `linux,host` | Теги внешнего runner process, через запятую |
| `CICD_RUNNER_TOTAL_SLOTS` | `1` | Количество параллельных слотов, которое внешний runner сообщает в heartbeat/poll |
| `CICD_RUNNER_POLL_INTERVAL_SECONDS` | `5` | Пауза между пустыми `work:poll` запросами внешнего `forge-runner` |
| `CICD_RUNNER_NO_CHECKOUT` | `false` | Отключает Git checkout и создаёт пустой workspace для команд |
| `CICD_RUNNER_WORK_DIR` | `/var/lib/forge/runner-work` в compose runner profile | Корень checkout workspace внешнего `forge-runner` |

## Сборочные переменные compose (в .env)

| Переменная | Default | Назначение |
|---|---|---|
| `CICD_DATABASE_USER` | `cicd` | Пользователь БД |
| `CICD_DATABASE_PASSWORD` | — (обязателен) | Пароль БД |
| `CICD_DATABASE_NAME` | `cicd` | Имя БД |
| `CICD_DATABASE_PORT` | `22543` | Публикация PostgreSQL на хост |
| `CICD_API_PORT` | `22801` | Публикация API на хост |
| `CICD_WEB_PORT` | `22802` | Публикация Dashboard на хост |

## Backend/runner-only (используются вне compose)

| Переменная | Default | Назначение |
|---|---|---|
| `CICD_RUNNER_MODE` | `docker` вне compose | Режим embedded runner binary: `docker` \| `host` |
| `CICD_EMBEDDED_RUNNER_ENABLED` | `true` | Отключение embedded execution при запуске внешнего runner-а |
| `CICD_RUNNER_REGISTRATION_TOKEN` | пусто | Bootstrap token внешнего runner protocol MVP |
| `CICD_RUNNER_CREDENTIAL` | пусто | Credential внешнего `forge-runner`, полученный при регистрации |
| `CICD_RUNNER_NAME` | `forge-runner` | Имя внешнего runner-а |
| `CICD_RUNNER_TAGS` | `linux,host` | Теги внешнего runner-а |
| `CICD_RUNNER_TOTAL_SLOTS` | `1` | Capacity внешнего runner-а |
| `CICD_RUNNER_POLL_INTERVAL_SECONDS` | `5` | Пауза между пустыми poll-запросами |
| `CICD_RUNNER_NO_CHECKOUT` | `false` | Запускать команды в пустом workspace без Git checkout |
| `CICD_RUNNER_WORK_DIR` | temp dir `forge-runner` | Workspace root внешнего runner-а |
| `CICD_RUNNER_KEEP_WORKSPACE` | `false` | Не удалять workspace после job |
| `CICD_API_URL` | `http://127.0.0.1:22801` | Base URL для cicd-cli |

## Генерация ключа

```bash
openssl rand -base64 32   # CICD_SECRETS_KEY
openssl rand -hex 16      # CICD_GIT_TOKEN / CICD_GIT_INTERNAL_TOKEN
openssl rand -hex 32      # CICD_RUNNER_REGISTRATION_TOKEN
```

> Удалённое legacy-значение `forge-internal-dev-token` отклоняется при старте backend. Для shared deployment задайте уникальный `CICD_GIT_INTERNAL_TOKEN`; пустое значение означает trusted-local режим без проверки internal hook token.

> Ротация `CICD_SECRETS_KEY` без перешифровки секретов сделает их нечитаемыми — см. `docs/STORAGE_ARCHITECTURE.md` (key rotation).
