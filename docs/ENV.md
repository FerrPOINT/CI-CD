# Переменные окружения (префикс CICD_)

> **Source of truth:** код приложения (`backend/src/*`) и `docker-compose.yml`. Этот файл — справочник для локального запуска и деплоя. Backend server читает runtime-настройки через `backend/src/config.rs::RuntimeConfig`; невалидные bool, `CICD_RUNNER_MODE`, CORS allowlist, artifact TTL, queue timeout и `CICD_SECRETS_KEY` падают при старте. `cicd-cli` и отдельный `forge-runner` читают свои process-boundary настройки через `clap`/env.

## Основные (задаются в docker-compose)

| Переменная | Default (compose) | Назначение |
|---|---|---|
| `CICD_DATABASE_URL` | нет вне compose | Полный URL PostgreSQL для прямого backend/test запуска |
| `CICD_MIGRATIONS_DIR` | `/app/migrations` в compose | Каталог committed SQLx migrations; прямой cargo-run по умолчанию использует `backend/migrations` |
| `CICD_BIND` | `0.0.0.0:22801` | Адрес API + Git Smart HTTP |
| `CICD_GIT_ROOT` | `/var/lib/forge/git` в backend/compose; `.env.example` задаёт `./.forge/git` для прямого local run | Корень bare-репозиториев при локальном запуске |
| `CICD_GIT_TOKEN` | — | Legacy shared token для Git Smart HTTP; пусто отключает только этот token, а при непустом `CICD_AUTH_SECRET` private/read-write Git routes всё равно требуют JWT/PAT + project membership |
| `CICD_GIT_INTERNAL_TOKEN` | — | `X-Internal-Token` для post-receive hook; пусто допустимо только для изолированного local development |
| `CICD_AUTH_SECRET` | — | JWT/PAT auth boundary; пусто оставляет trusted-network режим |
| `CICD_AUTH_COOKIE_SECURE` | `false` | `Secure` flag для refresh/CSRF cookies; включайте за TLS/reverse proxy |
| `CICD_CORS_ALLOWED_ORIGINS` | — | Comma-separated allowlist browser origins для API/Git Dashboard CORS; пусто сохраняет permissive trusted-local режим, explicit `*` запрещён |
| `CICD_SECRETS_KEY` | — | Base64 32-byte ключ AES-256-GCM (обязателен для secrets) |
| `CICD_ARTIFACTS_DIR` | `/var/lib/forge/artifacts` | Локальное хранилище артефактов |
| `CICD_ARTIFACT_RETENTION_DAYS` | `30` | TTL новых артефактов в днях (`1..3650`); backend retention worker удаляет expired local files и помечает metadata `purged_at` |
| `CICD_EMBEDDED_RUNNER_ENABLED` | `true` | Включает embedded runner внутри backend; при `false` работу забирает внешний `forge-runner`, а backend оставляет maintenance loop для ack-timeout requeue, lease expiry и stale-runner offline reconciliation |
| `CICD_RUNNER_MODE` | `host` в compose | Режим embedded runner: `host` для локального evidence/dev; `docker` только если Docker executor/socket подключены явно |
| `CICD_RUNNER_KEEP_WORKSPACE` | `false` | Не удалять workspace после job для embedded runner и `forge-runner`; truthy: `true`, `1`, `yes`, `on` |
| `CICD_RUNNER_QUEUE_TIMEOUT_SECONDS` | `86400` | Safety timeout для dispatch-eligible `queued` job без совместимого execution path; `0` отключает. Таймаут срабатывает только если нет embedded path для untagged work и нет online protocol runner-а с подходящими tags/current `shell` capability |
| `CICD_RUNNER_REGISTRATION_TOKEN` | — | Bootstrap token для `POST /api/v1/runner/register`; пусто отключает регистрацию внешних runner-ов |
| `CICD_RUNNER_CREDENTIAL` | — | Bearer credential уже зарегистрированного `forge-runner`; если пусто, runner регистрируется через `CICD_RUNNER_REGISTRATION_TOKEN` |
| `CICD_RUNNER_NAME` | `forge-runner` | Имя внешнего runner process |
| `CICD_RUNNER_TAGS` | `linux,host` | Теги внешнего runner process, через запятую |
| `CICD_RUNNER_TOTAL_SLOTS` | `1` | Количество слотов, которое внешний runner сообщает в heartbeat/poll; current `forge-runner` выполняет один offer за раз и сообщает active lease как busy slot |
| `CICD_RUNNER_POLL_INTERVAL_SECONDS` | `5` | Интервал пустого poll внешнего `forge-runner`: server long-poll wait capped at 30s + fallback sleep для мгновенного `204` |
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
| `CICD_EMBEDDED_RUNNER_ENABLED` | `true` | Отключение embedded execution при запуске внешнего runner-а; runtime maintenance для ack timeout, leases/stale runners продолжает работать |
| `CICD_RUNNER_QUEUE_TIMEOUT_SECONDS` | `86400` | Сколько секунд dispatch-eligible job может ждать без совместимого runner-а до `failed` diagnostic `no compatible runner before queue timeout`; `0` отключает |
| `CICD_RUNNER_REGISTRATION_TOKEN` | пусто | Bootstrap token внешнего runner protocol MVP |
| `CICD_RUNNER_CREDENTIAL` | пусто | Credential внешнего `forge-runner`, полученный при регистрации |
| `CICD_RUNNER_NAME` | `forge-runner` | Имя внешнего runner-а |
| `CICD_RUNNER_TAGS` | `linux,host` | Теги внешнего runner-а |
| `CICD_RUNNER_TOTAL_SLOTS` | `1` | Capacity внешнего runner-а; current `forge-runner` выполняет один offer за раз и heartbeat-ит active lease |
| `CICD_RUNNER_POLL_INTERVAL_SECONDS` | `5` | Интервал пустого poll: `forge-runner` передаёт `waitSeconds=min(value,30)` и досыпает остаток только после мгновенного `204` |
| `CICD_RUNNER_NO_CHECKOUT` | `false` | Запускать команды в пустом workspace без Git checkout |
| `CICD_RUNNER_WORK_DIR` | temp dir `forge-runner` | Workspace root внешнего runner-а |
| `CICD_RUNNER_KEEP_WORKSPACE` | `false` | Не удалять workspace после job; truthy: `true`, `1`, `yes`, `on` |
| `CICD_API_URL` | `http://127.0.0.1:22801` | Base URL для `cicd-cli` |
| `CICD_TIMEOUT_SECONDS` | `60` | Общий HTTP timeout для `cicd-cli`; `--timeout-seconds` имеет приоритет |
| `CICD_API_TOKEN` | пусто | Bearer PAT/JWT для `cicd-cli` в auth-mode |
| `CICD_OUTPUT` | `json` | Формат вывода `cicd-cli`: `json` или `table` |
| `CICD_ARTIFACT_RETENTION_DAYS` | `30` | TTL новых артефактов; пустое значение равно default, `0` не допускается |

