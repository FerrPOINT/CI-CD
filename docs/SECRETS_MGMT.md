# Secrets Management — Forge CI/CD

## 1. Обзор

План Phase 7: безопасное хранение секретов проектов (API-токены, пароли, ключи) в зашифрованном виде, инжекция в задачи при выполнении, ротация и маскирование в логах.

> **Статус:** Planned (Phase 7). Не реализовано. См. `docs/ROADMAP.md`.

---

## 2. Архитектура

```
┌──────────────┐     ┌───────────────┐     ┌──────────────────┐
│  User / API  │────▶│  Secrets API  │────▶│  AES-256-GCM     │
│  (set secret)│     │  (CRUD)       │     │  Encrypt/Decrypt │
└──────────────┘     └───────────────┘     └────────┬─────────┘
                                                     │
                                              ┌──────▼──────┐
                                              │  PostgreSQL │
                                              │  (encrypted)│
                                              └──────┬──────┘
                                                     │
┌──────────────┐     ┌───────────────┐              │
│  Runner      │────▶│  Secrets API  │────▶─────────┘
│  (get secret)│     │  (decrypt)    │     decrypt → env var
└──────────────┘     └───────────────┘     inject into job
```

### Принципы

- Секреты шифруются на уровне приложения (AES-256-GCM) перед записью в БД.
- Ключ шифрования хранится вне БД — в env-переменной `CICD_SECRETS_KEY`.
- Plain-text значение секрета никогда не возвращается API (кроме masked preview).
- Секреты привязаны к проекту — один проект не может читать секреты другого.
- Маскирование секретов в логах задач (replace на `***`).

---

## 3. Шифрование

### 3.1. Алгоритм

| Параметр | Значение |
|---|---|
| Алгоритм | AES-256-GCM |
| Длина ключа | 32 байта (256 бит) |
| Nonce | 12 байт (случайный, per-secret) |
| Auth tag | 16 байт |
| Crate | `aes-gcm` |

### 3.2. Ключ шифрования

Ключ задаётся через env-переменную `CICD_SECRETS_KEY` в формате base64:

```bash
# Генерация ключа
openssl rand -base64 32
# Вывод: k3JxQ9vN4mZp7RwL2XtY5bH8cF1dG6sE0aU3iO9jNkM=
```

```bash
# .env
CICD_SECRETS_KEY=k3JxQ9vN4mZp7RwL2XtY5bH8cF1dG6sE0aU3iO9jNkM=
```

### 3.3. Формат зашифрованного значения

В БД хранится:

```
base64(nonce || ciphertext || auth_tag)
```

```rust
fn encrypt(key: &[u8; 32], plaintext: &[u8]) -> Result<String, Error> {
    let cipher = Aes256Gcm::new(key);
    let mut nonce = [0u8; 12];
    OsRng.fill_bytes(&mut nonce);
    let ciphertext = cipher.encrypt(&nonce.into(), plaintext)?;
    let mut combined = nonce.to_vec();
    combined.extend_from_slice(&ciphertext);
    Ok(BASE64_STANDARD.encode(&combined))
}

fn decrypt(key: &[u8; 32], encoded: &str) -> Result<Vec<u8>, Error> {
    let combined = BASE64_STANDARD.decode(encoded)?;
    let (nonce_bytes, ciphertext) = combined.split_at(12);
    let cipher = Aes256Gcm::new(key);
    cipher.decrypt(nonce_bytes.into(), ciphertext)
}
```

---

## 4. Дата-модель (план)

### 4.1. Таблица secrets

```sql
CREATE TABLE IF NOT EXISTS secrets (
    id              UUID PRIMARY KEY,
    project_id      UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    key             TEXT NOT NULL,
    encrypted_value TEXT NOT NULL,
    description     TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    rotated_at      TIMESTAMPTZ,
    UNIQUE(project_id, key)
);
```

