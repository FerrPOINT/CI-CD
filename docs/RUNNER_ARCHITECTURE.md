# Целевая архитектура исполнения pipeline и runner-ов Forge CI/CD

> **Статус:** объяснительный narrative. Нормативные контракты — `contracts/RUNNER_PROTOCOL.md`; при конфликте прав контракт (ADR-0009). Текущее состояние — `docs/CURRENT_STATE.md`.

## 1. Назначение и границы

Документ задаёт целевую архитектуру execution-подсистемы Forge CI/CD: от чтения `.forge-ci.yml` по зафиксированному Git commit до планирования DAG, безопасной выдачи работы внешнему runner-у, сохранения логов и артефактов, обработки отказов и финальной агрегации pipeline.

Целевой Forge остаётся **self-hosted control plane**. Он хранит состояние, проверяет политики и координирует выполнение, но **никогда не запускает пользовательские команды в процессе API/control-plane** и не получает доступ к Docker socket через API-контейнер.

### Основные свойства

- Надёжное хранение координационного состояния в PostgreSQL.
- Pull-модель: runner самостоятельно запрашивает работу, а control plane выдаёт ограниченную по времени lease.
- At-least-once доставка assignment и **at-most-one активная lease** на конкретный job execution.
- Явные, проверяемые state machine для pipeline, job, execution attempt, lease и runner.
- Неизменяемый execution plan: повторный запуск всегда воспроизводим относительно commit SHA и сохранённого plan snapshot.
- Разделение control plane, runner process и execution backend.
- Docker как первый backend исполнения; Kubernetes - отдельный адаптер с тем же контрактом.
- Секреты расшифровываются минимально необходимое время, не сохраняются в логах, событиях или metadata.
- Логи и артефакты принадлежат конкретному execution attempt, а не перезаписывают историю job.

---

## 2. Текущее состояние и целевое состояние

| Область | Сейчас | Цель |
|---|---|---|
| Архитектура backend | Переходный monolith: `api.rs`, `platform.rs`, `runner.rs`, `store.rs`; `domain` и `cli` уже выделяются в workspace; `forge-runner` существует как отдельный shell-runner binary | Полный Cargo workspace `domain → app → infra → api`, отдельный `server` composition root и production runner process |
| Pipeline config | `.forge-ci.yml` уже читается из локального bare-репозитория по best-effort resolved commit; поддерживаются legacy линейные `stages/jobs` и v1 DAG MVP (`version: 1`, top-level `jobs.commands/needs/tags/secrets` + `defaults.tags`), при отсутствии файла используется `legacy_template`; `pipeline_plans` хранит raw config/template, parser version и SHA-256 hashes | Policy validation, triggers, retry/artifacts и production diagnostics |
| Планирование | Current `pipeline_plans` хранит normalised `legacy-linear` или `v1-dag` snapshot; v1 `needs` проходит topological validation и исполняется через runtime-стадии `dag-*` | Job-level DAG dispatcher с `needs`, матрицами, rules и неизменяемым policy-aware plan |
| Выполнение | Embedded supervisor в `cicd-server`; Docker или host shell; PID map в памяти; `job_leases` фиксирует embedded owner/expiry/terminal outcome; external runner protocol MVP может claim/ack/`secrets:resolve`/renew/logs/complete queued job через API; `forge-runner` shell MVP выполняет checkout/commands/secret env/log append/terminal completion отдельным процессом | Отдельные sandboxed runner-ы, pull protocol, leases, attempts, reconciliation; host shell отсутствует в production |
| Runner registry | Legacy `POST /runners` inventory плюс protocol register через `CICD_RUNNER_REGISTRATION_TOKEN`; runner credential hash, tags, heartbeat, capacity, capabilities, drain/disable metadata сохраняются в `runners` | Runner pools/scopes, credential rotation/revocation UI/API, mTLS/service identity, selection dispatcher |
| Очередь | Durable `job_queue` материализует dispatch row для current queued attempt и хранит `required_tags`; trigger/retry/manual start enqueue-ят non-manual work; embedded claim берёт только untagged rows, external `work:poll` claim-ит compatible queue row через `SKIP LOCKED` + `required_tags ⊆ runner.tags`, создаёт active `job_leases`, lease token/`workspace.checkoutUrl`, declared `attempt.secrets`, ack/`secrets:resolve`/renew/logs/complete и fencing generation; long-poll wakeup ещё target | PostgreSQL queue с richer scheduling eligibility, pool/protected-tag policy, capability matching и outbox/event wakeup |
| Retry | API повторно ставит job/pipeline в `queued` и уже создаёт новую `execution_attempt`; external protocol lease fencing есть на generation/token, но policy retry/lost-heartbeat suite ещё target | Policy-driven immutable execution attempt; история предыдущих попыток, логов и artifact manifest сохраняется |
| Cancel | API меняет статусы, закрывает active embedded lease и пытается остановить Docker/PID локального процесса | Cancel intent в БД, delivery к owner runner, grace period, forced backend termination, reconciliation |
| Таймауты | Job timeout убивает локальный процесс; embedded lease получает expiry `timeout_seconds + 60s`, reconciler fail-ит expired/missing lease | Queue, startup, execution, idle-log, cancellation и lease deadlines - конфигурируемые и фиксируемые в attempt |
| Secrets | AES-256-GCM at rest; API не возвращает значения; embedded runner inject-ит только `jobs.required_secrets`; external runner получает declared secret bundle только после ack lease; stdout/stderr masking best-effort | KMS/rotation, env/file injection policy, full redaction до записи логов |
| Logs | Строки `job_logs`, sequence по attempt, bounded page/search API, REST compatibility polling, SSE stream по current/latest attempt и external runner log append с `[stdout]`/`[stderr]`/`[system]` префиксом | Chunk protocol, idempotency, отдельное stream-поле, monotonic sequence per attempt и durable append |
| Artifacts | Local FS, metadata по `job_id` и active/latest `attempt_id`, лимит 50 MiB | Artifact manifest на attempt, checksum, staged upload, S3-compatible storage, retention, quarantine/cleanup |
| Изоляция | API/backend может запускать Docker; есть host-shell fallback; внешний `forge-runner` shell MVP можно запускать отдельно, но без production sandbox | Docker socket только у runner host; rootless/least privilege. Kubernetes runner создаёт ограниченный Job/Pod |
| Тесты | Domain/API/CLI tests, real PostgreSQL integration для persistent paths, frontend unit/build gates | Unit, property, protocol compatibility, runner contract, Docker/K8s integration, chaos/e2e |

