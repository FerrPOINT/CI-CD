# METRICS — метрики продукта и разработки

> **Статус:** частично Current verified (audit/reports считаются из БД), остальное Target approved. Количественные пороги зафиксированы как контракт для будущей observability-реализации.
> Основание: [ADR-0009](adr/0009-canonical-registry.md). SLO-цели: `SLO.md`.

## 1. DORA-метрики (разработка)

| Метрика | Определение | Текущий источник | Цель v1 |
|---|---|---|---|
| Deployment frequency | запуски деплой-job на production-окружение / нед | reports (jobs по окружению) | ≥ 1/день при активной разработке |
| Lead time for changes | commit sha → deployment завершён | target: связка pipeline ↔ deployment | ≤ 1 часа (p50) |
| Change failure rate | доля деплоев, после которых был failed job/rollback | reports | ≤ 15% |
| MTTR | время инцидента SEV1/SEV2 → resolution | `INCIDENT_RESPONSE.md` журнал | ≤ 4 часов |

Current: DORA считается вручную из reports/audit; автоматический расчёт — target (см. `contracts/EVENT_CONTRACT.md` события `forge.deployment.*`).

## 2. Операционные метрики (runtime)

| Метрика | Источник | Статус |
|---|---|---|
| HTTP requests: rate / errors / latency p50-p99 (per route) | axum middleware | Target (axum-prometheus, `TECH_CHOICES.md`) |
| Pipeline queue depth (jobs `queued`) | SQL | Target (gauge в метриках) |
| Job duration p50/p95 по проекту | reports | Current (агрегат в UI) |
| Runner heartbeats / online count | runners registry | Current (UI); метрики — Target |
| Outbox backlog / delivery latency / dead-letter count | outbox worker | Target |
| DB: connections, pool saturation, slow queries | sqlx/pg | Target |
| Artifact storage usage / quota | filesystem | Target |
| Git push rate / post-receive failures | git hooks | Target |

## 3. Продуктовые метрики (adoption)

| Метрика | Определение | Статус |
|---|---|---|
| Активные проекты (пайплайн за 7д) | SQL из reports | Current (ручной запрос) |
| Пайплайнов/нед, success rate | reports page | Current |
| Median pipeline duration | reports page | Current |
| Артефактов загружено/скачано | audit | Current (в audit-событиях) |
| Пользователи/токены активные | users registry | Current (UI) |

## 4. Правила сбора

1. Все метрики выводятся из уже зафиксированных событий `contracts/EVENT_CONTRACT.md` или выделенного metrics-канала — без отдельной trace-БД (правило workspace).
2. Никакие метрики не содержат значения секретов, имён токенов или содержимого исходников.
3. Дашборд метрик не входит в Dashboard MVP; экспорт — Prometheus-совместимый endpoint (target).
4. Изменение порогов/метрик — правка этого файла в PR; автоматические алерты ссылаются на `SLO.md`.
