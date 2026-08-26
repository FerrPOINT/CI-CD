# System Administration — Forge CI/CD

## 1. Overview

Админ-панель Forge CI/CD — страница `/admin` в Dashboard. Предоставляет системную информацию, а в будущем (Phase 9) — управление пользователями, audit log, системные настройки и метрики.

> **Текущий статус:** страница `/admin` реализована в Phase 0 с отображением системной информации. Управление пользователями, audit log, настройки и метрики — Planned (Phase 9). См. `docs/ROADMAP.md`.

---

## 2. Текущее состояние (Phase 0)

### 2.1. Страница `/admin`

Реализована в `frontend/src/pages/admin/index.tsx`. Доступна без аутентификации (auth — Phase 1). Отображает статичную системную информацию.

### 2.2. Системная информация

| Блок | Поле | Значение | Источник |
|------|------|----------|----------|
| **System** | Version | `0.1.0` | Hardcoded (план: из `CICD_VERSION` env или `Cargo.toml`) |
| | API | `:22801` | Hardcoded (план: из runtime config) |
| | Dashboard | `:22802` | Hardcoded (план: из runtime config) |
| **Database** | Engine | `PostgreSQL 17` | Hardcoded (план: из `pg_database` query) |
| | Port | `:22543` | Hardcoded (план: из runtime config) |

### 2.3. Компоненты UI

- `Card` (shadcn/ui) — два блока: System и Database.
- Иконки: `Settings`, `Server`, `Database` (lucide-react).
- i18n: заголовок через `t('navigation.admin')`.

### 2.4. Что НЕ реализовано

- Управление пользователями (Phase 1 + Phase 9).
- Audit log (Phase 9).
- Системные настройки (Phase 9).
- Метрики и отчёты (Phase 9).
- Аутентификация и RBAC (Phase 1) — страница доступна без login.
- Динамическая информация из API (план: `GET /api/v1/admin/system`).

---

## 3. Плановое: Phase 9 — Admin + Reports

### 3.1. Цель

Полноценная админ-панель с управлением пользователями, audit log, системными настройками и метриками CI/CD.

### 3.2. Структура страницы

```
/admin
├── Overview      — системная информация (текущий блок, расширенный)
├── Users         — управление пользователями
├── Settings      — системные настройки
├── Audit Log     — журнал действий администратора
└── Reports       — CI/CD метрики и графики
```

### 3.3. Tabs

| Tab | Route | Назначение | Статус |
|-----|-------|-----------|--------|
| Overview | `/admin` | Системная информация | ✅ Phase 0 (базовая) |
| Users | `/admin/users` | Управление пользователями | Planned Phase 9 |
| Settings | `/admin/settings` | Системные настройки | Planned Phase 9 |
| Audit Log | `/admin/audit-log` | Журнал аудита | Planned Phase 9 |
| Reports | `/admin/reports` | Метрики и графики | Planned Phase 9 |

---

## 4. Управление пользователями (Phase 9)

### 4.1. Возможности

- Список пользователей (`GET /api/v1/admin/users`).
- Создание пользователя (`POST /api/v1/admin/users`).
- Блокировка / активация (`PATCH /api/v1/admin/users/{id}` с `is_active`).
- Удаление пользователя (`DELETE /api/v1/admin/users/{id}`).
- Назначение глобальной роли (`PATCH /api/v1/admin/users/{id}` с `role`).

### 4.2. Поля пользователя (Phase 1+)

| Поле | Тип | Описание |
|------|-----|----------|
| `id` | UUID v4 | PK |
| `username` | TEXT | Уникальный логин |
| `email` | TEXT | Email |
| `display_name` | TEXT | Отображаемое имя |
| `is_admin` | BOOLEAN | Глобальный админ |
| `is_active` | BOOLEAN | Активен / заблокирован |
| `created_at` | TIMESTAMPTZ | Дата создания |
| `last_login_at` | TIMESTAMPTZ? | Последний вход |

### 4.3. Доступ