| Колонка | Тип | Описание |
|---|---|---|
| `id` | UUID | Первичный ключ |
| `project_id` | UUID | FK → `projects.id`, CASCADE |
| `key` | UUID | Имя секрета (e.g. `DEPLOY_TOKEN`, `NPM_TOKEN`) |
| `encrypted_value` | TEXT | Зашифрованное значение (base64) |
| `description` | TEXT | Описание (опционально) |
| `created_at` | TIMESTAMPTZ | Время создания |
| `updated_at` | TIMESTAMPTZ | Время последнего обновления |
| `rotated_at` | TIMESTAMPTZ | Время последней ротации |

**Ограничения:**
- `UNIQUE(project_id, key)` — уникальное имя секрета в рамках проекта.
- `CASCADE` — удаление проекта удаляет все его секреты.

---

## 5. API (план)

### 5.1. Endpoints

| Метод | Путь | Назначение |
|---|---|---|
| `GET` | `/api/v1/projects/{id}/secrets` | Список секретов (без значений) |
| `POST` | `/api/v1/projects/{id}/secrets` | Создать/обновить секрет |
| `DELETE` | `/api/v1/projects/{id}/secrets/{key}` | Удалить секрет |
| `POST` | `/api/v1/projects/{id}/secrets/{key}/rotate` | Ротация секрета |

### 5.2. GET — список секретов

```bash
curl -sS http://127.0.0.1:22801/api/v1/projects/{id}/secrets
```

```json
[
  {
    "id": "uuid",
    "key": "DEPLOY_TOKEN",
    "description": "Token for deploy server",
    "createdAt": "2026-08-26T10:00:00Z",
    "updatedAt": "2026-08-26T10:00:00Z",
    "rotatedAt": null
  }
]
```

> Значения не возвращаются. Поле `encrypted_value` не присутствует в response.

### 5.3. POST — создать/обновить секрет

```bash
curl -sS -X POST http://127.0.0.1:22801/api/v1/projects/{id}/secrets \
  -H "Content-Type: application/json" \
  -d '{
    "key": "DEPLOY_TOKEN",
    "value": "glpat-xxxxxxxxxxxxxxxxxxxx",
    "description": "GitLab deploy token"
  }'
```

```json
{
  "id": "uuid",
  "key": "DEPLOY_TOKEN",
  "createdAt": "2026-08-26T10:00:00Z"
}
```

### 5.4. DELETE — удалить секрет

```bash
curl -sS -X DELETE http://127.0.0.1:22801/api/v1/projects/{id}/secrets/DEPLOY_TOKEN
```

Response: 204 No Content.

### 5.5. POST — ротация секрета

```bash
curl -sS -X POST http://127.0.0.1:22801/api/v1/projects/{id}/secrets/DEPLOY_TOKEN/rotate \
  -H "Content-Type: application/json" \
  -d '{"value": "glpat-new-xxxxxxxxxxxxxxxxxxxx"}'
```

```json
{
  "id": "uuid",
  "key": "DEPLOY_TOKEN",
  "rotatedAt": "2026-08-26T12:00:00Z"
}
```

---

## 6. Инжекция в задачи

### 6.1. Механизм

При запуске job runner получает секреты проекта и инжектирует их как env-переменные:

```
DEPLOY_TOKEN=glpat-xxxxxxxxxxxxxxxxxxxx
NPM_TOKEN=npm_xxxxxxxxxxxxxxxx
```

### 6.2. Доступ runner'а к секретам

```rust
// Runner запрашивает секреты перед выполнением job
let secrets = client.get_project_secrets(project_id).await?;

// Секреты передаются в Docker-контейнер как env vars
let envs: Vec<(&str, &str)> = secrets.iter()
    .map(|s| (s.key.as_str(), s.value.as_str()))
    .collect();

docker.run(&image, &command, envs).await?;
```

### 6.3. Выборочные секреты

Future: поддержка указания списка нужных секретов в конфигурации job (YAML):

```yaml
jobs:
  - name: deploy
    image: alpine:3.21
    command: ./deploy.sh
    secrets: [DEPLOY_TOKEN, SSH_KEY]  # только эти секреты
```

---

## 7. Маскирование в логах

### 7.1. Принцип

Перед записью строки в `job_logs` все известные секреты заменяются на `***`:

```rust
fn mask_secrets(message: &str, secrets: &[String]) -> String {
    let mut result = message.to_string();
    for secret in secrets {
        if !secret.is_empty() && message.contains(secret) {
            result = result.replace(secret, "***");
        }
    }
    result
}
```

