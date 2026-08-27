# Контракт жизненного цикла данных

Статус: Accepted target contract. Основание: [ADR-0009](../adr/0009-canonical-registry.md).

Этот контракт определяет хранение, ownership, retention, восстановление и ротацию. Текущий MVP не считается реализующим его без проверяемых миграций, workers и restore drill.

## 1. Границы владения и размещение

- `tenant` -- единственный канонический владелец project-scoped данных. `projects` принадлежат одному `tenant`; pipelines, jobs, artifacts, repositories, secrets, schedules, events, deliveries и audit наследуют `tenant_id` через project либо имеют явный tenant scope.
- Все прикладные PostgreSQL tables и `_sqlx_migrations` находятся в schema `forge`. `public` не используется для доменных tables.
- `forge_owner` владеет database, schema, tables, sequences и DDL; `forge_runtime` имеет только `CONNECT`, `USAGE`, разрешённые DML и sequence usage. Runtime не может `CREATE`, `ALTER`, `DROP`, `TRUNCATE` или выполнять migrations.
- PostgreSQL хранит метаданные, состояния и намерения; Git object contents, artifact bytes, plaintext secrets, raw keys, absolute filesystem paths и credential-bearing URLs в ней запрещены.
- Bare Git и artifacts используют логические immutable storage keys. Artifact object key содержит tenant/project scope; межтенантная deduplication запрещена.
- `domain_events`, `outbox_messages`, `outbox_deliveries` и `audit_events` append-only относительно факта события. Изменяются только delivery/read/retention состояния, а не исходный payload.

## 2. Ownership data sets

| Набор данных | Автор записи | Хранилище | Необходимая связь |
|---|---|---|---|
| Project, pipeline, job, execution metadata | Соответствующий application service | PostgreSQL `forge` | `tenant_id` через project |
| Job logs | Runner/application service, append-only | PostgreSQL `forge` | `job_id`, project/tenant через pipeline |
| Artifact metadata | Artifact service | PostgreSQL `forge` | `artifact -> artifact_object -> tenant/project` |
| Artifact bytes | Object-store adapter | Private versioned object storage | Immutable object key/version/checksum |
| Bare repository bytes | Git adapter | Dedicated Git storage | `repository_id`, logical `storage_key` |
| Secret metadata/ciphertext | Secret service | PostgreSQL `forge` | `tenant_id`, `project_id`, versioned encrypted envelope |
| Domain events/deliveries | Aggregate transaction/workers | PostgreSQL `forge` | Event tenant/project scope and correlation IDs |
| Audit | Security-sensitive application transaction | PostgreSQL `forge`, archive | Tenant/project scope, actor/resource/request IDs |
| Backup manifest | Backup service | PostgreSQL catalog + off-site store | Backup ID, LSN, snapshots, checksums, key references |

Прямой доступ frontend к object bucket, Git volume, ciphertext или backup storage запрещён. Download, clone, secret injection и delivery history проходят server-side authorisation и audit.

## 3. Retention и удаление

Срок определяется tenant/project policy и фиксируется в объекте при создании. Policy не меняет уже созданный `expires_at` задним числом, кроме законного hold или отдельно аудируемой administrative операции.

| Данные | Нормативный срок | Удаление/архив |
|---|---|---|
| Job logs | Project policy 30--90 дней | Batch/partition purge после expiry; log order и sequence не переписываются. |
| Artifacts | Default 30 дней; tenant policy с утверждёнными min/max | `available -> expired -> delete_pending -> purged`; legal/incident hold блокирует physical purge. |
| Pipeline/job metadata | 180--365 дней; aggregate reports могут храниться дольше | Controlled purge с tombstone, не user-request `CASCADE`. |
| `domain_events`, schedule fires, terminal deliveries | Не менее 180 дней | Архив или batch purge с audit. Sensitive payload не сохраняется либо хранится зашифрованно. |
| Delivery response previews | Не более 30 дней | Sanitize и delete отдельно от delivery metadata. |
| Audit events | Не менее 365 дней | Append-only archive/export, затем утверждённый purge; cascade запрещён. |
| Deleted bare repository | Quarantine 7--30 дней | Read/write disabled до final `git fsck` и purge. |
| Superseded secret versions | До конца rollback window и retention всех использующих backup | Затем cryptographic erasure. |
| Backups | Daily не менее 35 дней, monthly не менее 12 месяцев | Immutable off-site copy; lifecycle не удаляет referenced object version раньше backup retention. |

Retention worker выбирает due rows с lease и `FOR UPDATE SKIP LOCKED`. Он не удаляет shared artifact object при active reference, не снимает hold, не удаляет audit cascade и не объявляет success до подтверждения physical deletion. Temporary storage error создаёт retry с bounded backoff и alert после порога.

