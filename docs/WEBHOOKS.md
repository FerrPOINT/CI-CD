# Webhooks — Forge CI/CD

## 1. Обзор

План Phase 6: входящие webhooks от Git-провайдеров (GitHub, GitLab, Gitea) для автоматического запуска пайплайнов, и исходящие webhooks для уведомления внешних систем о смене статуса.

> **Статус:** MVP реализован (хранение конфигурации webhook-ов + UI). Не реализовано: доставка событий, подпись HMAC, delivery history. См. `docs/ROADMAP.md` Phase 6.

---

## 2. Входящие webhooks (Incoming)

### 2.1. Назначение

Git-провайдер (GitHub/GitLab/Gitea) отправляет webhook при push, pull request, tag. Forge CI/CD принимает webhook, определяет проект по URL репозитория, запускает пайплайн для соответствующего Git-рефа.

### 2.2. Endpoint

```
POST /api/v1/webhooks/incoming
Content-Type: application/json
X-GitHub-Event: push
X-Hub-Signature-256: sha256=...
X-GitLab-Event: Push Hook
X-Gitlab-Token: ...
```

### 2.3. Поддерживаемые провайдеры

| Провайдер | Заголовок события | Заголовок подписи | Формат payload |
|---|---|---|---|
| GitHub | `X-GitHub-Event` | `X-Hub-Signature-256` | GitHub Webhook |
| GitLab | `X-Gitlab-Event` | `X-Gitlab-Token` (plain) | GitLab Webhook |
| Gitea | `X-Gitea-Event` | `X-Gitea-Signature` | Gitea Webhook |
| Generic | `X-Webhook-Event` | `X-Webhook-Signature` | Произвольный |

### 2.4. Поддерживаемые события

| Событие | GitHub | GitLab | Gitea | Действие |
|---|---|---|---|---|
| Push | `push` | `Push Hook` | `push` | Запуск пайплайна для ветки |
| Pull Request | `pull_request` | `Merge Request Hook` | `pull_request` | Запуск пайплайна для PR |
| Tag | `create` (ref_type=tag) | `Tag Push Hook` | `create` | Запуск пайплайна для тега |

### 2.5. Логика обработки

```
1. Запрос поступает на POST /api/v1/webhooks/incoming
2. Определяется провайдер по заголовкам
3. Проверяется подпись (HMAC-SHA256 для GitHub/Gitea, plain token для GitLab)
4. Парсится payload, извлекаются:
   - repository_url
   - git_ref (ветка/тег/SHA)
   - event_type
5. По repository_url находится проект в БД
6. Если проект найден и webhook включён — создаётся pipeline (status=queued)
7. Возвращается 200 OK с pipeline_id
```

### 2.6. Маппинг payload → pipeline

```json
// GitHub push event → Forge pipeline
{
  "repository": {
    "clone_url": "git@github.com:org/my-service.git"
  },
  "ref": "refs/heads/main",
  "after": "abc123def456"
}
```

Маппинг:

| Поле webhook | Поле pipeline |
|---|---|
| `repository.clone_url` | Поиск проекта по `repository_url` |
| `ref` → `refs/heads/main` | `git_ref = "main"` |
| `ref` → `refs/tags/v1.0` | `git_ref = "v1.0"` |
| `after` | Сохраняется в metadata (future) |

### 2.7. Настройка webhook в Git-провайдере

**GitHub:**
- URL: `https://cicd.example.com/api/v1/webhooks/incoming`
- Content type: `application/json`
- Secret: `CICD_WEBHOOK_SECRET_<project>` (см. раздел 4)
- Events: `push`, `pull_request`, `create`

**GitLab:**
- URL: `https://cicd.example.com/api/v1/webhooks/incoming`
- Secret token: `CICD_WEBHOOK_SECRET_<project>`
- Triggers: `Push events`, `Merge request events`, `Tag push events`

---

## 3. Исходящие webhooks (Outgoing)

### 3.1. Назначение

Forge CI/CD отправляет HTTP POST на внешние URL при смене статуса пайплайнов, стадий или задач. Внешние системы (Slack, Discord, custom integrations) получают уведомления.

### 3.2. События

