# Reports — CI/CD метрики и отчёты Forge CI/CD

## 1. Обзор

План Phase 9: сбор и визуализация CI/CD метрик — success rate, средняя длительность, история пайплайнов, частота деплоев. Текущий MVP отдаёт отчёты через project-scoped API и страницу Dashboard `/projects/:id/reports`.

> **Статус:** MVP реализован (`GET /projects/{id}/reports/summary`: total/successful/failed, success_rate, average_duration + страница Reports). Не реализовано: фильтры по периоду, графики, failure trends. См. `docs/ROADMAP.md` Phase 9.

---

## 2. Метрики

### 2.1. Pipeline Success Rate

Доля успешных пайплайнов от общего числа завершённых.

```
success_rate = count(status = 'success') / count(status IN ('success','failed','canceled')) * 100
```

| Период | SQL |
|---|---|
| За последние 7 дней | `WHERE finished_at >= now() - interval '7 days'` |
| За последние 30 дней | `WHERE finished_at >= now() - interval '30 days'` |
| За период | `WHERE finished_at BETWEEN :from AND :to` |

### 2.2. Average Duration

Среднее время выполнения пайплайна от `started_at` до `finished_at`.

```
avg_duration = avg(finished_at - started_at) WHERE status IN ('success','failed','canceled')
```

### 2.3. Pipeline Duration Percentiles

| Перцентиль | Описание |
|---|---|
| p50 (median) | Медианное время выполнения |
| p90 | 90% пайплайнов укладываются в это время |
| p95 | 95% пайплайнов |
| p99 | 99% пайплайнов |

```sql
SELECT
    percentile_cont(0.50) WITHIN GROUP (ORDER BY EXTRACT(EPOCH FROM (finished_at - started_at))) AS p50,
    percentile_cont(0.90) WITHIN GROUP (ORDER BY EXTRACT(EPOCH FROM (finished_at - started_at))) AS p90,
    percentile_cont(0.95) WITHIN GROUP (ORDER BY EXTRACT(EPOCH FROM (finished_at - started_at))) AS p95,
    percentile_cont(0.99) WITHIN GROUP (ORDER BY EXTRACT(EPOCH FROM (finished_at - started_at))) AS p99
FROM pipelines
WHERE status IN ('success','failed','canceled')
  AND finished_at >= now() - interval '30 days';
```

### 2.4. Deployment Frequency

Количество пайплайнов со stage `deploy` в статусе `success` за период.

```sql
SELECT
    date_trunc('day', p.finished_at) AS day,
    count(*) AS deployments
FROM pipelines p
JOIN stages s ON s.pipeline_id = p.id
WHERE s.name = 'deploy'
  AND s.status = 'success'
  AND p.finished_at >= now() - interval '30 days'
GROUP BY day
ORDER BY day;
```

### 2.5. Failure Rate by Stage

Доля неудач по стадиям.

```sql
SELECT
    s.name AS stage_name,
    count(*) FILTER (WHERE s.status = 'failed') AS failed_count,
    count(*) AS total_count,
    ROUND(count(*) FILTER (WHERE s.status = 'failed')::numeric / count(*) * 100, 2) AS failure_rate
FROM stages s
WHERE s.status IN ('success','failed','canceled')
  AND s.name IN ('build','test','deploy')
GROUP BY s.name
ORDER BY failure_rate DESC;
```

### 2.6. MTTR (Mean Time To Recovery)

Среднее время от `failed` пайплайна до следующего `success` в том же проекте на том же ref.

```
mttr = avg(time_between_failed_and_next_success)
```

---

## 3. API: current и target

### 3.1. Endpoints

