# Glossary — Термины CI/CD Forge CI/CD

## 1. Обзор

Глоссарий терминов, используемых в документации и коде Forge CI/CD. Термины на английском сохраняются в коде и API; описания — на русском.

---

## 2. Термины

### Pipeline

**Пайплайн.** Единица выполнения CI/CD — запуск конвейера для конкретного Git-рефа. Содержит упорядоченный набор стадий (`stages`). Имеет статус, timestamps (`created_at`, `started_at`, `finished_at`) и привязку к проекту.

```
Pipeline → Stage 1 (build) → Stage 2 (test) → Stage 3 (deploy)
```

В БД: таблица `pipelines`. Статусы: `queued`, `running`, `success`, `failed`, `canceled`.

---

### Stage

**Стадия.** Упорядоченный шаг пайплайна (e.g. `build`, `test`, `deploy`). Содержит набор задач (`jobs`). Позиция `position` уникальна в рамках пайплайна. Статус агрегируется из статусов дочерних jobs.

В БД: таблица `stages`. Уникальный constraint: `(pipeline_id, position)`.

---

### Job

**Задача.** Единица работы внутри стадии. Содержит Docker-образ (`image`) и команду (`command`). Выполняется на runner'е. Имеет append-only логи (`job_logs`). Статус изменяется через `transition_to()`.

В БД: таблица `jobs`. Уникальный constraint: `(stage_id, position)`.

---

### Runner

**Раннер.** Агент, выполняющий задачи (jobs) в изолированных контейнерах. Регистрируется на control plane через token. Забирает задачи из очереди (`SELECT FOR UPDATE SKIP LOCKED`), запрашивает Docker-образ, выполняет команду, стримит stdout/stderr в логи, сообщает статус.

> Planned: Phase 5 (Real Runner). В MVP задачи переводятся вручную.

---

### Artifact

**Артефакт.** Файл, произведённый задачей (бинарник, архив, отчёт, coverage). Загружается через API, хранится в storage backend (local FS или S3-compatible), имеет TTL для автоматической очистки.

> Planned: Phase 8 (Artifacts).

---

### Webhook

**Вебхук.** HTTP-вызов между системами.

- **Incoming webhook** — Git-провайдер (GitHub/GitLab/Gitea) вызывает Forge CI/CD при push/PR/tag. Forge проверяет подпись и запускает пайплайн.
- **Outgoing webhook** — Forge CI/CD вызывает внешний URL при смене статуса пайплайна/job. Payload подписывается HMAC-SHA256.

> Planned: Phase 6 (Webhooks).

---

### Ref

**Git-реф.** Ссылка на состояние Git-репозитория: ветка (`refs/heads/main`), тег (`refs/tags/v1.0`) или commit SHA (`abc123`). Пайплайн запускается для конкретного рефа. Хранится в `pipelines.git_ref`.

---

### Deployment

**Деплой.** Процесс развёртывания приложения в целевое окружение. В Forge CI/CD — stage с именем `deploy`, успешно завершённая. Метрика deployment frequency = количество успешных deploy-stages за период.

---

### Build

**Сборка.** Stage `build` — компиляция исходного кода в артефакт (бинарник, Docker-образ, архив). Первая стадия в типичном пайплайне.

---

### Test

**Тестирование.** Stage `test` — запуск unit/integration/e2e тестов. Вторая стадия в типичном пайплайне. Включает `cargo test` (backend), `pnpm test` (frontend), lint checks.

---

### Deploy

**Развёртывание.** Stage `deploy` — доставка артефакта в целевое окружение (dev/staging/prod). Третья стадия в типичном пайплайне. Может требовать ручного подтверждения (future: approval gates).

---

### Status

**Статус.** Текущее состояние сущности (pipeline/stage/job). Enum из 5 значений:

| Статус | Описание |
|---|---|
| `queued` | Создан, ожидает выполнения |
| `running` | Выполняется |
| `success` | Завершён успешно |
| `failed` | Завершён с ошибкой |
| `canceled` | Отменён |

Хранится в БД как TEXT с CHECK constraint. В коде — `JobStatus` enum.

---

### Transition

**Переход.** Смена статуса сущности. Валидируется методом `JobStatus::transition_to()`. Не все переходы допустимы (см. матрицу в `docs/WORKFLOW.md`). Недопустимый переход → 409 Conflict.

---

### Terminal State

**Терминальное состояние.** Статус, из которого невозможны дальнейшие переходы: `success`, `failed`, `canceled`. При переходе в терминальное состояние заполняется `finished_at`. Допускается только чтение и удаление.

---

### Control Plane

**Плоскость управления.** Центральный компонент CI/CD системы. Управляет проектами, пайплайнами, статусами, логами. Не выполняет задачи напрямую — делегирует runner'ам. Forge CI/CD — это control plane, а не remote-execution system.

---

### Agent

**Агент.** Синоним runner'а в контексте системы. Процесс, подключающийся к control plane, забирающий задачи из очереди и выполняющий их в изолированных контейнерах. Передаёт логи и статусы обратно в control plane.

> В текущей архитектуре (MVP) агентов нет — задачи переводятся вручную через UI/API/CLI. Planned: Phase 5.

---

## 3. Сводная таблица

| Термин | Сущность в БД | Уровень | Плановая фаза |
|---|---|---|---|
| Pipeline | `pipelines` | Верхний | Phase 0 (done) |
| Stage | `stages` | Средний | Phase 0 (done) |
| Job | `jobs` | Нижний | Phase 0 (done) |
| Runner | `runners` (plan) | Infrastructure | Phase 5 |
| Artifact | `artifacts` (plan) | Storage | Phase 8 |
| Webhook | `webhooks` (plan) | Integration | Phase 6 |
| Secret | `secrets` (plan) | Security | Phase 7 |
| Notification | `notifications` (plan) | Integration | Phase 6 |
| Report | SQL queries | Analytics | Phase 9 |

---

## 4. Иерархия сущностей

```
Project
  └── Pipeline (1 project → N pipelines)
        └── Stage (1 pipeline → N stages, упорядочены по position)
              └── Job (1 stage → N jobs, упорядочены по position)
                    └── JobLog (1 job → N log lines, append-only)
                    └── Artifact (1 job → N artifacts, planned)
```

---

## 5. Связанные термины

| Термин | Связан с | Описание связи |
|---|---|---|
| Pipeline | Project | Pipeline принадлежит проекту |
| Stage | Pipeline | Stage принадлежит пайплайну |
| Job | Stage | Job принадлежит стадии |
| JobLog | Job | Логи принадлежат задаче (append-only) |
| Runner | Job | Runner выполняет job |
| Artifact | Job | Artifact загружается после выполнения job |
| Webhook | Project | Webhook привязан к проекту |
| Secret | Project | Secret привязан к проекту |
| Deployment | Pipeline | Deployment = stage `deploy` в pipeline |

---

## References

- `docs/WORKFLOW.md` — статусы и переходы
- `docs/DATA_MODEL.md` — таблицы и связи
- `docs/ARCHITECTURE.md` — архитектура системы
- `docs/ROADMAP.md` — фазы разработки