### 7.2. Пример

Входной лог:
```
Deploying with token glpat-xxxxxxxxxxxxxxxxxxxx to server...
```

Записанный лог:
```
Deploying with token *** to server...
```

### 7.3. Гарантии

- Маскирование применяется перед записью в БД (`job_logs`).
- Секреты загружаются один раз перед выполнением job и кэшируются на время выполнения.
- Маскирование не зависит от runner'а — применяется на уровне API.

---

## 8. Ротация секретов

### 8.1. Процесс

1. Администратор генерирует новое значение секрета.
2. `POST /api/v1/projects/{id}/secrets/{key}/rotate` с новым значением.
3. Старое значение немедленно заменяется — новые запуски jobs используют обновлённый секрет.
4. Текущие running jobs продолжают использовать старое значение до завершения.
5. `rotated_at` обновляется для аудита.

### 8.2. Ротация ключа шифрования (master key rotation)

При смене `CICD_SECRETS_KEY`:

1. Расшифровать все секреты старым ключом.
2. Зашифровать новым ключом.
3. Обновить `encrypted_value` в БД.
4. Обновить env-переменную.

CLI-команда (план):

```bash
cicd-cli secrets rotate-master-key --old-key-file ./old.key --new-key-file ./new.key
```

---

## 9. Безопасность

### 9.1. Правила

- Ключ `CICD_SECRETS_KEY` никогда не коммитится в репозиторий.
- Ключ хранится в `.env` (gitignored) или в vault (HashiCorp Vault, AWS Secrets Manager).
- API никогда не возвращает расшифрованные значения.
- Логирование расшифрованных значений запрещено (code review check).
- Доступ к секретам — только через API с аутентификацией (Phase 1: Auth).
- Audit log: все операции с секретами логируются (кто, когда, какой ключ, действие).

### 9.2. Чек-лист code review

- [ ] В коде нет `println!` / `tracing::info!` с расшифрованным значением.
- [ ] Тесты не используют реальные секреты.
- [ ] Новые env-переменные добавлены в `.env.example` (без значений).
- [ ] Доступ к секретам через `project_id` (изоляция проектов).

---

## 10. Frontend (план)

- Вкладка "Secrets" в настройках проекта.
- Список секретов: ключ, описание, время создания/обновления, статус ротации.
- Кнопка "Add secret" → форма (key, value, description).
- Значения скрыты по умолчанию (`••••••••`), reveal on click (только preview, не полный текст).
- Кнопка "Rotate" → форма ввода нового значения.
- Кнопка "Delete" с подтверждением.
- Предупреждение о ключе шифрования: баннер если `CICD_SECRETS_KEY` не задан.

---

## 11. Env-переменные (план)

| Переменная | Default | Описание |
|---|---|---|
| `CICD_SECRETS_KEY` | — | Base64-ключ AES-256 (обязательно) |
| `CICD_SECRETS_ENABLED` | `true` | Глобальный выключатель |
| `CICD_SECRETS_MAX_PER_PROJECT` | `100` | Лимит секретов на проект |
| `CICD_SECRETS_MAX_VALUE_LENGTH` | `4096` | Макс. длина значения (байт) |

---

## 12. План реализации

- [ ] Таблица `secrets` в БД.
- [ ] Шифрование/дешифрование: `aes-gcm` crate, `CICD_SECRETS_KEY`.
- [ ] API: `GET/POST/DELETE /projects/{id}/secrets`, `POST .../rotate`.
- [ ] Инжекция секретов в env vars при выполнении job.
- [ ] Маскирование секретов в логах (`job_logs`).
- [ ] Audit log: запись операций с секретами.
- [ ] Frontend: secrets management UI.
- [ ] Тесты: encryption/decryption unit, masking, API integration, project isolation.

---

## References

- `docs/ROADMAP.md` — Phase 7: Secrets
- `docs/ARCHITECTURE.md` — стек, `AppState`
- `docs/WEBHOOKS.md` — webhook secrets (используют тот же механизм)
- `docs/CODE_REVIEW.md` — чек-лист безопасности