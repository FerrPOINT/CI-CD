# Project Administration — Forge CI/CD

## 1. Overview

Управление проектами-репозиториями в Forge CI/CD. Проект — корневая сущность, привязанная к Git-репозиторию. Через проект запускаются пайплайны, управляются стадии, задачи, логи, а в будущем — webhooks, secrets и artifacts.

> **Текущий статус:** реализованы `GET /projects` (list) и `POST /projects` (create) в Phase 0. `GET/PATCH/DELETE /projects/{id}` запланированы в Phase 2. См. `docs/ROADMAP.md`, `docs/API.md`.

---

## 2. Атрибуты проекта

| Поле | Тип | Required | Default | Описание |
|------|-----|----------|---------|----------|
| `id` | UUID v4 | server | — | Первичный ключ, генерируется `Uuid::new_v4()` |
| `name` | TEXT | yes | — | Уникальное имя проекта (non-empty) |
| `repository_url` | TEXT | yes | — | URL Git-репозитория |
| `default_branch` | TEXT | no | `"main"` | Ветка по умолчанию |
| `created_at` | TIMESTAMPTZ | server | `now()` | Время создания |

### 2.1. Валидация

- `name` — non-empty, unique. Duplicate → 500 (unique constraint violation).
- `repository_url` — non-empty. Планируется валидация Git URL format в Phase 2.
- `default_branch` — optional, default `"main"`.

---

## 3. Операции

### 3.1. Создание проекта

```
POST /api/v1/projects
Content-Type: application/json
```

**Request body:**
```json
{
  "name": "my-service",
  "repository_url": "git@github.com:org/my-service.git",
  "default_branch": "main"
}
```

**Response 200:**
```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "name": "my-service",
  "repository_url": "git@github.com:org/my-service.git",
  "default_branch": "main",
  "created_at": "2026-08-26T10:00:00Z"
}
```

**Ошибки:**
- `400` — `name` или `repository_url` пустые.
- `500` — duplicate name (unique constraint) или ошибка БД.

**curl:**
```bash
curl -sS -X POST http://127.0.0.1:22801/api/v1/projects \
  -H 'content-type: application/json' \
  -d '{"name":"my-service","repository_url":"git@github.com:org/my-service.git"}'
```

### 3.2. Список проектов

```
GET /api/v1/projects
```

Возвращает список всех проектов, отсортированных по `created_at DESC`.

**Response 200:**
```json
[
  {
    "id": "550e8400-...",
    "name": "my-service",
    "repository_url": "git@github.com:org/my-service.git",
    "default_branch": "main",
    "created_at": "2026-08-26T10:00:00Z"
  }
]
```

**curl:**
```bash
curl -sS http://127.0.0.1:22801/api/v1/projects
```

### 3.3. Детали проекта (Planned Phase 2)

```
GET /api/v1/projects/{id}
```

Возвращает один проект по UUID.

**Ошибки:**
- `404` — проект не найден.
- `503` — БД недоступна.

### 3.4. Редактирование проекта (Planned Phase 2)

```
PATCH /api/v1/projects/{id}
Content-Type: application/json
```

**Request body (partial update):**
```json
{
  "name": "my-service-renamed",
  "repository_url": "git@github.com:org/my-service-v2.git",
  "default_branch": "develop"
}
```

- Все поля optional: передаются только изменяемые.
- `name` — unique; при duplicate → `409 Conflict` (план) или `500` (текущий behaviour unique constraint).
- `repository_url` — non-empty, Git URL format (план: валидация).

**Response 200:** обновлённый `Project`.

**Ошибки:**
- `400` — невалидные данные.
- `404` — проект не найден.
- `409` — duplicate `name` (план).
- `500` — ошибка БД.

**curl:**
```bash
curl -sS -X PATCH http://127.0.0.1:22801/api/v1/projects/$PROJECT_ID \
  -H 'content-type: application/json' \
  -d '{"default_branch":"develop"}'
```

### 3.5. Удаление проекта (Planned Phase 2)

```
DELETE /api/v1/projects/{id}
```

Удаляет проект и **каскадно** все дочерние сущности:

```
project ──CASCADE──▶ pipelines ──CASCADE──▶ stages ──CASCADE──▶ jobs ──CASCADE──▶ job_logs
```

- `ON DELETE CASCADE` на всех FK.
- Удаление необратимо (soft-delete / архивация — Future).
- Все пайплайны, стадии, задачи и логи проекта удаляются автоматически.

**Response:** `204 No Content` (план) или `200 OK` с пустым body.

**Ошибки:**
- `404` — проект не найден.
- `503` — БД недоступна.
- `500` — ошибка БД.

**curl:**
```bash
curl -sS -X DELETE http://127.0.0.1:22801/api/v1/projects/$PROJECT_ID
```

### 3.6. Pagination (Planned Phase 2)

```
GET /api/v1/projects?page=0&size=20
```