В документации и пользовательском интерфейсе текущий механизм следует называть **embedded execution + durable queue + external runner protocol/forge-runner shell MVP**, а не полноценной distributed runner platform: production sandbox, protocol artifacts, long-poll wakeup, pool policy, full redaction/rotation и richer log chunks ещё не завершены.

---

## 3. Целевая топология

```text
                         ┌──────────────────────────────┐
                         │ UI / CLI / Git webhook       │
                         └──────────────┬───────────────┘
                                        │ HTTPS
                         ┌──────────────▼───────────────┐
                         │ Forge API                     │
                         │ auth, RBAC, pipeline commands │
                         └──────────────┬───────────────┘
                                        │ use cases
          ┌─────────────────────────────▼─────────────────────────────┐
          │ Application services                                       │
          │ parser, planner, scheduler, dispatcher, reconciliation    │
          └───────────┬─────────────────────┬─────────────────────────┘
                      │                     │
      ┌───────────────▼──────────────┐ ┌────▼────────────────────────┐
      │ PostgreSQL                    │ │ Object storage              │
      │ plans, queue, leases, logs,   │ │ artifacts, optional cache   │
      │ attempts, runner state, audit │ └─────────────────────────────┘
      └───────────────┬──────────────┘
                      │ HTTPS pull + long poll / wakeup hint
      ┌───────────────▼──────────────────────────────────────────────┐
      │ External Runner Process                                       │
      │ registration, heartbeat, claim, lease renewal, redaction      │
      └───────────────┬──────────────────────────────┬───────────────┘
                      │                              │
          ┌───────────▼───────────┐      ┌───────────▼────────────┐
          │ Docker executor        │      │ Kubernetes executor     │
          │ container per attempt  │      │ Job/Pod per attempt     │
          └────────────────────────┘      └────────────────────────┘
```

### Правило доверия

- API/server доверяет только аутентифицированному runner identity и проверяет право runner-а владеть lease.
- Runner process доверяет только TLS endpoint Forge и assignment, подписанному либо полученному по действующей lease.
- Execution container/Pod считается недоверенным кодом проекта.
- Пользовательский process не имеет DB credentials, control-plane token, Docker socket, service-account с широкими правами или master key секретов.
- Runner не может читать assignment, secret bundle, logs или artifacts другого runner-а/attempt.

---

## 4. Границы workspace и портов

Целевая структура сохраняет решение ADR-0005.

```text
backend/
├── domain/       # entities, value objects, state machines, port traits
├── app/          # use cases, policies, transaction boundaries
├── infra/        # PostgreSQL, Git, object storage, crypto, Docker/K8s adapters
├── api/          # Axum routes, DTO, auth middleware, OpenAPI
├── server/       # composition root, workers, graceful shutdown
├── src/bin/forge-runner.rs # current external shell-runner binary
├── migration/    # SQLx migrations
└── tests/        # black-box PostgreSQL and protocol tests
```

### Domain

Не зависит от Axum, SQLx, Docker, Kubernetes, файловой системы или HTTP. Содержит:

- `PipelineDefinition`, `ExecutionPlan`, `PlannedJob`, `DependencyEdge`.
- `PipelineStatus`, `JobStatus`, `AttemptStatus`, `LeaseStatus`, `RunnerStatus`.
- Политики eligibility, retry и status aggregation.
- Port traits: `PipelineRepository`, `QueueRepository`, `RunnerRepository`, `SecretProvider`, `LogSink`, `ArtifactStore`, `ExecutionBackend`.

### Application

Содержит use cases и транзакционные границы:

- `CreatePipelineRun`
- `ParseAndPlanPipeline`
- `EnqueueReadyJobs`
- `PollAndLeaseWork`
- `AcknowledgeLease`
- `RenewLease`
- `StartAttempt`
- `AppendAttemptLogs`
- `CompleteAttempt`
- `RequestCancellation`
- `ReconcileExpiredLeases`
- `ExpireArtifacts`
- `MarkRunnersOffline`

### Infrastructure

Реализует порты:

- PostgreSQL repositories и SQLx migrations.
- Git config reader по immutable commit SHA.
- Secret crypto / secret delivery adapter.
- Local FS и S3-compatible artifact storage.
- Docker Engine executor.
- Kubernetes Job executor.
- HTTP runner protocol client/server adapters.
- Outbox publisher, metrics, tracing и audit.

### API и server

- `api` преобразует DTO в application commands; SQL в handlers запрещён.
- `server` создаёт `PgPool`, repositories, schedulers, reconciliation workers, storage и API router.
- `runner` - отдельный исполняемый процесс; не линкуется с `server`/Axum handler implementation.

---

## 5. Pipeline parser и immutable execution plan

## 5.1 Источник конфигурации

При trigger pipeline Forge обязан:

1. Разрешить ref в **полный immutable commit SHA**.
2. Прочитать `.forge-ci.yml` именно из этого commit.
3. Сохранить исходный YAML/template, SHA-256 конфигурации, parser version и нормализованный execution plan.
4. Создать pipeline run только после успешной schema/domain validation.

Нельзя читать branch позднее на этапе runner execution: branch может переместиться между trigger и запуском job.

### Совместимость

Current transition phase сохраняет template `build/test/deploy` как `legacy_template` source в `pipeline_plans`; отсутствие `.forge-ci.yml` всё ещё запускает compatibility template. Current v1 DAG MVP принимает `version: 1`, `jobs`, `commands`, `needs`, defaults `image/timeout` и `allow_failure`, сохраняет `v1-dag` plan и проецирует его в runtime-стадии `dag-*` для embedded runner-а. Целевое production-поведение:

- `config_source=repository`: файл обязателен.
- `config_source=legacy_template`: template допустим только для migration/demo проектов.
- Ошибка parser/planner создаёт pipeline в `invalid`/`failed_planning` с diagnostics без постановки job в очередь.

## 5.2 Минимальный целевой DSL

```yaml
version: 1

defaults:
  image: alpine:3.21
  timeout: 20m
  retry:
    max_attempts: 2
    retry_on: [runner_lost, infrastructure, timeout]

jobs:
  build:
    image: rust:1.86
    commands:
      - cargo build --release
    tags: [linux, docker, amd64]
    artifacts:
      paths: [target/release/forge]
      expire_in: 7d

  test:
    needs: [build]
    image: rust:1.86
    commands:
      - cargo test
    timeout: 15m

  deploy:
    needs: [test]
    tags: [linux, production, deploy]
    secrets: [DEPLOY_TOKEN]
    commands:
      - ./scripts/deploy.sh
    retry:
      max_attempts: 1
```

