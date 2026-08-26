# Domain Model — Forge CI/CD

## 1. Bounded Contexts

| Контекст | Ответственность | Основные агрегаты |
|----------|-----------------|-------------------|
| Project Management | Проекты-репозитории, настройки | Project |
| Pipeline Execution | Пайплайны, стадии, задачи, переходы статусов | Pipeline, Stage, Job |
| Job Logging | Append-only логи выполнения задач | JobLog |
| Identity & Access (Phase 1) | Пользователи, сессии, роли | User, Session |
| Runner (Phase 5) | Runner-агенты, регистрация, heartbeat | Runner |
| Notifications (Phase 6) | Webhooks, SSE, уведомления | Webhook, Notification |
| Secrets (Phase 7) | Шифрованные секреты проектов | Secret |
| Artifacts (Phase 8) | Артефакты сборки, хранилище | Artifact |
| Admin (Phase 9) | Audit log, системные настройки, отчёты | AuditLog, SystemSetting |

> Текущий MVP (Phase 0) реализует контексты Project Management, Pipeline Execution и Job Logging.

---

## 2. Главные агрегаты

### 2.1. Project

Проект-репозиторий: привязка Git-репозитория к конфигурации CI/CD.

- **Поля:** `id` (UUID v4), `name` (TEXT, unique), `repository_url` (TEXT), `default_branch` (TEXT, default `"main"`), `created_at` (TIMESTAMPTZ).
- **Инварианты:**
  - `name` уникален в рамках инстанса (`UNIQUE CONSTRAINT`).
  - `name` — non-empty.
  - `repository_url` — non-empty, Git URL format (план: валидация в Phase 2).
  - `default_branch` — non-empty, default `"main"`.
- **Связи:** `1 Project → N Pipelines` (`ON DELETE CASCADE`).
- **Операции:** create, list (Phase 0); get, patch, delete (Phase 2).

### 2.2. Pipeline

Запуск пайплайна для Git-рефа. Содержит упорядоченные стадии.

- **Поля:** `id` (UUID v4), `project_id` (UUID FK), `git_ref` (TEXT), `status` (JobStatus), `created_at` (TIMESTAMPTZ), `started_at` (TIMESTAMPTZ?), `finished_at` (TIMESTAMPTZ?).
- **Инварианты:**
  - `status ∈ {queued, running, success, failed, canceled}` (CHECK constraint).
  - `started_at` заполняется при первом переходе в `running`.
  - `finished_at` заполняется при переходе в терминальный статус.
  - `started_at` не может быть позже `finished_at`.
  - `git_ref` — non-empty.
- **Связи:** `N Pipelines → 1 Project` (CASCADE); `1 Pipeline → N Stages` (CASCADE).
- **Агрегация:** статус pipeline вычисляется из статусов всех её stages (раздел 5).

### 2.3. Stage

Упорядоченная стадия пайплайна (build, test, deploy).

- **Поля:** `id` (UUID v4), `pipeline_id` (UUID FK), `name` (TEXT), `position` (INTEGER), `status` (JobStatus).
- **Инварианты:**
  - `position` уникален в рамках pipeline (`UNIQUE(pipeline_id, position)`).
  - `position ≥ 0`.
  - `name` — non-empty.
  - `status ∈ {queued, running, success, failed, canceled}` (CHECK constraint).
- **Связи:** `N Stages → 1 Pipeline` (CASCADE); `1 Stage → N Jobs` (CASCADE).
- **Агрегация:** статус stage вычисляется из статусов всех её jobs (раздел 5).

### 2.4. Job

Задача внутри стадии. Единица выполнения.

- **Поля:** `id` (UUID v4), `stage_id` (UUID FK), `name` (TEXT), `image` (TEXT), `command` (TEXT), `position` (INTEGER), `status` (JobStatus), `started_at` (TIMESTAMPTZ?), `finished_at` (TIMESTAMPTZ?).
- **Инварианты:**
  - `position` уникален в рамках stage (`UNIQUE(stage_id, position)`).
  - `position ≥ 0`.
  - `name`, `image`, `command` — non-empty.
  - `status ∈ {queued, running, success, failed, canceled}` (CHECK constraint).
  - `started_at` заполняется при переходе `queued → running`.
  - `finished_at` заполняется при переходе в терминальный статус.
  - Из терминального статуса переход невозможен.