| Событие | Когда |
|---|---|
| `pipeline.started` | Pipeline → running |
| `pipeline.success` | Pipeline → success |
| `pipeline.failed` | Pipeline → failed |
| `pipeline.canceled` | Pipeline → canceled |
| `job.started` | Job → running |
| `job.finished` | Job → success/failed |
| `job.failed` | Job → failed |

### 3.3. Payload

```json
{
  "event": "pipeline.failed",
  "timestamp": "2026-08-26T12:34:56Z",
  "data": {
    "pipelineId": "550e8400-e29b-41d4-a716-446655440000",
    "projectId": "...",
    "projectName": "my-service",
    "gitRef": "main",
    "status": "failed",
    "startedAt": "2026-08-26T12:34:00Z",
    "finishedAt": "2026-08-26T12:34:56Z",
    "durationSecs": 56,
    "failedStage": "build",
    "failedJob": "compile"
  },
  "signature": "sha256=a1b2c3d4e5f6..."
}
```

### 3.4. Подпись

**Алгоритм:** HMAC-SHA256.

**Ключ:** `webhook.secret` (уникальный для каждого webhook-URL).

**Подписываемое содержимое:** raw JSON body (serialized payload).

**Заголовок:**

```
X-Forge-Signature: sha256=<hex_hmac>
X-Forge-Event: pipeline.failed
X-Forge-Delivery: <uuid>
```

**Верификация на стороне получателя:**

```python
import hmac, hashlib

expected = hmac.new(
    secret.encode(),
    request_body,
    hashlib.sha256
).hexdigest()

if not hmac.compare_digest(f"sha256={expected}", request.headers["X-Forge-Signature"]):
    return 401
```

### 3.5. Регистрация webhook

```bash
POST /api/v1/projects/{id}/webhooks
Content-Type: application/json

{
  "url": "https://hooks.slack.com/services/...",
  "events": ["pipeline.failed", "pipeline.success"],
  "secret": "my-webhook-secret"
}
```

### 3.6. Доставка

| Характеристика | Значение |
|---|---|
| Метод | POST |
| Content-Type | `application/json` |
| Timeout | 10 секунд |
| Retry | 3 попытки, exponential backoff (1s, 4s, 16s) |
| HTTP success | 2xx (200–299) |
| HTTP failure | 3xx/4xx/5xx → retry |
| Content | JSON payload + подпись |

### 3.7. Таблица доставок