Начальная версия не должна включать arbitrary plugins, shell interpolation в definition, privileged mode, host mounts, Docker-in-Docker или возможность назначить Kubernetes service account из YAML.

## 5.3 Parser pipeline

```text
YAML bytes
  -> safe deserialization with size/depth limits
  -> syntax diagnostics (line/column)
  -> schema version dispatch
  -> normalization/default expansion
  -> static validation
  -> DAG construction
  -> policy validation
  -> immutable ExecutionPlan
```

### Обязательные проверки

- Максимальный размер config, число jobs, dependency edges, matrix expansion и длина command.
- Уникальность logical job name после matrix expansion.
- `needs` ссылается только на существующие jobs.
- Нет циклов; planner возвращает читаемый cycle path.
- Нет self-dependency.
- `tags`, image reference, secret key и artifact path проходят allowlist validation.
- Job с `secrets` имеет разрешение на project/environment secret.
- `timeout`, retry limits, artifact retention не превышают project/platform policy.
- Команды и environment names не попадают в server-side shell.
- Для всех jobs существует хотя бы потенциально совместимый runner class; предупреждение возможно при pipeline creation, но dispatch также проверяет фактическое наличие.
- План должен быть детерминированным: одинаковые `(commit_sha, yaml, parser_version, project policy)` дают одинаковый canonical plan hash.

## 5.4 DAG

Целевой DAG работает на уровне jobs. Стадии могут остаться presentation grouping, но не определяют семантику зависимости.

```text
build ──┬──> test-unit ──┐
        └──> test-lint ──┼──> package ──> deploy
                         │
                         └──> security-scan
```

Job становится eligible, когда:

- pipeline не отменён;
- job не terminal;
- нет активной execution lease;
- выполнен `not_before`;
- все `needs` завершены `success` или удовлетворяют явно заданной `allow_failure` policy;
- выбранный retry backoff истёк;
- project concurrency и runner capacity допускают запуск.

В транзакции с изменением состояния dependency successor должен быть пересчитан и поставлен в queue. Для надёжности периодический scheduler также сканирует jobs, которые должны быть eligible, но не имеют queue row.

---

## 6. Очередь, dispatch и lease

## 6.1 Почему PostgreSQL queue

На первой production-фазе PostgreSQL является источником истины и очередью. Отдельный broker не нужен, пока измеренный throughput не докажет обратное. Все решения по очереди должны переживать restart API и runner.

### Queue row

`job_queue` представляет не команду, а право планировщика выдать **следующую попытку** job:

| Поле | Назначение |
|---|---|
| `id` | UUID queue item |
| `job_id` | Логический job |
| `attempt_id` | Current execution attempt, unique queue owner |
| `pipeline_id`, `stage_id` | Денормализация для scoped scheduling и cascade/cleanup |
| `priority`, `queued_at` | Стабильный порядок |
| `not_before` | Backoff/retry/schedule deadline |
| `required_tags` | Нормализованные требования |
| `state` | `queued`, `leased`, `completed`, `canceled` |
| `lease_id` | Текущая lease, nullable |
| `leased_at`, `completed_at`, `updated_at` | Audit/reconciliation timestamps |

Current MVP indexes: unique `attempt_id`, partial unique open row per `job_id`, unique non-null `lease_id`, ready scan `(priority DESC, not_before, queued_at, id)` where `state='queued'`, GIN `required_tags`, and pipeline/stage state indexes. `required_capabilities`, GIN capabilities, pool-aware matching и protected-tag policy остаются target extension.

## 6.2 Pull protocol

Runner не получает произвольный job push-запросом. Он вызывает poll endpoint с capacity, tags и capability digest:

```text
runner -> POST /api/v1/runner/work:poll
Forge  -> 204 No Content
   или 200 LeaseOffer
runner -> POST /api/v1/runner/leases/{lease_id}/ack
runner -> POST /api/v1/runner/leases/{lease_id}/renew
runner -> POST /api/v1/runner/leases/{lease_id}/complete
```

Long polling допустим до короткого server deadline, например 20-30 секунд. Runner обязан делать exponential backoff с jitter на сетевых/5xx ошибках.

## 6.3 Выбор runner-а

При poll dispatcher выбирает queue item, а не пытается отправить job конкретному host. SQL claim выполняется в короткой транзакции:

1. Проверить authenticated runner, `status=online`, не `draining`, heartbeat свежий, circuit breaker closed.
2. Рассчитать effective free slots: `max_concurrency - active_leases`.
3. Выбрать подходящий queue row через `FOR UPDATE SKIP LOCKED`.
4. Проверить project pool policy, protected tag restrictions, runner tags и capabilities.
5. Создать `execution_attempt` в `leasing`.
6. Создать `job_lease` с `lease_token_hash`, `lease_expires_at`, fencing token.
7. Обновить queue row в `leased`.
8. Commit.
9. Вернуть assignment без plaintext secrets.

Параллельные API instances безопасны за счёт row locking и уникального активного lease constraint на job.

### Matching rule

Runner подходит, если:

```text
required_tags ⊆ runner.tags
AND required_capabilities ⊆ runner.capabilities
AND runner.scope разрешает project/environment
AND runner.status = online
AND runner.available_slots > 0
AND runner не quarantined/draining
```

`tags` описывают размещение и доверительную зону: `linux`, `amd64`, `gpu`, `production`, `internal-network`.

`capabilities` описывают технические свойства: executor `docker|kubernetes`, `arch`, Docker API/version, supported features, max memory/CPU, ephemeral storage, artifact transport version.

Теги `production`, `deploy`, `privileged` и аналогичные должны быть protected: их выдача и использование требуют project policy/RBAC, а не только строки из YAML.

## 6.4 Lease и fencing

Lease защищает от двойного выполнения при разделении сети или restart.

| Параметр | Целевое правило |
|---|---|
| `lease_id` | Уникальная lease конкретной attempt |
| `fencing_token` | Монотонное значение; старый owner не может завершить новую attempt |
| `ack_deadline` | Если runner не подтвердил assignment, lease освобождается |
| `lease_expires_at` | Продлевается только owner runner-ом |
| `renewal_interval` | Существенно короче TTL, например TTL/3 |
| `owner_runner_id` | Единственный runner, который вправе писать logs/status/artifacts |
| `attempt_id` | Неизменяемая связь lease с attempt |