- **Связи:** `N Jobs → 1 Stage` (CASCADE); `1 Job → N JobLogs` (CASCADE).
- **Transition rules:** `JobStatus::transition_to()` — единственный источник правды (раздел 4).

### 2.5. JobLog

Append-only строка лога выполнения задачи.

- **Поля:** `id` (BIGSERIAL), `job_id` (UUID FK), `sequence` (INTEGER), `message` (TEXT), `created_at` (TIMESTAMPTZ).
- **Инварианты:**
  - `sequence` уникален в рамках job (`UNIQUE(job_id, sequence)`).
  - `sequence ≥ 1`.
  - `message` — non-empty (после trim).
  - Append-only: редактирование и удаление логов не поддерживается.
  - `sequence` вычисляется сервером: `COALESCE(MAX(sequence), 0) + 1`.
- **Связи:** `N JobLogs → 1 Job` (CASCADE).

---

## 3. Value Objects

| VO | Пример | Ограничения |
|---|---|---|
| UUID | `550e8400-e29b-41d4-a716-446655440000` | UUID v4, генерируется сервером |
| GitRef | `main`, `v1.0.0`, `abc123def456` | Non-empty, Git ref format |
| DockerImage | `rust:1.86`, `alpine:3.21` | Non-empty, `image[:tag]` format |
| Timestamp | `2026-08-26T10:05:00Z` | ISO 8601 UTC |

---

## 4. Enum JobStatus и переходы

### 4.1. Определение

`JobStatus` — enum в `backend/src/domain.rs`, единственный источник правды для валидации переходов. Используется для всех трёх сущностей: `Job`, `Stage`, `Pipeline`.

```rust
pub enum JobStatus {
    Queued,
    Running,
    Success,
    Failed,
    Canceled,
}
```

| Variant | serde | Терминальный |
|---|---|---|
| `Queued` | `"queued"` | Нет |
| `Running` | `"running"` | Нет |
| `Success` | `"success"` | Да |
| `Failed` | `"failed"` | Да |
| `Canceled` | `"canceled"` | Да |

### 4.2. Диаграмма переходов

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

### 4.3. Матрица переходов

| From \ To | `queued` | `running` | `success` | `failed` | `canceled` |
|---|---|---|---|---|---|
| `queued` | — | ✅ | — | — | ✅ |
| `running` | — | — | ✅ | ✅ | ✅ |
| `success` | — | — | — | — | — |
| `failed` | — | — | — | — | — |
| `canceled` | — | — | — | — | — |

### 4.4. Побочные эффекты переходов

| Переход | Побочный эффект |
|---|---|
| `queued → running` | `started_at = now()` |
| `queued → canceled` | `finished_at = now()` |
| `running → success` | `finished_at = now()` |
| `running → failed` | `finished_at = now()` |
| `running → canceled` | `finished_at = now()` |

### 4.5. Недопустимые переходы

- `queued → success` — задача не может завершиться успешно без запуска.
- `queued → failed` — задача не может завершиться с ошибкой без запуска.
- `running → queued` — нельзя вернуть в очередь из выполнения.
- Любой переход из терминального статуса — запрещён.

При попытке недопустимого перехода API возвращает `400 Bad Request`:

```json
{
  "error": "invalid status transition from Queued to Success"
}
```

При попытке изменить терминальный статус:

```json
{
  "error": "terminal status cannot change"
}
```

---

## 5. Правила агрегации статусов

### 5.1. Принцип

Статус родительской сущности вычисляется из статусов дочерних. Агрегация выполняется автоматически при каждом изменении статуса job (`refresh_statuses`).

```
Job (1..N) ──aggregate──▶ Stage (1..N) ──aggregate──▶ Pipeline
```

### 5.2. jobs → stage

| Условие | Статус stage |
|---|---|
| Все jobs `queued` | `queued` |
| Хотя бы один `running`, остальные `queued`/`running` | `running` |
| Все `success` | `success` |
| Хотя бы один `failed`, остальные терминальные | `failed` |
| Хотя бы один `canceled`, остальные терминальные (без `failed`) | `canceled` |
| `failed` и `canceled` одновременно | `failed` (приоритет ошибки) |