| Параметр | Тип | Default | Описание |
|---|---|---|---|
| `page` | integer | 0 | Номер страницы (0-indexed) |
| `size` | integer | 20 | Размер страницы (max 100) |

> См. `docs/PAGINATION.md`.

---

## 4. Настройки ветки по умолчанию

`default_branch` — ветка Git, используемая по умолчанию при запуске пайплайна:

- `POST /projects/{id}/pipelines` без `git_ref` → используется `default_branch`.
- Изменяется через `PATCH /projects/{id}` с полем `default_branch`.
- Default: `"main"`.

### 4.1. Пример

```bash
# Создать проект с develop как default branch
curl -sS -X POST http://127.0.0.1:22801/api/v1/projects \
  -H 'content-type: application/json' \
  -d '{"name":"my-service","repository_url":"git@...","default_branch":"develop"}'

# Изменить default branch
curl -sS -X PATCH http://127.0.0.1:22801/api/v1/projects/$PROJECT_ID \
  -H 'content-type: application/json' \
  -d '{"default_branch":"main"}'

# Запустить пайплайн для default branch (без git_ref)
curl -sS -X POST http://127.0.0.1:22801/api/v1/projects/$PROJECT_ID/pipelines \
  -H 'content-type: application/json' \
  -d '{}'
```

---

## 5. Frontend

### 5.1. Список проектов (`/projects`)

- Card grid с проектами.
- Форма создания (dialog): `name`, `repository_url`, `default_branch`.
- Использует `useProjects()` и `useCreateProject()` из `frontend/src/api/hooks.ts`.

### 5.2. Редактирование (Planned Phase 2)

- Форма редактирования (dialog) с pre-filled значениями.
- `PATCH /projects/{id}` через `useUpdateProject()` hook (план).

### 5.3. Удаление (Planned Phase 2)

- Confirmation dialog (`AlertDialog`): «Удалить проект? Все пайплайны и логи будут удалены безвозвратно.»
- `DELETE /projects/{id}` через `useDeleteProject()` hook (план).
- Инвалидация `['projects']` query key после удаления.

### 5.4. TanStack Query hooks

```ts
// Текущие (Phase 0)
useProjects()           // GET /projects
useCreateProject()      // POST /projects

// Плановые (Phase 2)
useProject(id)          // GET /projects/{id}
useUpdateProject(id)    // PATCH /projects/{id}
useDeleteProject(id)    // DELETE /projects/{id}
```

> См. `docs/FRONTEND_ARCHITECTURE.md`, `docs/UI_UX.md`.

---

## 6. Плановое: участники и роли (Future)

### 6.1. Участники проекта

- `project_members` таблица: `project_id`, `user_id`, `role`, `created_at`.
- Endpoint: `GET/POST/DELETE /projects/{id}/members`.
- Управление участниками — в Project Settings (UI).

### 6.2. Роли в проекте

| Роль | Права |
|------|-------|
| **Project Admin** | Полное управление проектом, участниками, настройками |
| **Developer** | Запуск пайплайнов, управление статусами задач, логи |
| **Viewer** | Только просмотр |

### 6.3. Permissions matrix (целевая)

| Permission | Admin | Developer | Viewer |
|-----------|-------|-----------|--------|
| Edit project settings | ✅ | ❌ | ❌ |
| Delete project | ✅ | ❌ | ❌ |
| Manage members | ✅ | ❌ | ❌ |
| Trigger pipeline | ✅ | ✅ | ❌ |
| Transition job status | ✅ | ✅ | ❌ |
| Append job log | ✅ | ✅ | ❌ |
| View project | ✅ | ✅ | ✅ |

> RBAC — Future (после Phase 1 Auth). См. `docs/ROADMAP.md`, `docs/SECURITY.md`, `docs/TZ.md` раздел 2.

---

## 7. CLI

```bash
# Создать проект
cicd-cli project create --name "my-service" --repository-url "git@..." --branch main

# Список проектов
cicd-cli project list

# Плановое (Phase 2)
cicd-cli project get <uuid>
cicd-cli project update <uuid> --name "new-name" --branch develop
cicd-cli project delete <uuid>
```

> См. `docs/CLI.md`, `docs/API.md`.

---

## 8. Безопасность

- Все секреты — через env vars с префиксом `CICD_`.
- `repository_url` не должен содержать embedded credentials (план: sanitization в Phase 2).
- После внедрения auth (Phase 1): создание/редактирование/удаление проектов требует роль Developer / Admin.
- Audit log для операций с проектами (Phase 9).

> См. `docs/SECURITY.md`.

---

## 9. References

- `docs/API.md` — REST API спецификация (раздел Projects).
- `docs/DATA_MODEL.md` — таблица `projects`.
- `docs/DOMAIN_MODEL.md` — агрегат Project.
- `docs/FRONTEND_ARCHITECTURE.md` — TanStack Query hooks.
- `docs/ROADMAP.md` — Phase 2: Projects.
- `docs/SECURITY.md` — безопасность.
- `docs/PAGINATION.md` — пагинация (план).