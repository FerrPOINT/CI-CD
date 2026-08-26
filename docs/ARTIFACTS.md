# Artifacts — Хранение артефактов сборки Forge CI/CD

## 1. Обзор

План Phase 8: хранение артефактов сборки (бинарники, архивы, отчёты, coverage) с политикой удержания, API загрузки/скачивания и поддержкой S3-совместимого бэкенда.

> **Статус:** Planned (Phase 8). Не реализовано. См. `docs/ROADMAP.md`.

---

## 2. Архитектура

```
┌──────────────┐     ┌──────────────────┐     ┌─────────────────────┐
│  Runner      │────▶│  Artifacts API   │────▶│  Storage Backend    │
│  (upload)    │     │  (multipart)     │     │  (FS / S3)          │
└──────────────┘     └──────────────────┘     └─────────────────────┘
                                                      │
┌──────────────┐     ┌──────────────────┐            │
│  User / CLI  │────▶│  Artifacts API   │────▶───────┘
│  (download)  │     │  (stream)        │     read file / S3 object
└──────────────┘     └──────────────────┘
                                                      │
                     ┌──────────────────┐            │
                     │  Retention Job   │────▶───────┘
                     │  (cleanup TTL)   │     delete expired
                     └──────────────────┘
```

### Бэкенды хранения

| Бэкенд | Окружение | Описание |
|---|---|---|
| Local FS | Dev / single-node | Файловая система (`CICD_STORAGE` директория) |
| S3-compatible | Production | MinIO, AWS S3, Garage, SeaweedFS |

---

## 3. Дата-модель (план)

### 3.1. Таблица artifacts

```sql
CREATE TABLE IF NOT EXISTS artifacts (
    id              UUID PRIMARY KEY,
    job_id          UUID NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
    filename        TEXT NOT NULL,
    size_bytes      BIGINT NOT NULL,
    content_type    TEXT NOT NULL DEFAULT 'application/octet-stream',
    storage_key     TEXT NOT NULL,
    storage_backend TEXT NOT NULL DEFAULT 'local' CHECK (storage_backend IN ('local','s3')),
    checksum_sha256 TEXT NOT NULL,
    expires_at      TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

| Колонка | Тип | Описание |
|---|---|---|
| `id` | UUID | Первичный ключ |
| `job_id` | UUID | FK → `jobs.id`, CASCADE |
| `filename` | TEXT | Имя файла (e.g. `app.tar.gz`) |
| `size_bytes` | BIGINT | Размер в байтах |
| `content_type` | TEXT | MIME-тип |
| `storage_key` | TEXT | Путь в storage (FS path или S3 key) |
| `storage_backend` | TEXT | `local` или `s3` |
| `checksum_sha256` | TEXT | SHA256 контрольная сумма |
| `expires_at` | TIMESTAMPTZ | Время истечения (NULL = без TTL) |
| `created_at` | TIMESTAMPTZ | Время загрузки |

**Индексы:**
- `artifacts_pkey` — PRIMARY KEY (id)
- `idx_artifacts_job_id` — INDEX on `job_id` (для списка артефактов job)
- `idx_artifacts_expires_at` — INDEX on `expires_at` WHERE `expires_at IS NOT NULL` (для cleanup job)

---

## 4. API (план)

### 4.1. Загрузка артефакта

```bash
POST /api/v1/jobs/{job_id}/artifacts
Content-Type: multipart/form-data

--boundary
Content-Disposition: form-data; name="file"; filename="app.tar.gz"
Content-Type: application/gzip

<binary data>
--boundary
Content-Disposition: form-data; name="expires_in"

86400
--boundary--
```

**Response 201:**

```json
{
  "id": "uuid",
  "jobId": "uuid",
  "filename": "app.tar.gz",
  "sizeBytes": 15728640,
  "contentType": "application/gzip",
  "checksumSha256": "a1b2c3d4...",
  "expiresAt": "2026-08-27T12:00:00Z",
  "createdAt": "2026-08-26T12:00:00Z"
}
```

**Ограничения:**
- Максимальный размер файла: `CICD_ARTIFACTS_MAX_SIZE` (default 100 MB).
- Поддерживается только один файл за запрос.
- `expires_in` — секунды до истечения (опционально, default из `CICD_ARTIFACTS_TTL`).

### 4.2. Скачивание артефакта

```bash
GET /api/v1/artifacts/{id}/download
```

**Response 200:**

```
Content-Type: application/gzip
Content-Disposition: attachment; filename="app.tar.gz"
Content-Length: 15728640