| Метод | Путь | Назначение |
|---|---|---|
| `GET` | `/api/v1/projects/{id}/reports/summary` | Current verified: total/successful/failed, success_rate, average_duration_seconds |
| `GET` | `/api/v1/projects/{id}/reports/pipeline-history` | Target approved: история пайплайнов проекта с фильтрами |
| `GET` | `/api/v1/projects/{id}/reports/success-rate` | Target approved: success rate по дням/неделям |
| `GET` | `/api/v1/projects/{id}/reports/duration` | Target approved: duration stats (p50, p90, p95, p99) |
| `GET` | `/api/v1/projects/{id}/reports/deployment-frequency` | Target approved: частота деплоев по дням |
| `GET` | `/api/v1/projects/{id}/reports/failure-breakdown` | Target approved: breakdown неудач по стадиям |
| `GET` | `/api/v1/reports/summary` | Target approved: cross-project aggregate только после scoped RBAC/tenant policy |

### 3.2. Current: GET /api/v1/projects/{id}/reports/summary

```bash
curl -sS "http://127.0.0.1:22801/api/v1/projects/{id}/reports/summary"
```

```json
{
  "total_pipelines": 42,
  "successful_pipelines": 34,
  "failed_pipelines": 6,
  "success_rate": 0.8095,
  "average_duration_seconds": 187.4
}
```

`success_rate` — доля `0..1`; frontend отображает её как процент.

### 3.3. Target: GET /api/v1/projects/{id}/reports/success-rate

```bash
curl -sS "http://127.0.0.1:22801/api/v1/projects/{id}/reports/success-rate?period=30d&group_by=day"
```

```json
[
  { "date": "2026-08-20", "total": 14, "success": 12, "failed": 2, "successRate": 85.71 },
  { "date": "2026-08-21", "total": 18, "success": 17, "failed": 1, "successRate": 94.44 },
  { "date": "2026-08-22", "total": 10, "success": 9,  "failed": 1, "successRate": 90.00 }
]
```

### 3.4. Target: GET /api/v1/projects/{id}/reports/pipeline-history

```bash
curl -sS "http://127.0.0.1:22801/api/v1/projects/{id}/reports/pipeline-history?limit=50&status=failed"
```

```json
[
  {
    "pipelineId": "uuid",
    "projectName": "my-service",
    "gitRef": "main",
    "status": "failed",
    "startedAt": "2026-08-26T10:00:00Z",
    "finishedAt": "2026-08-26T10:03:12Z",
    "durationSecs": 192,
    "failedStage": "build",
    "failedJob": "compile"
  }
]
```

### 3.5. Параметры фильтрации

| Параметр | Тип | Описание |
|---|---|---|
| `from` | ISO 8601 | Начало периода |
| `to` | ISO 8601 | Конец периода |
| `status` | string | Фильтр по статусу |
| `group_by` | `day` / `week` / `month` | Группировка |
| `period` | `7d` / `30d` / `90d` | Быстрый выбор периода |
| `limit` | integer | Лимит записей (default 50, max 200) |

---

## 4. DORA Metrics

Forge CI/CD частично покрывает DORA (DevOps Research and Assessment) метрики:

| Метрика | Описание | Источник данных |
|---|---|---|
| **Deployment Frequency** | Как часто деплоится в production | `stages WHERE name='deploy' AND status='success'` |
| **Lead Time for Changes** | Время от коммита до деплоя | Future: Git integration + pipeline timestamps |
| **Change Failure Rate** | Доля неудачных деплоев | `pipelines WHERE failed_stage='deploy'` |
| **MTTR** | Среднее время восстановления | `time between failed and next success pipeline` |

---

## 5. Frontend

### 5.1. Project Reports Page

- Текущий маршрут: `/projects/:id/reports`.
- MVP показывает summary cards из `GET /api/v1/projects/{id}/reports/summary`.
- Target-графики и вкладки Overview, Pipelines, Deployments, Failures остаются в project-scoped workflow.
- Cross-project admin reports требуют отдельного scoped RBAC/tenant решения и не входят в текущий `/admin` baseline.
- Период-селектор: 7d / 30d / 90d / custom range.
- Проект-селектор: all / конкретный проект.

### 5.2. Графики (recharts)