- Только пользователи с ролью **Админ** (global `is_admin = true`).
- RBAC: `administer_users` permission.

> См. `docs/TZ.md` раздел 2, `docs/SECURITY.md`.

---

## 5. Audit Log (Phase 9)

### 5.1. Назначение

Журнал всех административных действий: создание/удаление пользователей, изменение системных настроек, удаление проектов.

### 5.2. Модель данных

```sql
CREATE TABLE IF NOT EXISTS audit_log (
    id          UUID PRIMARY KEY,
    user_id     UUID REFERENCES users(id) ON DELETE SET NULL,
    action      TEXT NOT NULL,
    entity_type TEXT NOT NULL,
    entity_id   UUID,
    metadata    JSONB,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

| Поле | Тип | Описание |
|------|-----|----------|
| `id` | UUID | PK |
| `user_id` | UUID | Кто выполнил действие |
| `action` | TEXT | Тип действия (`user.create`, `project.delete`, `settings.update`) |
| `entity_type` | TEXT | Тип сущности (`user`, `project`, `system_setting`) |
| `entity_id` | UUID? | ID сущности (если применимо) |
| `metadata` | JSONB | Дополнительные данные (old/new values) |
| `created_at` | TIMESTAMPTZ | Время действия |

### 5.3. API

| Метод | Путь | Назначение |
|-------|------|-----------|
| `GET` | `/api/v1/admin/audit-log` | Список записей (с фильтрами) |

Фильтры:

| Параметр | Тип | Описание |
|---|---|---|
| `user_id` | UUID | Фильтр по пользователю |
| `action` | TEXT | Фильтр по типу действия |
| `entity_type` | TEXT | Фильтр по типу сущности |
| `from` | ISO 8601 | Начало периода |
| `to` | ISO 8601 | Конец периода |
| `page` | integer | Пагинация |
| `size` | integer | Размер страницы |

### 5.4. Свойства

- Записи audit log **не удаляются** (immutable).
- Доступ — только **Админ**.
- Фильтрация по пользователю, типу действия, дате.

---

## 6. Системные настройки (Phase 9)

### 6.1. Модель данных

```sql
CREATE TABLE IF NOT EXISTS system_settings (
    key         TEXT PRIMARY KEY,
    value       TEXT NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by  UUID REFERENCES users(id) ON DELETE SET NULL
);
```

### 6.2. Настройки (план)

| Ключ | Default | Описание |
|------|---------|----------|
| `instance_name` | `Forge CI/CD` | Название инстанса |
| `default_branch` | `main` | Ветка по умолчанию для новых проектов |
| `max_pipeline_duration` | `3600` | Максимальная длительность пайплайна (сек) |
| `artifact_max_size_mb` | `100` | Максимальный размер артефакта (МБ) |
| `artifact_ttl_days` | `30` | TTL артефактов (дней) |
| `log_max_length` | `10000` | Максимальная длина строки лога |
| `webhook_max_retries` | `3` | Максимальное количество retry для webhook delivery |

### 6.3. API

| Метод | Путь | Назначение |
|-------|------|-----------|
| `GET` | `/api/v1/admin/settings` | Список всех настроек |
| `PATCH` | `/api/v1/admin/settings/{key}` | Обновление настройки |

### 6.4. Доступ

- Чтение — **Админ**.
- Запись — **Админ**.
- Каждое изменение записывается в audit log.

---

## 7. Метрики и отчёты (Phase 9)

### 7.1. Метрики

| Метрика | Описание | SQL (кратко) |
|---------|----------|---------------|
| Pipeline Success Rate | Доля успешных пайплайнов | `count(status='success') / count(terminal) * 100` |
| Average Duration | Среднее время выполнения | `avg(finished_at - started_at)` |
| Duration Percentiles | p50, p90, p95, p99 | `percentile_cont(...) WITHIN GROUP (...)` |
| Deployment Frequency | Количество успешных deploy stages | `count(stage='deploy' AND status='success')` |
| Failure Trends | Количество failed по дням | `GROUP BY date(finished_at)` |

> См. `docs/REPORTS.md` для полного SQL.

### 7.2. Frontend

- Charts: `recharts` для визуализации.
- Графики: success rate (line), duration histogram (bar), failure trends (line).
- Фильтры: проект, период (7/30/90 дней).
- API: `GET /api/v1/admin/reports/success-rate?from=...&to=...`.

### 7.3. Prometheus metrics (Future)

- `GET /metrics` — Prometheus format.
- Метрики: `cicd_pipelines_total`, `cicd_pipeline_duration_seconds`, `cicd_jobs_total`, `cicd_job_duration_seconds`.

> См. `docs/MONITORING.md`, `docs/PERFORMANCE.md`.

---

## 8. Системная информация (расширенная, Phase 9)

### 8.1. План расширения

| Блок | Поле | Источник |
|------|------|----------|
| **System** | Version | `CICD_VERSION` env или `Cargo.toml` |
| | API port | Runtime config (`CICD_API_PORT`) |
| | Dashboard port | Runtime config (`CICD_WEB_PORT`) |
| | Uptime | Process start time |
| **Database** | Engine | `SELECT version()` |
| | Port | `CICD_DATABASE_PORT` |
| | Pool size | `PgPool` size + idle connections |
| | DB size | `SELECT pg_database_size('cicd')` |
| | Table count | `SELECT count(*) FROM information_schema.tables` |
| **Counts** | Projects | `SELECT count(*) FROM projects` |
| | Pipelines | `SELECT count(*) FROM pipelines` |
| | Jobs | `SELECT count(*) FROM jobs` |
| | Job logs | `SELECT count(*) FROM job_logs` |

### 8.2. API (план)

```
GET /api/v1/admin/system
```

**Response 200:**
```json
{
  "version": "0.2.0",
  "ports": { "api": 22801, "dashboard": 22802, "database": 22543 },
  "uptime_seconds": 86400,
  "database": {
    "engine": "PostgreSQL 17.6",
    "pool_size": 10,
    "pool_idle": 5,
    "db_size_bytes": 10485760
  },
  "counts": {
    "projects": 5,
    "pipelines": 142,
    "jobs": 426,
    "job_logs": 15234
  }
}
```

---

## 9. Доступ и безопасность

### 9.1. Текущий (Phase 0)

- Страница `/admin` доступна без аутентификации.
- Отображается статичная информация — нет риска утечки данных.

### 9.2. Плановое (Phase 1+)

- Страница `/admin` требует аутентификацию (JWT).
- Доступ — только роль **Админ** (`is_admin = true`).
- `RequireAuth` wrapper + `RequireAdmin` check в router.
- Все admin-действия записываются в audit log.
- API endpoints `/api/v1/admin/*` защищены middleware.

> См. `docs/SECURITY.md`, `docs/ROUTING.md`.

---

## 10. CLI (план)

```bash
# Системная информация
cicd-cli admin system

# Управление пользователями (Phase 9)
cicd-cli admin list-users
cicd-cli admin create-user --email --username --display-name --password [--is-admin]
cicd-cli admin toggle-user <uuid> --active true|false

# Audit log (Phase 9)
cicd-cli admin audit-log [--limit 50 --user <uuid> --action project.delete]

# Системные настройки (Phase 9)
cicd-cli admin settings
cicd-cli admin set-setting --key default_branch --value develop
```

> См. `docs/CLI.md`, `docs/API.md`.

---

## 11. References

- `docs/ARCHITECTURE.md` — архитектура приложения.
- `docs/ROADMAP.md` — Phase 9: Admin + Reports.
- `docs/ROUTING.md` — роутинг frontend (`/admin`).
- `docs/FRONTEND_ARCHITECTURE.md` — структура frontend.
- `docs/SECURITY.md` — безопасность и RBAC.
- `docs/REPORTS.md` — метрики и отчёты.
- `docs/MONITORING.md` — мониторинг.
- `docs/TZ.md` — техническое задание (роли, admin-функции).
- `frontend/src/pages/admin/index.tsx` — реализация страницы.