После `lease_expires_at` API не принимает completion, log append или artifact finalization от старой lease. Runner, который обнаружил `409 stale lease`, обязан прекратить execution и локально очистить workspace/backend resource.

---

## 7. Регистрация, аутентификация и жизненный цикл runner-а

## 7.1 Registration

Администратор или project maintainer создаёт **одноразовый registration token** с ограничениями:

- scope: instance / organization / project / runner pool;
- permitted tags и executor kinds;
- expiry;
- максимальное количество регистраций;
- optional bootstrap metadata;
- audit actor и reason.

Runner запускается с token только для bootstrap:

```bash
forge-runner \
  --api-url https://forge.example \
  --registration-token "$CICD_RUNNER_REGISTRATION_TOKEN" \
  --name "builder-a-01"
```

Current binary запускается напрямую с `--registration-token` или с уже сохранённым `--credential`. Команды `register` как отдельного subcommand пока нет; registration выполняется при старте, если credential не передан.

`POST /api/v1/runner/register` возвращает:

- `runner_id`;
- долгоживущий credential, хранимый runner-ом с правами `0600`;
- server CA/pinned public-key information;
- heartbeat/poll limits;
- assigned pool/scopes;
- token rotation deadline.

Registration token не является постоянным API credential и никогда не применяется для выполнения job.

## 7.2 Аутентификация

Целевой minimum:

- TLS обязателен.
- Runner получает opaque bearer credential или short-lived JWT с audience `forge-runner-api`.
- В БД хранится только hash credential и metadata; plaintext показывается ровно один раз.
- Rotation происходит до expiry через authenticated runner endpoint.
- Credential revocation немедленно прекращает poll/renew/log/artifact access.

Предпочтительная production-эволюция:

- mTLS с runner client certificate, выпущенным registration endpoint;
- краткоживущий access token, привязанный к сертификату;
- server certificate pinning или корпоративный CA;
- CRL/revocation для compromised runner.

## 7.3 Heartbeat и inventory

Runner отправляет heartbeat отдельно от lease renewal:

```json
{
  "runnerVersion": "0.1.0",
  "status": "online",
  "draining": false,
  "capacity": { "totalSlots": 4, "busySlots": 1 },
  "capabilities": {
    "executorKinds": ["docker"],
    "os": "linux",
    "arch": "amd64",
    "dockerApiVersion": "1.45",
    "maxCpuMillis": 8000,
    "maxMemoryMiB": 16384
  },
  "tags": ["linux", "docker", "amd64", "trusted-build"],
  "activeLeaseIds": ["..."]
}
```

Control plane хранит нормализованный snapshot capabilities с revision/hash. Runner не может во время heartbeat самовольно добавить protected tag; изменение таких tag требует административного action.

### Runner state machine

```text
registered
  -> online
  -> draining
  -> offline
  -> disabled
  -> revoked

online -> unhealthy       # stale heartbeat / repeated transport failures
unhealthy -> online       # fresh heartbeat
online/draining -> revoked
disabled/revoked -> terminal
```

- `online`: принимает новые assignments.
- `draining`: завершает текущие attempts, новых не получает.
- `unhealthy`: не получает новые leases; active attempts проверяются reconciliation.
- `offline`: heartbeat отсутствует дольше threshold.
- `disabled`: оператор временно исключил runner.
- `revoked`: credential скомпрометирован/удалён; новые запросы запрещены.

`last_seen_at` сам по себе не является source of truth для active execution: статус должен сопоставляться с leases и attempts.

---

## 8. Execution attempt и жизненный цикл job

Логический job описывает node DAG. Каждый запуск создаёт отдельный immutable `execution_attempt`.

### Attempt state machine

```text
created
  -> leasing
  -> assigned
  -> starting
  -> running
  -> uploading
  -> succeeded | failed | canceled | timed_out | lost

leaving states:
leasing  -> abandoned       # ack deadline
assigned -> abandoned       # runner did not start
starting/running/uploading -> cancel_requested -> canceled
starting/running/uploading -> timed_out
starting/running/uploading -> lost
failed/timed_out/lost -> retry_wait -> leasing
```

Терминальные состояния attempts не меняются. Любой retry создаёт новую `attempt_no`.

### Logical job state

`JobStatus` остаётся агрегированной проекцией последней релевантной attempt:

```text
pending -> queued -> leased -> running
running -> success | failed | canceled | skipped
failed/lost/timed_out -> retry_wait -> queued
```

Для совместимости старый `queued/running/success/failed/canceled` API может сохраняться на переходной фазе, но новые статусы должны быть явно отражены в v2/API additions. `retry_wait`, `leased`, `lost`, `timed_out`, `skipped` нельзя скрывать как `running`, если UI/API нужны достоверные diagnostics.

### Pipeline state

```text
created -> planning -> queued -> running
planning -> failed_planning
queued/running -> cancel_requested -> canceled
queued/running -> success | failed
```

`success` только если все обязательные DAG nodes успешны или policy допускает `allow_failure`. `failed` - если обязательный job исчерпал retry policy. `canceled` - если cancellation был причиной незавершённости и policy не зафиксировала независимую обязательную failure.

---

## 9. Docker и Kubernetes execution boundary

## 9.1 Общий контракт executor

Runner process получает `ExecutionSpec`, а backend возвращает `ExecutionHandle`:

```rust
trait ExecutionBackend {
    async fn start(&self, spec: ExecutionSpec) -> Result<ExecutionHandle, ExecutionError>;
    async fn wait(&self, handle: &ExecutionHandle) -> Result<ExecutionResult, ExecutionError>;
    async fn cancel(&self, handle: &ExecutionHandle, grace: Duration) -> Result<(), ExecutionError>;
    async fn inspect(&self, handle: &ExecutionHandle) -> Result<ExecutionObservation, ExecutionError>;
}
```

В `ExecutionSpec` входят только нормализованные данные attempt: commit SHA, image digest/reference, command array, workspace policy, resource limits, declared artifact paths, masked env names и secret injection references.

## 9.2 Docker executor

Docker - первый поддерживаемый backend.

Обязательные ограничения:

- Runner, а не API server, имеет доступ к Docker Engine.
- По умолчанию rootless Docker/отдельный dedicated host.
- Контейнер получает уникальное имя `forge-attempt-<attempt_id>`.
- Никаких Docker socket, host PID namespace, `--privileged`, `--network host`, arbitrary host bind mounts.
- `cap-drop=ALL`; add capabilities только через platform allowlist.
- Read-only root filesystem, writable `tmpfs` и isolated workspace volume.
- CPU, memory, PIDs, disk quota, ulimit и wall-clock timeout.
- Default network policy - deny; egress разрешается только runner pool policy.
- Image pull разрешён только из allowed registries; в production предпочтительны digest-pinned images.
- UID/GID не root, если image позволяет.
- Workspace удаляется после завершения независимо от результата, кроме явно контролируемого diagnostic retention.
- Git credentials передаются только на checkout phase и не остаются в process environment после неё.

`host shell` допускается только для disposable local development. Target production configuration validator обязан отклонять host shell или требовать отдельный явный unsafe-флаг вне стандартного compose.

## 9.3 Kubernetes executor

Kubernetes runner не предоставляет project YAML возможность создать произвольный Pod. Он создаёт Job по жёсткому server-side template:

- namespace выбирается по runner pool / project policy;
- service account ограничен namespace и не имеет cluster-admin;
- Pod Security Admission `restricted`;
- non-root, read-only root FS, dropped Linux capabilities;
- `resources.requests/limits`, `activeDeadlineSeconds`, `ttlSecondsAfterFinished`;
- network policies по умолчанию deny;
- node selector/tolerations только из policy, а не из пользовательского YAML;
- Kubernetes Job UID сохраняется как execution backend reference;
- cancel удаляет Job с foreground propagation и проверяет, что Pod завершён;
- watcher сопоставляет Pod/Job status с attempt, но не заменяет lease fencing.

Docker и Kubernetes должны выдавать одинаковые нормализованные `ExecutionResult`: `exit_code`, `termination_reason`, timestamps, backend reference и diagnostics без секретов.

---

## 10. Секреты: delivery, injection и redaction

## 10.1 Хранение и доступ

Текущий AES-256-GCM encrypted-at-rest механизм остаётся переходной основой. Целевой service должен поддерживать key identifier и ротацию key-encryption key.

- Secret values никогда не входят в pipeline detail, queue row, event payload, audit payload, SQL error или trace.
- Project/environment secret выбирается только по declared key из immutable plan.
- RBAC проверяет изменение secret, использование protected secret и запуск pipeline.
- Attempt сохраняет **имена и version references**, но не plaintext.
- Rotation влияет на будущие attempts; running attempt использует snapshot, полученный на старте.

## 10.2 Выдача секретов runner-у

Assignment не должен содержать secret values. После успешного `ack` owner runner вызывает endpoint наподобие:

```text
POST /api/v1/runner/leases/{lease_id}/secrets:resolve
```

Проверяются runner credential, lease token, expiry, fencing token, project scope и факт, что attempt ещё active. Ответ:

- одноразовый/короткоживущий secret bundle;
- только declared keys;
- optional encrypted transport payload;
- auditable event без значений.

Runner держит plaintext только в памяти и передаёт его execution backend:

- как environment variables для простых values;
- как временные файлы с `0600` для private keys/multiline values;
- временные файлы размещаются вне workspace и удаляются после завершения;
- secret не включается в command-line arguments, labels, container metadata или artifact names.

## 10.3 Redaction

Redaction выполняется **на runner до отправки log chunk**. Control plane применяет защитный второй слой до durable storage.

Правила:

- Masker строится из значений активного secret snapshot.
- Значения сортируются по убыванию длины, empty/слишком короткие значения не маскируются глобальной заменой без специальных правил.
- Поддерживаются boundary-safe варианты: exact, URL-encoded и base64 variants, если это предусмотрено policy.
- Лог chunk может разделить секрет на границе: runner хранит хвост длиной до `max_secret_length - 1`.
- Вместо секрета пишется `***`.
- Redaction не гарантирует скрытие произвольных производных секретов, хешей или намеренно эксфильтрованных данных; policy должна запрещать печать secrets и ограничивать доверие к project code.
- Diagnostic logs runner-а также не должны печатать environment/process command с plaintext values.

---

## 11. Логи и артефакты

## 11.1 Логи

Логи принадлежат `execution_attempt`, а не логическому job. Это сохраняет историю retry.

`attempt_log_chunks`:

| Поле | Назначение |
|---|---|
| `attempt_id` | Владелец |
| `sequence` | Монотонный номер от runner-а |
| `stream` | `stdout`, `stderr`, `system` |
| `payload` | Уже redacted UTF-8/binary-safe content |
| `sha256` | Проверка idempotency/integrity |
| `created_at`, `received_at` | Время runner/control plane |

Protocol:

- `POST /leases/{id}/logs` содержит `(attempt_id, fencing_token, sequence, chunks[])`.
- Повтор отправки того же `(attempt_id, sequence, sha256)` возвращает success идемпотентно.
- Тот же sequence с другим hash возвращает conflict и security event.
- API принимает bounded payload; runner делает batching/flush по размеру и интервалу.
- UI использует cursor `after_sequence`, SSE/WebSocket может быть добавлен как read projection.
- Для больших logs используется chunk compression/object storage, но индекс и cursor остаются в PostgreSQL.
- `MAX(sequence)+1` на сервере не используется для distributed writers.

## 11.2 Артефакты

Артефакт также принадлежит attempt. В target schema:

- `artifact_manifest`: logical artifact name/path, checksum, content type, size, retention, status.
- `artifact_upload`: multipart/pre-signed upload session, expiry, owner attempt.
- `artifact_object`: storage backend/key, checksum, finalization timestamp.

Поток:

1. Runner ищет только declared artifact paths внутри workspace после завершения commands.
2. Runner создаёт manifest с SHA-256 и размером.
3. API проверяет lease ownership, path policy, quota и attempt state.
4. API возвращает presigned URL либо stream upload endpoint.
5. Runner загружает object.
6. Runner вызывает finalize; storage checksum/size проверяются.
7. Artifact становится `available`; failed/incomplete uploads очищает retention worker.

Ограничения:

- Artifact path не может выйти за workspace (`..`, symlink escape проверяются).
- Лимиты на файл, total attempt, project quota и число artifacts.
- `download` проходит RBAC и выдаёт short-lived signed URL либо проксируется.
- Retention worker использует state `deleting`, затем удаляет object и metadata с reconciliation для orphan objects.
- Local FS допустима для single-node/dev; production использует S3-compatible backend.

---

## 12. Cancellation, retry, timeouts и reconciliation

## 12.1 Cancellation

Отмена pipeline/job - intent, который должен переживать сетевой сбой:

