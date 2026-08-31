# Roadmap — Forge CI/CD

## 1. Назначение

Roadmap фиксирует порядок доведения Forge до базового self-hosted CI/CD control plane. Он не является списком всех возможных идей: в базовый продукт попадает только то, без чего система небезопасна, непонятна в диагностике или ненадёжна в эксплуатации.

Фактическое состояние возможностей хранится в `docs/CURRENT_STATE.md`, требования — в `docs/PRODUCT_REQUIREMENTS.md`, доказательства и проверки — в `docs/TRACEABILITY.md`.

## 2. Принципы приоритизации

1. Сначала сохраняем доказательства выполнения: попытки, логи, артефакты, статусы и аудит не должны затираться при retry или restart.
2. Затем закрываем доступ: auth, project-level RBAC, token/session revoke и безопасные defaults важнее новых интеграций.
3. Потом делаем доставку наблюдаемой: webhooks, notifications, scheduler и deployments должны иметь history, retry и понятный outcome.
4. Любая capability считается готовой только после кода, миграций, тестов, UI/API/CLI обновлений и документации.
5. Configuration-only экраны не выдаются за рабочую automation.

## 3. Current baseline

На 2026-08-31 в коде уже есть:

- проекты, встроенный bare Git hosting, Smart HTTP и auto-trigger pipeline на push;
- pipeline из `.forge-ci.yml` с линейными stages/jobs, fallback-шаблон, manual jobs, basic timeout и `allow_failure`;
- embedded runner в `cicd-server`: Docker/host shell, stdout/stderr в attempt-owned `job_logs`, cancel/retry;
- `execution_attempts`: каждая job получает initial attempt, retry job/pipeline создаёт новую attempt и сохраняет старые логи;
- REST logs, attempts API и SSE stream логов job;
- local artifacts до 50 MiB с metadata текущей/latest attempt и SHA-256 для новых uploads;
- project secrets: AES-256-GCM at rest, env injection в embedded runner, best-effort masking stdout/stderr;
- environments/deployments metadata, reports, audit log;
- users, roles, argon2id credentials, session-bound access JWT, sessions, refresh rotate/logout/revoke, scoped PAT и project memberships при `CICD_AUTH_SECRET`;
- schedules MVP, outgoing webhooks через outbox с basic retry/HMAC, bounded delivery history/requeue и `in_app`/`sse` notifications через local outbox history/stream;
- OpenAPI generation/drift gate и generated frontend schema.

Ключевые ограничения current baseline:

- execution attempts реализованы как MVP-слой без внешних leases/fencing; bounded log pagination/search уже есть, command spans и richer error diagnostics остаются Phase 1 follow-up;
- auth/RBAC имеет project membership, scoped PAT, Git Smart HTTP read/write checks, session-bound access invalidation и refresh rotate/logout/revoke MVP, но без tenant isolation, service-account tokens, scoped Git credentials и production cookie/CSRF/session-family policy;
- execution встроен в backend process, поэтому shared/prod режим требует external runner boundary;
- webhooks/notifications имеют bounded delivery history/requeue MVP, но без production leases/fencing, full dead-letter policy/metrics и external adapters; email/Slack notification adapters не реализованы;
- backup/restore имеет local scripted MVP helper и CI self-test/dry-run, но ещё не является off-site/PITR production platform или verified restore drill gate.

## 4. Базовый roadmap

### Phase 1 — Execution attempts и диагностические логи

**Цель:** retry не затирает историю, а пользователь понимает причину падения без чтения неструктурированной простыни.

Deliverables:

- Current MVP: таблицы `execution_attempts` и attempt-owned log/artifact metadata;
- Current MVP: retry pipeline/job создаёт новую attempt, старые логи и timestamps остаются доступными;
- Current MVP: API, UI и CLI показывают attempts и логи выбранной attempt;
- Current MVP: bounded `/logs/page` для current/latest и concrete attempt с `limit/after/q`; UI читает логи страницами и поддерживает поиск;
- Target follow-up: лог делится минимум на command span, stream, exit code, started/finished timestamps и error tail;
- Target follow-up: richer empty/error states и отдельные UI tests переключения attempts.

Gate:

- real PostgreSQL migration test для retry без удаления старых логов;
- API/OpenAPI/CLI contract test на attempts/log ordering и bounded log page/search;
- UI screenshot smoke для переключения attempts; dedicated component/e2e test остаётся follow-up;
- документация `API.md`, `DATA_MODEL.md`, `USER_GUIDE.md`, `CURRENT_STATE.md`.

### Phase 2 — Production auth и project RBAC

**Цель:** shared-инстанс можно отдавать команде без trusted-network допущения.

Deliverables:

- Current MVP: `project_memberships`, project members API/UI, list filtering и deny-before-load для project-owned routes;
- Current MVP: refresh session rotation and logout/revoke, включая фиксацию `revoked_at` и немедленную инвалидизацию session-bound access JWT;
- Current MVP: Git Smart HTTP read/write checks через legacy `CICD_GIT_TOKEN` либо JWT/PAT + project membership по связанному repository URL;
- Current MVP: scoped PAT с обязательным `project_id`, scopes `api:read|api:write|git:read|git:write`, expiry/revoke/last-used и без глобального доступа по умолчанию;
- Target follow-up: refresh-cookie, CSRF и token-version/session-family reuse policy;
- Target follow-up: service-account tokens, scoped Git credentials, tenant-bound repository mapping и delivery routes с repository/project binding;
- CORS/CSRF policy для production deployment.

