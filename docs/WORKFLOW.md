# Workflow — Статусная модель пайплайнов Forge CI/CD

## 1. Обзор

Единая модель переходов статусов для пайплайнов, стадий и задач. Статус — enum `JobStatus` с методом `transition_to()` — единственный источник правды для валидации переходов.

Иерархия агрегации: **job → stage → pipeline**. Статус каждого уровня вычисляется из дочерних элементов по детерминированным правилам.

---

## 2. Статусы

Все три сущности (pipeline, stage, job) используют один набор статусов:

| Статус | Описание | Терминальный |
|---|---|---|
| `queued` | Создан, ожидает выполнения | Нет |
| `running` | Выполняется в данный момент | Нет |
| `success` | Завершён успешно | Да |
| `failed` | Завершён с ошибкой | Да |
| `canceled` | Отменён пользователем или системой | Да |

### Терминальные состояния

Терминальные статусы — `success`, `failed`, `canceled`. Из терминального статуса переход невозможен. После перехода в терминальный статус:

- Заполняется `finished_at` (TIMESTAMPTZ).
- Дальнейшие переходы отклоняются с ошибкой 409 Conflict.
- Допускается только чтение и удаление родительской сущности.

---

## 3. Диаграмма переходов

```
                         ┌──────────┐
                         │  queued  │
                         └────┬─────┘
                              │
            ┌─────────────────┼─────────────────┐
            ▼                 ▼                  ▼
      ┌──────────┐     ┌──────────┐      ┌───────────┐
      │ running  │     │ canceled │      │  success  │
      └────┬─────┘     └──────────┘      └───────────┘
           │            (из queued)       (из queued,
           │                                теоретически)
           │
     ┌─────┴──────┐
     ▼            ▼
┌──────────┐ ┌───────────┐
│ success  │ │  failed   │
└──────────┘ └───────────┘
```

---

## 4. Правила переходов

### 4.1. Матрица допустимых переходов

| Из \ В | `queued` | `running` | `success` | `failed` | `canceled` |
|---|---|---|---|---|---|
| `queued` | — | ✅ | — | — | ✅ |
| `running` | — | — | ✅ | ✅ | ✅ |
| `success` | — | — | — | — | — |
| `failed` | — | — | — | — | — |
| `canceled` | — | — | — | — | — |

### 4.2. Подробное описание переходов

| Переход | Условие | Побочный эффект |
|---|---|---|
| `queued → running` | Запуск выполнения | `started_at = now()` |
| `queued → canceled` | Ручная отмена до запуска | `finished_at = now()` |
| `running → success` | Успешное завершение | `finished_at = now()` |
| `running → failed` | Ошибка выполнения | `finished_at = now()` |
| `running → canceled` | Ручная отмена во время выполнения | `finished_at = now()` |

### 4.3. Недопустимые переходы

- `queued → success` — задача не может завершиться успешно без запуска.
- `queued → failed` — задача не может завершиться с ошибкой без запуска.
- `running → queued` — нельзя вернуть в очередь из выполнения.
- Любой переход из терминального статуса — запрещён.

При попытке недопустимого перехода API возвращает **409 Conflict**:

```json
{
  "error": "invalid_transition",
  "message": "Cannot transition from 'success' to 'running'",
  "from": "success",
  "to": "running"
}
```

---

## 5. Агрегация статусов

### 5.1. Принцип

Статус родительской сущности вычисляется из статусов дочерних. Агрегация выполняется автоматически при каждом изменении статуса job.

```
Job (1..N) ──aggregate──▶ Stage (1..N) ──aggregate──▶ Pipeline
```

### 5.2. Правила агрегации: jobs → stage

Статус stage вычисляется из статусов всех её jobs:

| Условие | Статус stage |
|---|---|
| Все jobs `queued` | `queued` |
| Хотя бы один job `running`, остальные `queued`/`running` | `running` |
| Все jobs `success` | `success` |
| Хотя бы один job `failed`, остальные терминальные | `failed` |
| Хотя бы один job `canceled`, остальные терминальные (без `failed`) | `canceled` |
| Есть `failed` и `canceled` одновременно | `failed` (приоритет ошибки) |

**Приоритет ошибок:** `failed` > `canceled` > `running` > `queued` > `success`.

### 5.3. Правила агрегации: stages → pipeline

Статус pipeline вычисляется из статусов всех её stages по тем же правилам, что и jobs → stage.

| Условие | Статус pipeline |
|---|---|
| Все stages `queued` | `queued` |
| Хотя бы одна stage `running`, остальные `queued`/`running` | `running` |
| Все stages `success` | `success` |
| Хотя бы одна stage `failed`, остальные терминальные | `failed` |
| Хотя бы одна stage `canceled`, остальные терминальные (без `failed`) | `canceled` |
| Есть `failed` и `canceled` одновременно | `failed` (приоритет ошибки) |

