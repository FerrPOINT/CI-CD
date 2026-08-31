# Forge CI/CD

Self-hosted control plane для Git-репозиториев и CI/CD: bare Git-хостинг со Smart HTTP, автозапуск пайплайнов на push, выполнение jobs в Docker/shell, артефакты, секреты, окружения и полный аудит — Rust (Axum + SQLx + PostgreSQL) + React (Vite + Tailwind + shadcn/ui).

## ⚠️ Статус и границы доверия

Продукт находится в стадии **MVP (0.1.x)** и **не готов к эксплуатации в недоверенных сетях**:

- если `CICD_AUTH_SECRET` не задан или пустой, API и Dashboard работают в trusted-network режиме без auth enforcement;
- при заданном `CICD_AUTH_SECRET` включаются login/JWT/PAT, global roles и project membership RBAC, но tenant isolation и scoped PAT остаются target;
- нет TLS, CORS permissive, rate limiting пока in-process и не заменяет reverse proxy/distributed limiter;
- schedules и outgoing webhooks работают как MVP worker; notifications, inbound provider webhooks и production-grade delivery guarantees ещё не реализованы.

Полный честный срез: [docs/CURRENT_STATE.md](docs/CURRENT_STATE.md) · политика: [SECURITY.md](SECURITY.md). До Phase D security запускайте только в доверенной сети / за reverse proxy.

### Возможности и статусы

| Возможность | Статус |
|---|---|
| Проекты, пайплайны, jobs, логи (embedded runner: Docker/shell) | ✅ Current verified |
| Git-хостинг: bare + Smart HTTP + `post-receive` авто-триггер | ✅ Current verified |
| Артефакты (≤50 MiB, локальное хранилище) | ✅ Current verified |
| Секреты (AES-256-GCM at rest; env injection в embedded runner + masking stdout) | ✅ Current verified |
| Окружения/деплои, отчёты, аудит (200 событий) | ✅ Current verified |
| Auth/RBAC/сессии/PAT enforcement при `CICD_AUTH_SECRET` | ✅ Current verified |
| Schedules и outgoing webhooks | ✅ Current verified MVP |
| Notifications и inbound provider webhooks | ⚙️ Configuration only |
| Внешние runner-ы (lease/protocol), tenant/scoped PAT, production scheduler/outbox guarantees | 🎯 Target approved |

Легенда: ✅ работает сейчас · ⚙️ конфигурация без исполнения · 🎯 принято в архитектуру ([ADR](docs/ADR.md), [контракты](docs/contracts/)).

## Быстрый старт

```bash
cp .env.example .env
echo "CICD_SECRETS_KEY=$(openssl rand -base64 32)" >> .env   # обязательно для secrets
docker compose up --build -d
curl http://127.0.0.1:22801/api/v1/health
# Dashboard: http://127.0.0.1:22802
```

Порты: Dashboard `22802` · API + Git Smart HTTP `22801` · PostgreSQL `22543` (только `127.0.0.1`).

```bash
# Простейший CI-флоу
curl -X POST http://127.0.0.1:22801/api/v1/repositories \
  -H 'content-type: application/json' -d '{"name":"my-service"}'
git clone http://127.0.0.1:22802/git/my-service.git
# … коммитим .forge-ci.yml и код, затем:
git push                       # → post-receive → пайплайн создан и выполнен
```

Конфигурация: [docs/ENV.md](docs/ENV.md) · CLI: [docs/CLI.md](docs/CLI.md).

## Скриншоты

### Дашборд

![Дашборд](docs/screenshots/02-dashboard.png)

### Пайплайн: стадии, jobs и логи

![Детали пайплайна](docs/screenshots/06-pipeline-detail.png)

### Код-ревью: diff веток

![Сравнение веток](docs/screenshots/10-compare.png)

### Pull-запросы

![Pull-запросы](docs/screenshots/11-pull-requests.png)

### Секреты проекта

![Секреты проекта](docs/screenshots/14-secrets.png)

### Diff из pull-запроса

![Diff из pull-запроса](docs/screenshots/22-pr-diff.png)

### Логи джоба

![Логи джоба](docs/screenshots/33-job-logs.png)

### Подтверждение удаления

![Подтверждение удаления проекта](docs/screenshots/24-project-delete-confirm.png)

### Мобильная версия (375×812)

![Дашборд — мобильная версия](docs/screenshots/m-dashboard.png)

Полный визуальный реестр (45 скринов: 21 базовое состояние + 18 состояний действий + 6 мобильных): [docs/assets/screens/manifest.md](docs/assets/screens/manifest.md).

## Документация

**По аудитории:**