1. API проверяет RBAC и переводит pipeline/job в `cancel_requested`.
2. Scheduler отменяет `ready` queue rows без выдачи lease.
3. Для active lease создаётся durable `cancel_request`.
4. Runner получает cancel через следующий poll/renew/explicit endpoint.
5. Runner посылает graceful cancellation executor-у.
6. После grace period runner делает forced termination, если backend ещё жив.
7. Runner сообщает terminal result `canceled`.
8. Reconciler завершает зависимые jobs как `skipped` или `canceled` согласно policy.

API не должен объявлять job окончательно `canceled` только потому, что локально отправил signal. Terminal transition подтверждается attempt result либо reconciliation после проверяемого backend observation.

## 12.2 Retry

Нужно разделять:

- **Transport retry**: повтор HTTP log/upload/renew request с тем же idempotency key; не создаёт новую attempt.
- **Lease delivery retry**: lease не acknowledged до deadline; старая attempt `abandoned`, queue item возвращается в `ready`.
- **Execution retry**: новая attempt после terminal `failed`, `lost`, `timed_out` или policy-allowed infrastructure error.

Retry policy входит в immutable plan и может ограничиваться project policy:

```yaml
retry:
  max_attempts: 3
  retry_on: [runner_lost, infrastructure, timeout]
  backoff: exponential
  max_backoff: 5m
```

По умолчанию не повторяются configuration/validation errors, exit code проекта, policy denial, cancellation и stale lease. Retry application должен учитывать idempotency risk deploy jobs: для deployment-пула default `max_attempts=1`, пока job явно не объявлен безопасным для retry.

## 12.3 Таймауты

| Таймаут | Объект | Действие |
|---|---|---|
| `queue_timeout` | Job слишком долго не назначается | `failed`/`timed_out` с diagnostic `no compatible runner` |
| `lease_ack_timeout` | Lease | `abandoned`, release queue |
| `lease_ttl` | Active lease | `expired`; reconciliation |
| `startup_timeout` | Container/Pod не перешёл в running | cancel backend, `failed`/`timed_out` |
| `execution_timeout` | Attempt | `cancel_requested`, forced termination после grace |
| `idle_log_timeout` | Long-running job | warning/reconcile; не fail по умолчанию |
| `cancel_grace_period` | Executor | forced termination |
| `artifact_upload_timeout` | Artifact session | abort/cleanup |

Все deadlines фиксируются на уровне attempt и audit/log event содержит deadline reason.

## 12.4 Reconciliation workers

Reconciliation - обязательный server worker, запускаемый на старте и периодически:

- `lease_expiry_reconciler`: находит истекшие leases, fencing old owner, переводит attempt в `lost`/`abandoned`, планирует retry по policy.
- `runner_health_reconciler`: stale heartbeat → `unhealthy/offline`, прекращает assignment.
- `attempt_reconciler`: запрашивает executor observation через runner или backend-specific adapter, если completion отсутствует.
- `queue_reconciler`: восстанавливает missing queue row для eligible job и убирает queue rows canceled/terminal jobs.
- `pipeline_aggregator`: пересчитывает job → pipeline статус из durable attempts/DAG.
- `artifact_reconciler`: удаляет expired/incomplete upload и помечает object mismatch.
- `outbox_relay`: публикует audit/domain events после commit.

Reconciler должен быть идемпотентен, работать lock-safe и не держать DB transaction во время network I/O.

---

## 13. Целевая data model

Новые таблицы вводятся SQLx versioned migrations; production bootstrap должен разделять apply/verify migration modes, а legacy `store::migrate()` adoption нужен только для старых инсталляций.

### Основные сущности

| Таблица | Назначение |
|---|---|
| `pipeline_definitions` | Config source, commit SHA, raw YAML, parser version, config hash |
| `pipelines` | Экземпляр запуска и агрегированный status |
| `execution_plans` | Canonical normalized plan JSON и plan hash |
| `planned_jobs` | Immutable job nodes, requirements, policy snapshot |
| `planned_job_dependencies` | DAG edges |
| `jobs` | Mutable runtime projection logical job |
| `execution_attempts` | Неизменяемая история каждого запуска job |
| `job_queue` | Current dispatch ledger: queued/leased/completed/canceled row на current attempt, `SKIP LOCKED` claim, open-row uniqueness и terminal cleanup |
| `job_leases` | Current lease ledger: active/completed/expired/canceled, generation, expiry, external lease token hash, ack deadline, ack/renew/logs/complete и protocol version; target расширяет lost-heartbeat policy/full protocol data planes |
| `runners` | Identity, scope, status, drain/revocation |
| `runner_credentials` | Credential hashes, expiry, rotation/revocation |
| `runner_capability_snapshots` | Inventory revision/history |
| `runner_heartbeats` | Последние и/или агрегированные telemetry samples |
| `attempt_log_chunks` | Append-only logs по attempt |
| `artifact_manifests` | Metadata и status artifacts |
| `artifact_uploads` | Staged sessions |
| `attempt_secret_refs` | Только secret keys/version refs, без plaintext |
| `cancel_requests` | Durable intent и delivery status |
| `outbox_messages` | Transactional outbox |
| `audit_log` | Security and operator actions |

### Ключевые constraints

- `UNIQUE (pipeline_id, logical_job_key)` для planned job.
- `UNIQUE (attempt_id, sequence)` для logs.
- Partial unique index: не более одной active lease на job:
  `UNIQUE(job_id) WHERE lease_status IN ('offered','acknowledged','active')`.
- `UNIQUE (runner_id, credential_hash)` и credential status/revocation.
- FK artifact → attempt, attempt → job, job → pipeline через stage.
- Queue item может быть только для non-terminal job.
- Foreign key/validation запрещает attempt completion с lease другого runner-а.
- Fencing token проверяется во всех mutating runner endpoints.

---

## 14. API и runner protocol

Версия runner protocol отделяется от user-facing REST: `/api/v1/runner/...`. Все тела документируются OpenAPI/JSON schema и имеют explicit `protocolVersion`. Current MVP реализует register/heartbeat/immediate poll/ack/`secrets:resolve`/renew/logs/complete, `workspace.checkoutUrl`, declared `attempt.secrets` и `forge-runner` shell binary; остальные строки в таблицах ниже являются target contract.

### Control-plane API

