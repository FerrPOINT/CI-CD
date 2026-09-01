# SLO — Service Level Objectives

> **Статус:** Current verified MVP для synthetic CI regression smoke; production SLO остаются Target approved. `frontend/e2e/performance.spec.ts` измеряет seeded API read p95 и Dashboard route ready-time на disposable Compose stack, но SLO ниже не считаются достигнутыми без 30 дней реальных метрик и нагрузочного evidence.
> Основание: [ADR-0009](adr/0009-canonical-registry.md). Потребности продукта: `PRODUCT_REQUIREMENTS.md` §7.

## 1. Модель

- **SLI** — измеримый индикатор (доля хороших событий / измерений).
- **SLO** — целевой уровень SLI за окно 30 дней.
- **Error budget** = 100% − SLO. Бюджет тратится на инновации; при исчерпании — приоритет надёжности (freeze рискованных изменений, см. `INCIDENT_RESPONSE.md`).
- Текущий CI performance smoke — регрессионный guard для seeded MVP, а не production SLO-доказательство. Production-измерение — только на реальных данных мониторинга (target: Prometheus-совместимые метрики, см. `docs/contracts/EVENT_CONTRACT.md` §observability и `TECH_CHOICES.md`).

## 2. Сервисные SLO (control plane)

| SLI | Определение | SLO (30d) | Заметки |
|---|---|---|---|
| API availability | доля не-5xx ответов небыстрых маршрутов (кроме /health) | ≥ 99.5% | MVP single-node: цель осознанно умеренная |
| API latency p95 (reads) | GET /projects, /pipelines, /jobs/{id} | ≤ 300 ms | без сетевого RTT клиента |
| API latency p95 (writes) | POST/PUT/PATCH мутации | ≤ 500 ms | включая идемпотентный повтор |
| Git Smart HTTP success | доля успешных clone/fetch/push | ≥ 99.0% | измеряется post-receive-hook исходом |
| Pipeline dispatch latency | время push → job `running` (p95) | ≤ 30 s | target: с внешними runner-ами |
| Job scheduling latency | время lease-offer → job start (p95) | ≤ 10 s | target-протокол `RUNNER_PROTOCOL.md` |
| Webhook delivery success | доля доставок в пределах 8 попыток | ≥ 99.0% | target outbox `EVENT_CONTRACT.md` |

## 2.1. MVP regression budgets

Эти пороги используются только в `frontend/e2e/performance.spec.ts` на synthetic seeded Compose stack:

| SLI | Порог CI smoke | Переопределение |
|---|---|---|
| Seeded API read p95 | ≤ 1000 ms | `E2E_API_READ_P95_BUDGET_MS` |
| Dashboard route ready-time max | ≤ 5000 ms | `E2E_DASHBOARD_READY_BUDGET_MS` |

Падение этих budget-ов считается регрессией MVP. Прохождение не означает соблюдение 30-day SLO из разделов ниже.

## 3. Инфраструктурные SLO

| SLI | SLO (30d) | Заметки |
|---|---|---|
| PostgreSQL availability | ≥ 99.5% | single-node; RTO/RPO — `DISASTER_RECOVERY.md` |
| Dashboard availability | ≥ 99.5% | статический SPA + nginx |
| Artifact download success | ≥ 99.0% | локальное хранилище |

## 4. Правила

1. SLO определяются до реализации измерения; изменение SLO — через правку этого файла в PR с пометкой причины.
2. Ни один SLO не объявляется достигнутым без 30 дней реальных измерений.
3. Error budget PostgreSQL/API расходуется на: эксперименты, миграции, рестарты. Не расходуется на: известные повторяющиеся дефекты.
4. SIRE-порядок при исчерпании бюджета: сначала деградация non-critical фич (scheduler), затем freeze.
5. Отчёт по SLO — часть `METRICS.md` (target: автоматический дайджест).