<binary data>
```

Потоковая отдача (stream) — не загружается в память целиком.

### 4.3. Список артефактов job

```bash
GET /api/v1/jobs/{job_id}/artifacts
```

```json
[
  {
    "id": "uuid",
    "filename": "app.tar.gz",
    "sizeBytes": 15728640,
    "contentType": "application/gzip",
    "expiresAt": "2026-08-27T12:00:00Z",
    "createdAt": "2026-08-26T12:00:00Z"
  }
]
```

### 4.4. Удаление артефакта

```bash
DELETE /api/v1/artifacts/{id}
```

Response: 204 No Content.

### 4.5. Полный список endpoints

| Метод | Путь | Назначение |
|---|---|---|
| `GET` | `/api/v1/jobs/{job_id}/artifacts` | Список артефактов job |
| `POST` | `/api/v1/jobs/{job_id}/artifacts` | Загрузка артефакта (multipart) |
| `GET` | `/api/v1/artifacts/{id}` | Метаданные артефакта |
| `GET` | `/api/v1/artifacts/{id}/download` | Скачивание (stream) |
| `DELETE` | `/api/v1/artifacts/{id}` | Удаление артефакта |

---

## 5. Storage Backends

### 5.1. Local File System

**Конфигурация:**

```bash
CICD_STORAGE_BACKEND=local
CICD_STORAGE_PATH=/var/lib/cicd/artifacts
```

**Структура директорий:**

```
/var/lib/cicd/artifacts/
├── {job_id[0..2]}/
│   └── {job_id}/
│       └── {artifact_id}_{filename}
```

Sharding по первым 2 символам `job_id` предотвращает скопление файлов в одной директории.

### 5.2. S3-compatible

**Конфигурация:**

```bash
CICD_STORAGE_BACKEND=s3
CICD_S3_ENDPOINT=http://minio:9000
CICD_S3_BUCKET=cicd-artifacts
CICD_S3_ACCESS_KEY=minioadmin
CICD_S3_SECRET_KEY=minioadmin
CICD_S3_REGION=us-east-1
CICD_S3_FORCE_PATH_STYLE=true
```

**Storage key:**

```
artifacts/{job_id}/{artifact_id}_{filename}
```

**Crate:** `aws-sdk-s3` или `s3` crate.

**Поддерживаемые S3-провайдеры:**

| Провайдер | Notes |
|---|---|
| MinIO | Self-hosted, default для dev |
| AWS S3 | Cloud production |
| Garage | Self-hosted, lightweight |
| SeaweedFS | Self-hosted, distributed |
| Cloudflare R2 | Cloud, no egress fees |

### 5.3. Абстракция

```rust
#[async_trait]
pub trait ArtifactStorage: Send + Sync {
    async fn upload(&self, key: &str, reader: impl AsyncRead + Send) -> Result<()>;
    async fn download(&self, key: &str) -> Result<impl AsyncRead + Send>;
    async fn delete(&self, key: &str) -> Result<()>;
    async fn exists(&self, key: &str) -> Result<bool>;
}