| Метод | Путь | Назначение |
|---|---|---|
| `POST` | `/api/v1/runner-registration-tokens` | Создать bootstrap token |
| `GET` | `/api/v1/runners` | Список runner-ов и health |
| `PATCH` | `/api/v1/runners/{id}` | Drain/disable/tags/scopes по RBAC |
| `POST` | `/api/v1/pipelines/{id}/cancel` | Запросить отмену |
| `POST` | `/api/v1/jobs/{id}/retry` | Создать новую attempt при policy/RBAC |
| `GET` | `/api/v1/pipelines/{id}` | Pipeline + DAG + attempts projection |
| `GET` | `/api/v1/attempts/{id}/logs?after_sequence=` | Cursor logs |
| `GET` | `/api/v1/attempts/{id}/artifacts` | Artifacts attempt |

### Runner API

| Метод | Путь | Статус | Назначение |
|---|---|---|---|
| `POST` | `/api/v1/runner/register` | Current MVP | Registration через `CICD_RUNNER_REGISTRATION_TOKEN`; credential возвращается один раз |
| `POST` | `/api/v1/runner/credentials:rotate` | Target | Credential rotation |
| `POST` | `/api/v1/runner/heartbeat` | Current MVP | Liveness, capacity, inventory |
| `POST` | `/api/v1/runner/work:poll` | Current MVP | Immediate poll for compatible LeaseOffer with `required_tags ⊆ runner.tags`; long-poll target |
| `POST` | `/api/v1/runner/leases/{id}/ack` | Current MVP | Подтвердить lease |
| `POST` | `/api/v1/runner/leases/{id}/renew` | Current MVP | Продлить active lease |
| `POST` | `/api/v1/runner/leases/{id}/secrets:resolve` | Current MVP | Получить declared secret bundle после ack lease |
| `POST` | `/api/v1/runner/leases/{id}/logs` | Current MVP | Server-sequenced stdout/stderr/system log lines; idempotent chunks target |
| `POST` | `/api/v1/runner/leases/{id}/artifacts:init` | Target | Создать artifact upload |
| `POST` | `/api/v1/runner/leases/{id}/artifacts:finalize` | Target | Подтвердить artifact |
| `POST` | `/api/v1/runner/leases/{id}/complete` | Current MVP | Подтверждённый result attempt |
| `GET` | `/api/v1/runner/leases/{id}/control` | Target | Cancel/drain control signal |

### Пример LeaseOffer

```json
{
  "protocolVersion": 1,
  "leaseId": "9c8b...",
  "fencingToken": 42,
  "ackDeadline": "2026-08-26T12:00:30Z",
  "leaseExpiresAt": "2026-08-26T12:02:00Z",
  "attempt": {
    "id": "a7f1...",
    "number": 2,
    "pipelineId": "p1...",
    "jobId": "j1...",
    "jobKey": "test",
    "commitSha": "0123456789abcdef...",
    "executor": "shell",
    "image": "rust@sha256:...",
    "commands": ["cargo test"],
    "environment": {},
    "secrets": ["DEPLOY_TOKEN"],
    "timeoutSeconds": 900,
    "workspace": {
      "checkout": true,
      "checkoutUrl": "https://forge.example/git/project.git"
    },
    "artifacts": []
  }
}
```

В ответе нет plaintext secret values, project master credentials, DB URL, artifact storage credentials или Docker socket information. Current `forge-runner` использует `checkoutUrl`, после ack получает declared secret bundle, выполняет shell commands и загружает stdout/stderr через protocol log append; artifacts пока не загружаются protocol-ом.

### Ошибки protocol

- `401`: credential invalid/revoked.
- `403`: runner scope/tag/pool forbidden.
- `409`: stale lease/fencing token/sequence conflict.
- `410`: lease expired.
- `422`: malformed capability/inventory/log/artifact declaration.
- `429`: poll/heartbeat rate limit.
- `503`: control plane transiently unavailable; runner применяет backoff.

---

## 15. Наблюдаемость и аудит

Обязательные structured fields:

- `request_id`, `pipeline_id`, `job_id`, `attempt_id`, `lease_id`, `runner_id`;
- `fencing_token`, `attempt_no`, `executor_kind`;
- `duration_ms`, timeout/retry reason;
- безопасные result category и exit code.

Метрики:

- queue depth/age по tags, project, pool;
- active/expired leases;
- runner online/unhealthy/draining/offline;
- dispatch latency и assignment failure reason;
- attempts by terminal status/retry count;
- execution duration, timeout, cancel completion time;
- log ingest lag/rejections;
- artifact upload bytes/errors;
- secret resolution/redaction counters без значений;
- reconciliation action count;
- stale lease/fencing rejection count.

Audit события:

- registration token create/revoke/use;
- runner registration/disable/drain/revoke;
- protected tag/pool policy changes;
- secret create/rotate/delete/use by key name;
- manual retry/cancel;
- forced attempt termination;
- artifact deletion/retention cleanup.

---

## 16. Стратегия тестирования

## 16.1 Domain unit/property tests

- Parser accepts supported DSL and produces canonical plan hash.
- Parser rejects malformed YAML, unknown schema version, unsafe image/path, duplicate jobs, invalid secret key.
- DAG topological ordering, disconnected components, cycles, `allow_failure`.
- Eligibility: dependencies, tags, capability matching, concurrency, retry deadline.
- State machines: all valid/invalid transitions for pipeline/job/attempt/lease/runner.
- Retry classification and backoff with deterministic clock.
- Aggregation of DAG outcomes into pipeline status.
- Redaction including secrets split across log chunks.
- Artifact workspace path validation and symlink escape defense.

## 16.2 PostgreSQL integration tests

Поднимаются в isolated PostgreSQL с versioned migrations:

- Concurrent poll из нескольких server instances выдаёт единственную active lease.
- `SKIP LOCKED` не блокирует разные queue rows.
- Lease ack/renew/expiry, stale completion и fencing rejection.
- Restart server между offer и ack.
- Runner heartbeat expiry меняет eligibility.
- Retry создаёт новую attempt и сохраняет старые logs/artifacts.
- Cancellation race: completion vs cancel, lease expiry vs completion.
- Queue reconciliation восстанавливает lost queue row.
- Transactional outbox не теряет committed event.
- RBAC/project scope не позволяет runner-у получить чужой job/secret/artifact.
- Log duplicate sequence идемпотентен, mismatch конфликтует.
- Artifact finalize требует owner lease и проверяет checksum.

## 16.3 Protocol и runner contract tests