## Генерация ключа

```bash
openssl rand -base64 32   # CICD_SECRETS_KEY
openssl rand -hex 16      # CICD_GIT_TOKEN / CICD_GIT_INTERNAL_TOKEN
openssl rand -hex 32      # CICD_RUNNER_REGISTRATION_TOKEN
```

> Удалённое legacy-значение `forge-internal-dev-token` отклоняется при старте backend. Для shared deployment задайте уникальный `CICD_GIT_INTERNAL_TOKEN`; пустое значение означает trusted-local режим без проверки internal hook token.

> Ротация `CICD_SECRETS_KEY` без перешифровки секретов сделает их нечитаемыми — см. `docs/STORAGE_ARCHITECTURE.md` (key rotation).

## Валидация значений

- Bool-переменные backend (`CICD_EMBEDDED_RUNNER_ENABLED`, `CICD_RUNNER_KEEP_WORKSPACE`, `CICD_AUTH_COOKIE_SECURE`) принимают `true/false`, `1/0`, `yes/no`, `on/off`.
- `CICD_RUNNER_MODE` принимает только `docker` или `host`.
- `CICD_RUNNER_QUEUE_TIMEOUT_SECONDS` принимает `0` для отключения или `1..2592000` секунд.
- `CICD_ARTIFACT_RETENTION_DAYS` принимает `1..3650`.
- `CICD_SECRETS_KEY` должен быть base64-encoded 32 bytes.
- `CICD_CORS_ALLOWED_ORIGINS` не принимает explicit `*`; непустое значение должно содержать хотя бы один origin.