- Полная карта документации — [docs/README](docs/README.md)
- Пользователь/владелец проекта — [USER_GUIDE](docs/USER_GUIDE.md)
- Разработчик — [DEVELOPMENT_GUIDE](docs/DEVELOPMENT_GUIDE.md)
- Оператор — [OPERATIONS](docs/OPERATIONS.md), [TROUBLESHOOTING](docs/TROUBLESHOOTING.md)
- Обозреватель безопасности — [SECURITY](SECURITY.md), [CURRENT_STATE](docs/CURRENT_STATE.md)
- Продукт/требования — [PRODUCT_REQUIREMENTS](docs/PRODUCT_REQUIREMENTS.md), [ROADMAP](docs/ROADMAP.md)

**Архитектура** (входная точка — [ARCHITECTURE_INDEX](docs/ARCHITECTURE_INDEX.md)):

- Целевые контракты (нормативные): [contracts/](docs/contracts/) — API, AUTHZ, RUNNER_PROTOCOL, PIPELINE_DSL, EVENT, DATA_LIFECYCLE, MIGRATION, UI_API
- Narrative: [ARCHITECTURE](docs/ARCHITECTURE.md), [FUNCTIONAL_ARCHITECTURE](docs/FUNCTIONAL_ARCHITECTURE.md), [AUTHORIZATION](docs/AUTHORIZATION.md), [RUNNER](docs/RUNNER_ARCHITECTURE.md), [AUTOMATION](docs/AUTOMATION_ARCHITECTURE.md), [STORAGE](docs/STORAGE_ARCHITECTURE.md), [DELIVERY](docs/DELIVERY_ARCHITECTURE.md), [architecture/](docs/architecture/) (границы, topology, transition map, sequence-флоу)
- Справочники: [API](docs/API.md), [DATA_MODEL](docs/DATA_MODEL.md), [GIT_HOSTING](docs/GIT_HOSTING.md), [ENV](docs/ENV.md), [CLI](docs/CLI.md), [REPORTS](docs/REPORTS.md), [LIBRARIES](docs/LIBRARIES.md)
- Реализация (исполнимые спецификации): [IMPLEMENTATION_CONTRACTS](docs/IMPLEMENTATION_CONTRACTS.md) · [MIGRATION_SPEC](docs/MIGRATION_EXECUTION_SPEC.md) · [AUTH_SPEC](docs/AUTH_IMPLEMENTATION_SPEC.md) · [EXECUTION_SPEC](docs/EXECUTION_AUTOMATION_IMPLEMENTATION_SPEC.md) · [ADR 0001–0009](docs/ADR.md)

**Качество, безопасность и SDLC:** [TEST_PLAN](docs/TEST_PLAN.md) · [TRACEABILITY](docs/TRACEABILITY.md) · [THREAT_MODEL](docs/THREAT_MODEL.md) · [RISK_REGISTER](docs/RISK_REGISTER.md) · [ACCESSIBILITY](docs/ACCESSIBILITY.md) · [THIRD_PARTY/SBOM](docs/THIRD_PARTY.md) · [SLO](docs/SLO.md) · [METRICS](docs/METRICS.md) · [DISASTER_RECOVERY](docs/DISASTER_RECOVERY.md) · [INCIDENT_RESPONSE](docs/INCIDENT_RESPONSE.md)

**Участие и политика:** [DOCUMENTATION_GOVERNANCE](docs/DOCUMENTATION_GOVERNANCE.md) · [CONTRIBUTING](CONTRIBUTING.md) · [SECURITY](SECURITY.md) · [SUPPORT](SUPPORT.md) · [CHANGELOG](CHANGELOG.md) · лицензия [MIT](LICENSE)

## Структура

```text
CI-CD/
├── backend/                 # Cargo workspace: серверный crate + domain/ + cli/
│   ├── src/                 # api/platform/git_host/pulls/runner/store (мигрирует по ADR-0005)
│   ├── domain/              # cicd-domain: чистые типы + JobStatus
│   ├── cli/                 # cicd-cli: HTTP-клиент control plane
│   └── tests/ + docker-compose.test.yml   # contract-тесты + test-DB fixture
├── frontend/                # React SPA (pages, widgets, typed API hooks)
├── docs/                    # guides, contracts, architecture, adr, screenshots
├── scripts/verify_docs.py   # линтер документации (ссылки/канон/статусы)
├── docker-compose.yml       # postgres (loopback) + backend + frontend
└── justfile                 # just up/down/health/test-*
```

## Команды

| Команда | Описание |
|---|---|
| `just up` / `just down` | Собрать/остановить Docker Compose стек |
| `just health` | Проверить API health-check |
| `just test-backend` | Backend unit и contract tests (через rust:1.86) |
| `just test-frontend` / `just build-frontend` | Vitest / production build |
| `python3 scripts/verify_docs.py --all` | Проверка документации |

## Лицензия

MIT — см. [LICENSE](LICENSE).
