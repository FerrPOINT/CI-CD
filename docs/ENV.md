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
| `CICD_SECRETS_KEY` | — | Base64 32-byte ключ AES-256-GCM (обязателен для secrets) |
| `CICD_ARTIFACTS_DIR` | `/var/lib/forge/artifacts` | Локальное хранилище артефактов |
| `CICD_RUNNER_MODE` | `host` в compose | Режим embedded runner: `host` для локального evidence/dev; `docker` только если Docker executor/socket подключены явно |

## Сборочные переменные compose (в .env)

| Переменная | Default | Назначение |
|---|---|---|
| `CICD_DATABASE_USER` | `cicd` | Пользователь БД |
| `CICD_DATABASE_PASSWORD` | — (обязателен) | Пароль БД |
| `CICD_DATABASE_NAME` | `cicd` | Имя БД |
| `CICD_DATABASE_PORT` | `22543` | Публикация PostgreSQL на хост |
| `CICD_API_PORT` | `22801` | Публикация API на хост |
| `CICD_WEB_PORT` | `22802` | Публикация Dashboard на хост |

## Backend-only (используются вне compose)

| Переменная | Default | Назначение |
|---|---|---|
| `CICD_RUNNER_MODE` | `docker` вне compose | Режим embedded runner binary: `docker` \| `host` |
| `CICD_RUNNER_KEEP_WORKSPACE` | `false` | Не удалять workspace после job |
| `CICD_API_URL` | `http://127.0.0.1:22801` | Base URL для cicd-cli |

## Генерация ключа

```bash
openssl rand -base64 32   # CICD_SECRETS_KEY
openssl rand -hex 16      # CICD_GIT_TOKEN / CICD_GIT_INTERNAL_TOKEN
```

> Удалённое legacy-значение `forge-internal-dev-token` отклоняется при старте backend. Для shared deployment задайте уникальный `CICD_GIT_INTERNAL_TOKEN`; пустое значение означает trusted-local режим без проверки internal hook token.

> Ротация `CICD_SECRETS_KEY` без перешифровки секретов сделает их нечитаемыми — см. `docs/STORAGE_ARCHITECTURE.md` (key rotation).