### 5.4. Пустые сущности

| Сущность | Условие | Статус |
|---|---|---|
| Stage без jobs | Нет дочерних jobs | `queued` (по умолчанию) |
| Pipeline без stages | Нет дочерних stages | `queued` (по умолчанию) |

### 5.5. Каскадная отмена

При отмене pipeline (`queued → canceled` или `running → canceled`):

1. Все дочерние stages в нетерминальном статусе переводятся в `canceled`.
2. Все дочерние jobs этих stages в нетерминальном статусе переводятся в `canceled`.
3. `finished_at` заполняется на всех уровнях.

При отмене stage аналогично отменяются все её jobs в нетерминальном статусе.

---

## 6. Жизненный цикл пайплайна (пример)

### 6.1. Создание и запуск

```
1. POST /api/v1/pipelines {project_id, git_ref, stages}
   → Pipeline: queued
   → Stages: queued (все)
   → Jobs: queued (все)

2. Job "checkout" → running
   → Stage "build": running (агрегация)
   → Pipeline: running (агрегация)

3. Job "checkout" → success
   → Stage "build": running (есть другие jobs в queued)
   → Pipeline: running

4. Job "compile" → running → success
   → Stage "build": success (все jobs success)
   → Pipeline: running (есть stages в queued)

5. Stage "test" jobs → running → success
   → Stage "test": success
   → Pipeline: running (stage "deploy" в queued)

6. Stage "deploy" jobs → running → success
   → Stage "deploy": success
   → Pipeline: success (все stages success)
```

### 6.2. Сценарий с ошибкой

```
1. Pipeline: queued → running (job "compile" запущен)

2. Job "compile" → failed
   → Stage "build": failed
   → Pipeline: failed (агрегация)
   → Stage "test": canceled (каскадная отмена)
   → Stage "deploy": canceled (каскадная отмена)
   → Jobs в "test" и "deploy": canceled
```

### 6.3. Сценарий отмены

```
1. Pipeline: running

2. POST /api/v1/pipelines/{id}/cancel
   → Pipeline: canceled
   → Все нетерминальные stages → canceled
   → Все нетерминальные jobs → canceled
   → finished_at заполняется на всех уровнях
```

---

## 7. API переходов

### 7.1. Перевод статуса job

```bash
PATCH /api/v1/jobs/{id}/status
Content-Type: application/json

{
  "status": "running"
}
```

Возможные ответы:

| HTTP | Описание |
|---|---|
| 200 | Переход выполнен, возвращён обновлённый job |
| 409 | Недопустимый переход (`invalid_transition`) |
| 404 | Job не найден |

### 7.2. Отмена pipeline

```bash
POST /api/v1/pipelines/{id}/cancel
```

Каскадно отменяет все нетерминальные дочерние сущности.

---

## 8. Timestamps

| Статус | `started_at` | `finished_at` |
|---|---|---|
| `queued` | `NULL` | `NULL` |
| `running` | `NOT NULL` | `NULL` |
| `success` | `NOT NULL` | `NOT NULL` |
| `failed` | `NOT NULL` | `NOT NULL` |
| `canceled` | `NULL` или `NOT NULL` | `NOT NULL` |

> `canceled` из `queued`: `started_at` остаётся `NULL` (задача не начиналась).
> `canceled` из `running`: `started_at` уже заполнен.

---

## 9. CHECK constraints в БД

Все три таблицы (`pipelines`, `stages`, `jobs`) имеют CHECK constraint:

```sql
CHECK (status IN ('queued','running','success','failed','canceled'))
```

Это гарантирует валидность статусов на уровне БД даже при прямых SQL-запросах.

---

## 10. Реализация в коде

### 10.1. Enum JobStatus

```rust
pub enum JobStatus {
    Queued,
    Running,
    Success,
    Failed,
    Canceled,
}

impl JobStatus {
    pub fn transition_to(&self, target: &JobStatus) -> Result<(), TransitionError> {
        // Единственный источник правды для валидации переходов
    }
}
```

### 10.2. Агрегация

```rust
pub fn aggregate_status(statuses: &[JobStatus]) -> JobStatus {
    // Приоритет: failed > canceled > running > queued > success
}
```

### 10.3. Слои

- `domain` — `JobStatus`, `transition_to()`, `aggregate_status()`.
- `store` — SQL-запросы, обновление статусов, каскадная отмена.
- `api` — HTTP-хендлеры, валидация входных данных, вызов domain-логики.

---

## References

- `docs/DATA_MODEL.md` — таблицы `pipelines`, `stages`, `jobs`, CHECK constraints
- `docs/API.md` — эндпоинты переходов статусов
- `docs/ARCHITECTURE.md` — слои `api → domain → store`
- `docs/ROADMAP.md` — Phase 5: real runner (автоматические переходы)