# Forge CI/CD

Self-hosted CI/CD control plane для FerrPOINT: Git hosting, pipeline runs, job logs, artifacts, secrets, environments, reports, notifications and audit in one local stack.

| Поле | Значение |
|---|---|
| Статус | MVP `0.1.x`, запускать только в trusted network или за reverse proxy |
| Backend | Rust 2024, Axum 0.8, SQLx 0.8, PostgreSQL 17 |
| Frontend | React 19, Vite 6, Tailwind CSS 4, shadcn/ui |
| Runtime | Docker Compose, embedded Docker/shell runner, Git Smart HTTP |
| Порты | Dashboard `22802`, API/Git `22801`, PostgreSQL `22543` на `127.0.0.1` |
| Лицензия | [FerrPOINT Proprietary Source-Available Evaluation License v1.0](LICENSE) |

## Что есть

- Bare Git hosting со Smart HTTP, branch/tag browser, сравнение веток и pull-request flow.
- Pipelines, stages, jobs, bounded job logs и embedded runner для Docker/shell-команд.
- Артефакты до 50 MiB в локальном хранилище, environments/deployments и reports.
- Секреты с AES-256-GCM at rest, env injection в runner и masking stdout.
- Auth/RBAC при заданном `CICD_AUTH_SECRET`: login/JWT, scoped PAT, session-bound access JWT, refresh rotation, global roles и project membership checks.
- Schedules, outgoing webhooks, `in_app`/`sse` notifications, outbox delivery history, attempt log и ручной requeue failed-доставок как bounded MVP.
- Документированные API/contracts, ADR, threat model, SLO/metrics/DR и screenshot evidence.

## Границы

- Без `CICD_AUTH_SECRET` API и Dashboard работают в trusted-network режиме.
- TLS, distributed rate limiting, tenant isolation, service-account tokens, scoped Git credentials, production cookie/CSRF/session-family policy и external notification adapters остаются production-hardening задачами.
- Scheduler/outbox уже исполняет MVP-сценарии, но не заявляет crash-safe/distributed delivery guarantees.

Полный честный срез: [docs/CURRENT_STATE.md](docs/CURRENT_STATE.md). Security policy: [SECURITY.md](SECURITY.md).

## Быстрый старт

```bash
cp .env.example .env
echo "CICD_SECRETS_KEY=$(openssl rand -base64 32)" >> .env
docker compose up --build -d
curl http://127.0.0.1:22801/api/v1/health
```

Dashboard: `http://127.0.0.1:22802`.

Минимальный Git flow:

```bash
curl -X POST http://127.0.0.1:22801/api/v1/repositories \
  -H 'content-type: application/json' \
  -d '{"name":"my-service"}'

git clone http://127.0.0.1:22802/git/my-service.git
# add .forge-ci.yml, commit, push
git push
```

## Работа

| Команда | Назначение |
|---|---|
| `just up` / `just down` | Собрать или остановить Docker Compose stack |
| `just health` | Проверить API health |
| `just test-backend` | Backend unit и contract tests |
| `just test-frontend` | Vitest |
| `just build-frontend` | Production build SPA |
| `python3 scripts/verify_docs.py --all` | Проверка документации и ссылок |

CLI: [docs/CLI.md](docs/CLI.md). Конфигурация окружения: [docs/ENV.md](docs/ENV.md).

## Структура

```text
CI-CD/
├── backend/           # Rust workspace: API, domain, CLI, Git hosting, runner, store
├── frontend/          # React dashboard with generated API hooks
├── docs/              # architecture, contracts, operations, quality, screenshots
├── scripts/           # documentation and verification helpers
├── docker-compose.yml # postgres + backend + frontend
└── justfile           # local workflow commands
```

## Документы

- [docs/README.md](docs/README.md) - карта документации.
- [docs/USER_GUIDE.md](docs/USER_GUIDE.md) - пользовательские сценарии.
- [docs/DEVELOPMENT_GUIDE.md](docs/DEVELOPMENT_GUIDE.md) - разработка.
- [docs/OPERATIONS.md](docs/OPERATIONS.md) и [docs/TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md) - эксплуатация.
- [docs/ARCHITECTURE_INDEX.md](docs/ARCHITECTURE_INDEX.md), [docs/contracts](docs/contracts) и [docs/ADR.md](docs/ADR.md) - архитектура и контракты.
- [docs/TEST_PLAN.md](docs/TEST_PLAN.md), [docs/TRACEABILITY.md](docs/TRACEABILITY.md), [docs/THREAT_MODEL.md](docs/THREAT_MODEL.md) - качество и безопасность.

Скриншоты хранятся в [docs/assets/screens/manifest.md](docs/assets/screens/manifest.md); README больше не дублирует весь визуальный реестр.

## Лицензия

Proprietary source-available. Not open source.

Viewing/evaluation only.

Commercial, production, resale, redistribution, SaaS/hosting use require written license from FerrPOINT. См. [LICENSE](LICENSE), [NOTICE](NOTICE) и [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