```sql
CREATE TABLE IF NOT EXISTS webhook_deliveries (
    id              UUID PRIMARY KEY,
    webhook_id      UUID NOT NULL REFERENCES webhooks(id) ON DELETE CASCADE,
    event_type      TEXT NOT NULL,
    payload         JSONB NOT NULL,
    response_status INTEGER,
    response_body   TEXT,
    attempt         INTEGER NOT NULL DEFAULT 1,
    status          TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','success','failed','retrying')),
    delivered_at    TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

**API доставки:**

| Метод | Путь | Назначение |
|---|---|---|
| `GET` | `/api/v1/projects/{id}/webhooks/{wid}/deliveries` | История доставок |
| `POST` | `/api/v1/projects/{id}/webhooks/{wid}/deliveries/{did}/redeliver` | Повторная доставка |

---

## 4. Верификация подписи (входящие webhooks)

### 4.1. GitHub (HMAC-SHA256)

```rust
fn verify_github_signature(secret: &str, body: &[u8], signature: &str) -> bool {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(body);
    let expected = format!("sha256={}", hex::encode(mac.finalize().into_bytes()));
    constant_time_eq(expected.as_bytes(), signature.as_bytes())
}
```

Заголовок: `X-Hub-Signature-256: sha256=<hex>`.

### 4.2. GitLab (plain token)

GitLab отправляет plain token в заголовке `X-Gitlab-Token`. Сравнение с `CICD_WEBHOOK_SECRET_<project>`:

```rust
fn verify_gitlab_token(expected: &str, token: &str) -> bool {
    constant_time_eq(expected.as_bytes(), token.as_bytes())
}
```

### 4.3. Gitea (HMAC-SHA256)

Аналогично GitHub. Заголовок: `X-Gitea-Signature: <hex>` (без префикса `sha256=`).

### 4.4. Generic (HMAC-SHA256)

Для custom интеграций. Заголовок: `X-Webhook-Signature: sha256=<hex>`.

---

## 5. Дата-модель (план)

### 5.1. Таблица webhooks

```sql
CREATE TABLE IF NOT EXISTS webhooks (
    id          UUID PRIMARY KEY,
    project_id  UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    url         TEXT NOT NULL,
    events      TEXT[] NOT NULL DEFAULT '{}',
    secret      TEXT NOT NULL,
    active      BOOLEAN NOT NULL DEFAULT true,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

### 5.2. Таблица webhook_sources (входящие)

```sql
CREATE TABLE IF NOT EXISTS webhook_sources (
    id          UUID PRIMARY KEY,
    project_id  UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    provider    TEXT NOT NULL CHECK (provider IN ('github','gitlab','gitea','generic')),
    secret      TEXT NOT NULL,
    events      TEXT[] NOT NULL DEFAULT '{}',
    active      BOOLEAN NOT NULL DEFAULT true,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

---

## 6. API (план)

### 6.1. Исходящие webhooks

| Метод | Путь | Назначение |
|---|---|---|
| `GET` | `/api/v1/projects/{id}/webhooks` | Список webhooks проекта |
| `POST` | `/api/v1/projects/{id}/webhooks` | Регистрация webhook |
| `PATCH` | `/api/v1/projects/{id}/webhooks/{wid}` | Обновление webhook |
| `DELETE` | `/api/v1/projects/{id}/webhooks/{wid}` | Удаление webhook |
| `POST` | `/api/v1/projects/{id}/webhooks/{wid}/test` | Тестовая отправка |
| `GET` | `/api/v1/projects/{id}/webhooks/{wid}/deliveries` | История доставок |

### 6.2. Входящие webhooks

| Метод | Путь | Назначение |
|---|---|---|
| `POST` | `/api/v1/webhooks/incoming` | Приём webhook от Git-провайдера |
| `GET` | `/api/v1/projects/{id}/webhook-sources` | Список источников |
| `POST` | `/api/v1/projects/{id}/webhook-sources` | Регистрация источника |
| `DELETE` | `/api/v1/projects/{id}/webhook-sources/{sid}` | Удаление источника |

---

## 7. Безопасность

- Webhook secret хранится в зашифрованном виде (см. `docs/SECRETS_MGMT.md`).
- Верификация подписи обязательна — запрос без валидной подписи отклоняется (401).
- URL webhook'а валидируется (должен быть HTTPS в production).
- Rate limiting: не более 100 webhook-доставок в минуту на URL.
- IP allowlist для входящих webhooks (опционально, по диапазонам GitHub/GitLab).

---

## 8. Frontend (план)

- Вкладка "Webhooks" в настройках проекта.
- Список зарегистрированных webhooks с переключателем active/inactive.
- Форма регистрации: URL, события (multi-select), secret.
- История доставок с response status, attempts, timestamp.
- Кнопка "Redeliver" для повторной отправки.
- Кнопка "Test" для отправки тестового payload.

---

## 9. Env-переменные (план)

| Переменная | Default | Описание |
|---|---|---|
| `CICD_WEBHOOKS_ENABLED` | `true` | Глобальный выключатель |
| `CICD_WEBHOOK_TIMEOUT_SECS` | `10` | Timeout доставки |
| `CICD_WEBHOOK_MAX_RETRIES` | `3` | Макс. попыток |
| `CICD_WEBHOOK_RATE_LIMIT` | `100` | Макс. доставок в минуту на URL |

---

## 10. План реализации

- [ ] Таблицы: `webhooks`, `webhook_sources`, `webhook_deliveries`.
- [ ] Endpoint `POST /api/v1/webhooks/incoming` с верификацией подписи.
- [ ] Парсеры payload: GitHub, GitLab, Gitea, generic.
- [ ] Маппинг repository_url → project, ref → pipeline trigger.
- [ ] Исходящие webhooks: async delivery task, retry, HMAC-SHA256.
- [ ] API CRUD для webhooks и webhook_sources.
- [ ] Frontend: webhook management, delivery history.
- [ ] Тесты: signature verification, payload parsing, delivery retry.

---

## References

- `docs/ROADMAP.md` — Phase 6: Webhooks
- `docs/WORKFLOW.md` — статусы пайплайнов (источник событий)
- `docs/NOTIFICATIONS.md` — внутренние уведомления (связанный механизм)
- `docs/SECRETS_MGMT.md` — хранение webhook secrets