| График | Тип | Данные |
|---|---|---|
| Success Rate Trend | Line chart | success-rate по дням |
| Duration Distribution | Histogram | distribution длительностей |
| Duration Percentiles | Bar chart | p50, p90, p95, p99 |
| Deployment Frequency | Bar chart | deployments по дням |
| Failure Breakdown | Pie/Donut chart | неудачи по стадиям |
| Pipeline Activity | Area chart | total/success/failed по дням |

### 5.3. Компоненты

```typescript
// SuccessRateChart.tsx
<ResponsiveContainer width="100%" height={300}>
  <LineChart data={successRateData}>
    <XAxis dataKey="date" />
    <YAxis domain={[0, 100]} unit="%" />
    <Tooltip />
    <Line dataKey="successRate" stroke="#22c55e" />
  </LineChart>
</ResponsiveContainer>
```

---

## 6. SQL-запросы (справочно)

### 6.1. Success Rate по дням

```sql
SELECT
    date_trunc('day', finished_at) AS day,
    count(*) AS total,
    count(*) FILTER (WHERE status = 'success') AS success,
    count(*) FILTER (WHERE status = 'failed') AS failed,
    count(*) FILTER (WHERE status = 'canceled') AS canceled
FROM pipelines
WHERE finished_at >= now() - interval '30 days'
  AND status IN ('success','failed','canceled')
GROUP BY day
ORDER BY day;
```

### 6.2. Duration distribution

```sql
SELECT
    width_bucket(EXTRACT(EPOCH FROM (finished_at - started_at)), 0, 600, 12) AS bucket,
    count(*) AS count
FROM pipelines
WHERE status IN ('success','failed','canceled')
  AND finished_at >= now() - interval '30 days'
GROUP BY bucket
ORDER BY bucket;
```

### 6.3. Top failing jobs

```sql
SELECT
    j.name AS job_name,
    s.name AS stage_name,
    count(*) AS failures
FROM jobs j
JOIN stages s ON j.stage_id = s.id
WHERE j.status = 'failed'
  AND j.finished_at >= now() - interval '30 days'
GROUP BY j.name, s.name
ORDER BY failures DESC
LIMIT 10;
```

---

## 7. Кэширование

Отчёты — тяжёлые SQL-запросы. Стратегия кэширования:

| Уровень | Механизм | TTL |
|---|---|---|
| Frontend | `@tanstack/react-query` staleTime | 60 секунд |
| API in-memory | `tokio::sync::RwLock<Cache>` | 5 минут |
| БД | Materialized views (future) | 1 час |

Кэш инвалидируется при:
- Новом завершённом пайплайне (pipeline → terminal status).
- Target-ручном запросе `POST /api/v1/reports/refresh`.

---

## 8. Env-переменные (план)

| Переменная | Default | Описание |
|---|---|---|
| `CICD_REPORTS_ENABLED` | `true` | Глобальный выключатель |
| `CICD_REPORTS_CACHE_TTL` | `300` | TTL кэша отчётов (секунды) |
| `CICD_REPORTS_MAX_RANGE_DAYS` | `365` | Макс. диапазон периода (дней) |
| `CICD_REPORTS_MAX_LIMIT` | `200` | Макс. лимит записей |

---

## 9. План реализации

- [ ] API endpoints: project-scoped success-rate, duration, deployment-frequency, failure-breakdown, pipeline-history.
- [ ] SQL-запросы с агрегациями и percentile_cont.
- [ ] In-memory кэш с TTL.
- [ ] Frontend: расширить `/projects/:id/reports` страницу графиками (recharts).
- [ ] Период-селектор, проект-селектор.
- [ ] Тесты: корректность агрегаций, кэширование, API contract.

---

## References

- `docs/ROADMAP.md` — Phase 9: Reports
- `docs/DATA_MODEL.md` — таблицы `pipelines`, `stages`, `jobs`
- `docs/WORKFLOW.md` — статусы и терминальные состояния
- `docs/API.md` — REST API спецификация