- Generated/OpenAPI client compatibility для текущей и предыдущей runner protocol версии.
- Registration token single-use/expiry/max usage.
- Credential rotation/revocation.
- Poll long-poll cancellation и backoff.
- Network interruption до/после `ack`, log batch retry, result retry.
- Runner обнаруживает `409/410` и прекращает stale execution.
- Clock skew within configured tolerance.

## 16.4 Executor integration tests

Docker:

- `alpine` command success/failure.
- Timeout/cancel действительно останавливает container.
- Нет сети по default.
- Нет Docker socket/host mount/privileged capability.
- Memory/PID/resource constraints применяются.
- Secrets доступны process, отсутствуют в durable logs.
- Artifact upload/download checksum совпадает.

Kubernetes, при наличии test cluster:

- Job получает resource limits, non-root policy, namespace/service account.
- Cancel удаляет Job/Pod.
- Deadline/result корректно переводятся в attempt status.
- Pod watcher/reconciliation не создаёт duplicate completion.

## 16.5 End-to-end и fault injection

- Project → push/trigger → DAG view → external runner → logs → artifact → final success.
- Нет подходящего runner: queue timeout и понятный diagnostic.
- Runner crash в execution: lease expiry, new attempt на другом runner-е.
- API restart во время active runs.
- Network partition runner/control plane.
- Secret appears in stdout/stderr: UI и DB log содержат только `***`.
- Cancel pipeline с queued, leased и running nodes.
- Retry policy для build и non-idempotent deploy.
- UI browser flow на 375, 1920 и 2560 px.

---

## 17. Поэтапная поставка

## Фаза 0 - фиксация контрактов и baseline

- Зафиксировать ADR: runner protocol, lease/fencing, execution sandbox, artifact ownership.
- Сохранять SQLx migration baseline и не вводить schema changes без migration/verify gate.
- Документировать current embedded runner как transitional/unsafe for production.
- Добавить real-PostgreSQL harness.
- Не менять публичные v1 routes без compatibility strategy.

**Gate:** workspace checks, migration apply на пустой DB, существующие API/CLI контракты не регрессируют.

## Фаза 1 - parser и planning DAG без внешнего runner-а

- Вынести parser из `api.rs` в domain/app.
- Current MVP: поддержать commit SHA pinning where available и immutable `pipeline_plans` snapshot для legacy-linear и v1 DAG plan.
- Target follow-up: parser diagnostics, `planned_jobs`/dependencies, job-level dispatcher и DAG visualization API.
- Использовать compatibility executor только для local development.
- Перестать silently fallback на deploy template в production config mode.

**Gate:** parser/DAG/property tests, real DB pipeline plan persistence, invalid config не создаёт executable queue jobs.

## Фаза 2 - durable queue, attempts и leases

- Current MVP: `job_queue` материализует dispatch row для current queued attempt, поддерживает claim через `SKIP LOCKED`, basic tag matching и terminal/cancel cleanup.
- Current MVP: `job_leases` фиксирует embedded runner claim, generation, expiry и terminal close.
- Current MVP: external runner protocol создаёт lease offer, проверяет lease token/fencing, поддерживает ack/renew/logs/complete, heartbeat и отдаёт `workspace.checkoutUrl`.
- Расширить `job_queue` до pool/protected-tag/capability matching, priorities/backoff и observability.
- Расширить lease expiry/reconciliation до lost-heartbeat/cancel races и long-poll/event wakeup.
- Сохранить текущую retry-модель с новой attempt и добавить policy/fencing поверх неё.
- Добавить reconciliation/outbox workers.
- Сохранить UI projection старых status полей как compatibility read model.

**Gate:** concurrent lease tests, restart/expiry/cancellation race tests, no duplicate active execution.

## Фаза 3 - внешний Docker runner

- Current MVP: создать `forge-runner` shell binary поверх существующего registration/auth/heartbeat protocol MVP.
- Подключить production-grade heartbeat, capabilities, tags, capacity и drain/revoke policy.
- Перенести Docker socket/commands/workspace lifecycle из `cicd-server` в runner.
- Удалить production host-shell execution.
- Реализовать end-to-end Docker runner на isolated host.

**Gate:** API container не имеет Docker socket; runner e2e `alpine echo`, timeout, cancel, lease recovery проходят.

## Фаза 4 - secrets, logs и artifacts

- Довести current secret resolve до KMS/rotation/environment policy, attempt secret refs и dual-layer redaction во всех каналах.
- Мигрировать logs на attempt chunks/idempotency.
- Мигрировать artifacts на attempt manifests/checksum/staged upload.
- Добавить S3-compatible backend и retention workers.
- Проверить RBAC и audit события.

**Gate:** injected secret не присутствует в API response, DB logs, UI, traces и artifact metadata; retry не удаляет старую историю.

## Фаза 5 - Kubernetes, hardening и rollout

- Реализовать Kubernetes executor за общим `ExecutionBackend` port.
- Ввести protected runner pools/tags, network/security policies и resource quotas.
- Добавить metrics, alerts, runbook, backup/restore и chaos tests.
- Запустить shadow/non-blocking Forge pipeline параллельно GitHub Actions.
- Сравнить результаты, duration, retry и failure behavior.
- Сделать Forge required check только после acceptance criteria и operational sign-off.

**Gate:** Docker и Kubernetes backends проходят contract suite; operational drill подтверждает recovery после runner/API/DB failure.

---

## 18. Критерии готовности production execution

Forge можно называть distributed CI/CD execution platform только когда одновременно выполнены условия:

- API/control-plane не запускает пользовательские команды и не имеет Docker socket.
- Внешний runner зарегистрирован через ограниченный bootstrap token и аутентифицируется постоянным rotation-capable credential.
- Dispatcher учитывает runner scope, protected tags, capabilities, capacity и healthy heartbeat.
- Каждая работа имеет durable queue row, lease, fencing token и immutable execution attempt.
- Потеря runner-а не приводит к бесконечному `running` и не допускает stale completion.
- Retry не уничтожает логи/артефакты предыдущей попытки.
- Secrets инжектируются только owner runner-у, маскируются до persistence и не возвращаются API.
- Docker/Kubernetes isolation и cancellation/timeouts подтверждены integration tests.
- Logs/artifacts имеют attempt ownership, checksum/idempotency/retention.
- Real PostgreSQL, runner protocol, e2e и failure/reconciliation tests включены в CI.
- Документация `ARCHITECTURE`, `API`, `DATA_MODEL`, `SECURITY`, `RESILIENCE`, `ARTIFACTS`, `DEPLOYMENT` и `OPS_RUNBOOK` синхронизирована с реально поставленной фазой.