Удаление tenant/project выполняется как asynchronous lifecycle: `active -> delete_requested -> access_revoked -> purging -> deleted`, либо `delete_failed`. Сначала блокируются новые pipelines, uploads, secret injection, clone/push и downloads; затем создаются deletion jobs, сохраняется минимальный tombstone и производится идемпотентный purge. Access не возвращается при failed purge.

## 4. Integrity, access и audit

- Artifact upload потоково вычисляет SHA-256; immutable object публикуется только после successful metadata commit. Download проверяет `artifact.read`, tenant/project scope, state, expiry и hold, и возвращает digest/version headers.
- Object/store mismatch, checksum mismatch или Git integrity failure блокирует доступ к повреждённому ресурсу и создаёт high-severity audit/alert. Git backup/purge использует `git fsck`.
- Audit event содержит immutable occurred time, tenant/project, actor, action, outcome, resource, request/correlation ID и allowlisted redacted metadata. Password, token, plaintext/ciphertext secret, signed URL, raw headers и full IP запрещены.
- Security-sensitive mutation и его audit event фиксируются одной транзакцией. Audit history не удаляется вместе с actor/project; hash chain/checkpoints и archive verification обязательны.
- Hold creation/removal, retention-policy change, deletion request, restore, key rotation и backup failure требуют audit. Hold не даёт доступ к данным.

## 5. Backup, restore и DR

Начальные production SLO: PostgreSQL metadata RPO не более 15 минут; Git/artifacts RPO не более 24 часов; single-region RTO не более 4 часов. Backup и transport всегда encrypted; off-site copy находится в независимом failure domain.

Complete backup имеет immutable manifest с PostgreSQL backup ID, WAL/LSN boundary, applied migration version/checksum, Git snapshot/bundle IDs и SHA-256, artifact object versions/checksums, required KMS key IDs, backup encryption key ID, application/schema version и digest manifest.

| Требование | Обязательное поведение |
|---|---|
| PostgreSQL | Continuous WAL archiving и physical base backup для PITR; daily portable logical dump; integrity verification. |
| Git | Crash-consistent snapshot либо `git bundle --all` после `git fsck`; checksum каждого bundle/snapshot. |
| Artifacts | Versioned private object storage, inventory конкретных object versions и off-site replication. |
| Catalog | `backup_catalog` получает `verified` только после checksums/manifest verification. |
| Restore drill | Не реже раза в месяц и после изменения migration, storage или key policy; report фиксирует фактические RPO/RTO. |

Restore выполняется только в isolated environment: создать roles/schema, restore PostgreSQL до manifest boundary, запустить migration verify без DDL, восстановить и проверить Git/artifacts, проверить доступность требуемых KMS key versions, выполнить controlled decrypt test secret и read-only smoke. Traffic переключается только по решению incident commander. После partial restore reconciler создаёт findings, но не удаляет данные автоматически первые 24 часа recovery window.

## 6. Envelope encryption и key rotation

Каждая `secret_version` использует новый случайный 256-bit DEK: plaintext шифруется AES-256-GCM, DEK заворачивается KEK KMS/Vault. В PostgreSQL допускаются только ciphertext, nonce, encrypted DEK, `kek_key_id`, algorithm, AAD version и lifecycle metadata. AAD включает `tenant_id`, `project_id`, secret ID и version, исключая подмену ciphertext между scope.

| Операция | Нормативное поведение |
|---|---|
| Новый secret | Использует active KEK и новую DEK; plaintext показывается только create/update caller и не логируется. |
| Ротация KEK | Новые writes используют новый `kek_key_id`; rewrap worker заменяет encrypted DEK без изменения ciphertext и, где возможно, без вывода plaintext из KMS boundary. |
| Ротация secret value | Создаёт новую `secret_version`; прежняя получает `superseded_at`, rollback возможен только в retention window и аудируется. |
| Retirement KEK | Запрещён, пока все active envelopes не rewrapped, не истёк backup retention со старым KEK и restore drill не подтверждён. |
| Crypto erasure | После expiry удаляются ciphertext и encrypted DEK; уничтожение KMS material допускается только после условий backup retention. |
| Legacy migration | Dual decrypt временно допустим; каждая запись переносится atomically в envelope version до удаления legacy ciphertext. |

DEK/plaintext существует в памяти только на время криптооперации best-effort. Secret decrypt разрешён только trusted runner use-case с действующим lease; injection идёт через protected environment/temp file, не command line. Runner redactor маскирует value на время выполнения.

## 7. Проверяемые требования

- Real PostgreSQL tests проверяют tenant isolation, runtime DDL denial, retention retry/hold, artifact checksum и non-orphan publish/purge.
- Backup test проверяет неполный manifest, checksum/key-unavailable error и isolated restore smoke.
- Security tests проверяют отсутствие secret в API/audit/log payload, AAD mismatch, KEK rewrap, secret-value rotation и запрет раннего key retirement.
- Operations evidence включает последний verified backup, restore report, retention queue age, checksum/Git integrity alerts и число envelope/key versions в use.