### 5.3. stages → pipeline

Те же правила, что и jobs → stage.

| Условие | Статус pipeline |
|---|---|
| Все stages `queued` | `queued` |
| Хотя бы одна `running`, остальные `queued`/`running` | `running` |
| Все `success` | `success` |
| Хотя бы одна `failed`, остальные терминальные | `failed` |
| Хотя бы одна `canceled`, остальные терминальные (без `failed`) | `canceled` |
| `failed` и `canceled` одновременно | `failed` (приоритет ошибки) |

### 5.4. Приоритет

`failed` > `canceled` > `running` > `queued` > `success`

### 5.5. Пустые сущности

| Сущность | Условие | Статус |
|---|---|---|
| Stage без jobs | Нет дочерних jobs | `queued` (по умолчанию) |
| Pipeline без stages | Нет дочерних stages | `queued` (по умолчанию) |

### 5.6. Каскадная отмена

При отмене pipeline (`queued → canceled` или `running → canceled`):

1. Все дочерние stages в нетерминальном статусе переводятся в `canceled`.
2. Все дочерние jobs этих stages в нетерминальном статусе переводятся в `canceled`.
3. `finished_at` заполняется на всех уровнях.

При отмене stage аналогично отменяются все её jobs в нетерминальном статусе.

> См. `docs/WORKFLOW.md` для полного описания.

---

## 6. Invariants и бизнес-правила

### 6.1. Переходы статусов

- Переход статуса job возможен только если:
  1. `transition_to()` возвращает `Ok` для пары (from, to).
  2. job не в терминальном статусе.
- После обновления статуса job:
  1. Пересчитывается статус stage (`refresh_statuses`).
  2. Пересчитывается статус pipeline (`refresh_statuses`).
  3. Обновляются `started_at` / `finished_at` на всех уровнях.

### 6.2. Каскадное удаление

- Удаление project → удаление всех pipelines → stages → jobs → job_logs (`ON DELETE CASCADE`).
- Удаление pipeline → удаление всех stages → jobs → job_logs.
- Удаление stage → удаление всех jobs → job_logs.
- Удаление job → удаление всех job_logs.

### 6.3. Идемпотентность миграции

- `store::migrate()` выполняет `CREATE TABLE IF NOT EXISTS` при старте.
- Миграция безопасна для повторного запуска.

### 6.4. Append-only логи

- Логи только добавляются (`POST /jobs/{id}/logs`).
- Редактирование и удаление логов не поддерживается.
- `sequence` вычисляется сервером, клиент не может задать произвольный `sequence`.

---

## 7. Планируемые сущности (Roadmap)

| Фаза | Сущность | Поля | Назначение |
|---|---|---|---|
| Phase 1 | `User` | id, username, email, password_hash, is_admin, created_at | Аутентификация |
| Phase 1 | `Session` | id, user_id, refresh_token, expires_at | JWT refresh |
| Phase 5 | `Runner` | id, name, token_hash, status, last_heartbeat | Runner-агенты |
| Phase 6 | `Webhook` | id, project_id, url, events, secret, created_at | Webhook-уведомления |
| Phase 6 | `WebhookDelivery` | id, webhook_id, event, status, response_code, attempts | Доставка webhooks |
| Phase 7 | `Secret` | id, project_id, key, encrypted_value, created_at | Секреты проектов |
| Phase 8 | `Artifact` | id, job_id, filename, size_bytes, storage_key, created_at | Артефакты сборки |
| Phase 9 | `AuditLog` | id, user_id, action, entity_type, entity_id, created_at | Audit log |
| Phase 9 | `SystemSetting` | key, value, updated_at | Системные настройки |

> См. `docs/DATA_MODEL.md` раздел 8 «Планируемые таблицы».

---

## 8. References

- `docs/ARCHITECTURE.md` — общая архитектура.
- `docs/DATA_MODEL.md` — физическая модель данных (SQL-схема).
- `docs/WORKFLOW.md` — статусная модель и агрегация.
- `docs/API.md` — REST API спецификация.
- `backend/src/domain.rs` — реализация `JobStatus` и transition rules.
- `backend/src/store.rs` — реализация схемы БД и миграции.
- `docs/ROADMAP.md` — план разработки.