Gate:

- negative auth/RBAC suite: viewer не меняет project, secret, pipeline, deploy и policy;
- disabled user/revoked token не проходит ни один protected route;
- route-policy inventory test не позволяет добавить endpoint без политики;
- UI скрывает или блокирует forbidden actions и показывает понятный 403.

### Phase 3 — Надёжные webhooks, notifications и scheduler

**Цель:** автоматизация либо реально доставляет результат, либо честно показывает failure.

Deliverables:

- Current MVP: delivery history для outgoing webhooks/local notifications: attempts, status, HTTP code/error class, `failed_at`, next retry и явный requeue failed-доставки новой generation;
- Target follow-up: production lease/fencing/crash recovery, response preview allowlist, full replay/dead-letter workflow и operator metrics;
- Current MVP: `in_app`/`sse` notification delivery на terminal pipeline events через local outbox projection;
- Target follow-up: email/Slack adapters, notification templates, preferences и aggregation;
- full cron semantics с timezone и next-run preview;
- incoming provider webhook handlers только после auth/signature validation.

Gate:

- outbox integration tests на retry, failed history, requeue generation и idempotency; target дополнительно покрывает leases/crash retry/single observed outcome;
- scheduler tests на cron/timezone/no-double-fire;
- UI показывает последний outcome и ошибку доставки;
- metrics и runbook для stuck deliveries.

### Phase 4 — Backup/restore и operational recovery

**Цель:** self-hosted установка не теряет PostgreSQL, Git repositories и artifacts без проверяемого восстановления.

Deliverables:

- Current MVP: `scripts/backup.sh`/`scripts/verify-backup.sh` для PostgreSQL, Git storage и artifacts;
- Current MVP: `scripts/restore.sh` с checksum consistency checks и guarded `--confirm-restore`;
- Target follow-up: documented RPO/RTO measurement и restore drill evidence;
- startup/reconciliation checks после restart.

Gate:

- Current gate: backup helper self-test/dry-run в CI; target gate: automated restore drill на disposable окружении;
- post-restore проверка проектов, pipeline history, Git refs и artifact metadata;
- runbook в `OPERATIONS.md` и troubleshooting для типовых failures.

### Phase 5 — External runner, durable queue и leases

**Цель:** пользовательский код не выполняется внутри control plane.

Deliverables:

- отдельный runner binary/process;
- durable `job_queue`, claim/ack/renew/expire leases и fencing token;
- runner registration credentials, heartbeat, tags/capacity/drain;
- cancel/timeout/reconciliation для потерянного runner-а;
- secret delivery только owner-у lease.

Gate:

- lost heartbeat/lease expiry/cancel race tests;
- no duplicate active execution invariant;
- Docker smoke на isolated runner host;
- API container не требует Docker socket.

### Phase 6 — Artifact retention и object storage

**Цель:** артефакты управляемы по сроку, размеру и месту хранения.

Deliverables:

- retention/TTL policies и cleanup worker;
- checksum reconciliation и backfill digest для legacy metadata;
- S3-compatible object storage adapter с tenant/project isolation;
- project-scoped access checks для metadata и bytes.

Gate:

- upload/download/size/retention integration tests;
- object-storage adapter smoke;
- restore drill учитывает artifacts.

### Phase 7 — Delivery environments

**Цель:** Forge поддерживает CD-сценарии без превращения в инфраструктурный orchestrator.

Deliverables:

- protected environments;
- approval gates для production-like окружений;
- deploy history с actor, pipeline/ref, status и evidence;
- rollback запускается как отдельный traceable pipeline/action.

Gate:

- approval policy tests;
- audit trail для approval/deploy/rollback;
- UI не позволяет обойти required approval.

## 5. Later, не базовый слой

Эти функции полезны, но не должны раздувать текущий baseline:

- flaky test tracking и quarantine;
- dependency/security scans, SBOM и license gates;
- DORA/advanced reports, percentiles, dashboards и exports;
- matrix builds и сложный DAG planner сверх базового immutable plan;
- SSO/OIDC;
- Kubernetes runner adapter;
- full code-review platform, threaded comments, merge queues;
- package/container registry;
- issue tracker, IDE или browser terminal.

## 6. Definition of Done

Фаза считается завершённой только если:

- код и миграции реализованы без сохранения legacy-обмана в UI/API;
- `cargo fmt`, `cargo clippy`, backend tests и relevant integration tests зелёные;
- `pnpm test`, `pnpm build` и затронутые UI tests зелёные;
- OpenAPI и generated frontend schema обновлены при изменении API;
- `docs/PRODUCT_REQUIREMENTS.md`, `docs/TRACEABILITY.md`, `docs/API.md`, `docs/DATA_MODEL.md`, `docs/USER_GUIDE.md` и `docs/CURRENT_STATE.md` синхронизированы;
- для UI-изменений обновлены screenshots/evidence;
- `python3 scripts/verify_docs.py --all` проходит.

## 7. References

- `docs/CURRENT_STATE.md` — фактические возможности и ограничения.
- `docs/PRODUCT_REQUIREMENTS.md` — требования и out-of-scope.
- `docs/TRACEABILITY.md` — REQ-ID, проверки и evidence.
- `docs/ARCHITECTURE_INDEX.md` — карта архитектурных документов.
- `docs/contracts/` — нормативные целевые контракты.