pub struct LocalStorage { root: PathBuf }
pub struct S3Storage { client: S3Client, bucket: String }
```

---

## 6. Политика удержания (Retention)

### 6.1. TTL

Каждый артефакт имеет `expires_at` — время, после которого он удаляется.

| Параметр | Default | Описание |
|---|---|---|
| `CICD_ARTIFACTS_TTL` | `604800` (7 дней) | TTL по умолчанию (секунды) |
| `CICD_ARTIFACTS_MAX_SIZE` | `104857600` (100 MB) | Макс. размер файла |

### 6.2. Cleanup job

Фоновый процесс (каждый час) удаляет истёкшие артефакты:

```sql
SELECT id, storage_key, storage_backend FROM artifacts
WHERE expires_at IS NOT NULL AND expires_at < now()
LIMIT 100;
```

Для каждого:
1. Удалить файл из storage backend (FS или S3).
2. Удалить запись из `artifacts`.

### 6.3. Без TTL

Если `expires_at = NULL`, артефакт хранится бессрочно. Удаляется только вручную через API или при удалении job (CASCADE).

### 6.4. Ручное управление

Артефакты можно удалить через:
- `DELETE /api/v1/artifacts/{id}` — один артефакт.
- Удаление job → CASCADE удаляет все артефакты.
- Удаление stage/pipeline → CASCADE до jobs → CASCADE до artifacts.

---

## 7. Контроль целостности

### 7.1. SHA256 checksum

При загрузке вычисляется SHA256 checksum и сохраняется в `checksum_sha256`.

```rust
use sha2::{Sha256, Digest};

let mut hasher = Sha256::new();
tokio::io::copy(&mut reader, &mut hasher).await?;
let checksum = hex::encode(hasher.finalize());
```

### 7.2. Верификация при скачивании

Опционально: клиент может проверить целостность:

```bash
curl -sS http://127.0.0.1:22801/api/v1/artifacts/{id}/download | sha256sum
# Сравнить с checksumSha256 из метаданных
```

---

## 8. Frontend (план)

- Вкладка "Artifacts" в деталях job.
- Список артефактов: имя, размер, тип, время загрузки, истечение.
- Кнопка "Download" — скачивание через API.
- Кнопка "Delete" — удаление с подтверждением.
- Индикатор истечения (warning badge если `expires_at` < 24h).
- Прогресс-бар при загрузке (multipart upload).

---

## 9. Env-переменные (план)

| Переменная | Default | Описание |
|---|---|---|
| `CICD_ARTIFACTS_ENABLED` | `true` | Глобальный выключатель |
| `CICD_STORAGE_BACKEND` | `local` | `local` или `s3` |
| `CICD_STORAGE_PATH` | `/var/lib/cicd/artifacts` | Путь для local storage |
| `CICD_S3_ENDPOINT` | — | URL S3-совместимого хранилища |
| `CICD_S3_BUCKET` | — | Имя bucket |
| `CICD_S3_ACCESS_KEY` | — | Access key |
| `CICD_S3_SECRET_KEY` | — | Secret key |
| `CICD_S3_REGION` | `us-east-1` | Регион |
| `CICD_S3_FORCE_PATH_STYLE` | `true` | Path-style URLs (MinIO) |
| `CICD_ARTIFACTS_TTL` | `604800` | TTL по умолчанию (секунды) |
| `CICD_ARTIFACTS_MAX_SIZE` | `104857600` | Макс. размер (байт, 100 MB) |
| `CICD_ARTIFACTS_CLEANUP_INTERVAL` | `3600` | Интервал cleanup job (секунды) |

---

## 10. Docker Compose (план, S3 dev)

```yaml
services:
  minio:
    image: minio/minio:latest
    command: server /data --console-address ":9001"
    environment:
      MINIO_ROOT_USER: minioadmin
      MINIO_ROOT_PASSWORD: minioadmin
    ports:
      - "9000:9000"
      - "9001:9001"
    volumes:
      - cicd_minio_data:/data

volumes:
  cicd_minio_data:
```

---

## 11. План реализации

- [ ] Таблица `artifacts` в БД.
- [ ] Trait `ArtifactStorage` + реализации `LocalStorage`, `S3Storage`.
- [ ] API: upload (multipart), download (stream), list, delete.
- [ ] SHA256 checksum при загрузке.
- [ ] Cleanup job: удаление истёкших артефактов.
- [ ] Конфигурация: local / S3 через env vars.
- [ ] Frontend: artifacts tab в job details.
- [ ] Docker Compose: MinIO для dev (опционально).
- [ ] Тесты: upload/download, size limit, TTL cleanup, S3 integration.

---

## References

- `docs/ROADMAP.md` — Phase 8: Artifacts
- `docs/ARCHITECTURE.md` — стек, Docker Compose
- `docs/DATA_MODEL.md` — таблица `jobs` (FK для artifacts)
- `docs/API.md` — REST API спецификация