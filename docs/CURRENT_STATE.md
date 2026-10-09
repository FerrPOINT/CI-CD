# CURRENT STATE — Forge CI/CD

> **Производный снимок текущего состояния.** Сверяется с кодом и evidence; authority — код и коммит, не этот файл. Обновлять при каждом изменении capability.
> Снято: `2026-10-08`, candidate `feat/task-delivery-evidence-20261007`, не установленный runtime; visual evidence перечислено в `docs/assets/screens/manifest.md`.

## Что работает сейчас (Current verified)

### Непринятое hardening reader fence и OCI recovery, 2026-10-08

Последний завершённый frozen packet `0f560932eb2b`: PG3 PASS/0 FAIL
(все пять SIGKILL checkpoints), OCI0 PASS/1 FAIL на исходном runner completion
timeout.37 Python cases, row/SQL safety smoke, locked check и strict Clippy прошли;
259-input source parity и exact own cleanup подтверждены. Full follow-ups не
запускались. DNS/BuildKit session ошибки в том же окне требуют проверки offline
preloaded-image path, не увеличения таймаутов. Старый failed8f596 packet сохранён:
его backup rehearsal subprocess cause остаётся неизвестным. Подробности и hashes:
[latest packet](TASK_DELIVERY_VERIFICATION.md#latest-frozen-native-packet-8-октября).

После56f1217 найдены gaps PostgreSQL reader grants/snapshot inventory и OCI
reconciliation после immutable checks. Source исправления и новые regressions
описаны в [verification](TASK_DELIVERY_VERIFICATION.md#hardening-reader-fence-и-oci-recovery-2026-10-08).
Первый packet прошёл10 Windows/WSL pure Python cases; свежий WSL/rootless Rust1.88 gate
прошёл fmt/all-targets/all-features check/Clippy и219 workspace tests, включая4
OCI recovery units. Scoped259 source hashes повторно сверены, cleanup complete.
Последующее review исправляет writer history-view/rewrite/inheritance/FK routes,
проверку historical PG proof и допустимые released writer sessions. Historical
Windows26 pure cases и aggregate32 docs/tests PASS; native tests расширены.
Свежий immutable epoch d92019b98cb3 проходит22 Linux pure cases, actual PG17.11
SQL smoke и locked/offline feature check/strict Clippy; parent сверил259 hashes,
0 mismatches. Исторический epoch4e95ebcac21f завершился PG2/3 FAILED на Compose
stop timeout, а не успешным drain; explicit5s stop grace не увеличивает20s deadline
и не повторяет Unknown. Отдельный d920 epoch завершился PG3/3 PASS2795.37s и OCI1/1
PASS90.73s, но workspace FAILED137/1 на stat-cache regression; error route ещё
не диагностирован. Exact own cleanup complete,20 baseline volumes сохранены.
Последующий counter-review доказал numeric row hash collision. Current source
использует lossless PostgreSQL record-text/strings, а не jsonb/float normalization;
чистый production fingerprint RED repro стал GREEN. Новый native/normal/release
gate для этого source ещё не принят. Прежние runtime PASS ниже не подтверждают
lossless fix. PR88 Draft и full SDLC blocked.
Documentation-only publication не включает локальные hardening source edits;
новый docs head не заменяет code/native acceptance.
Последующий local OCI fixture guard проходит11 pure cases и fresh locked
check/Clippy на260 inputs, но separate diagnostic4a94b411e888 падает на
preparation до build (0/1). Его причина ещё неизвестна, cleanup/parity complete;
PostgreSQL/full gates в нём не выполнялись. Guard остаётся unpublished,
documentation headd51c260 имеет четыре CI SUCCESS, не native guard acceptance.
Свежий diagnostic e6e304554e4d проходит offline preparation, locked check и
strict Clippy, но actual OCI0/1 снова падает на исходном runner completion
timeout. Current/frozen260 parity и exact own cleanup подтверждены; PG/full
gates в нём не выполнялись. Причина pipeline ещё не доказана;
[отдельная запись](TASK_DELIVERY_VERIFICATION.md#offline-oci-runner-diagnostic-e6e304554e4d).
Следующий exact-source diagnostic a7c3dc379a3c проходит actual OCI1/1,
включая real runner/artifact/image/data/rollback и оба SIGKILL recovery cases,
при неизменных deadlines.260-input parity и exact cleanup подтверждены.
Это scoped owner-local acceptance, не полный native/SDLC gate; PG и full
follow-ups в нём не выполнялись.20 pure OCI tests PASS Windows/Linux.
[Текущая OCI-приёмка](TASK_DELIVERY_VERIFICATION.md#offline-oci-actual-acceptance-a7c3dc379a3c)
не объясняет однозначно исторические failed packets и не разрешает source rollout.

### PostgreSQL shadow delivery source continuation, 2026-10-08

`forge-delivery --postgres` добавляет отдельный privileged isolated protocol:
exact artifact/image/schema/catalog, enforced writer drain/fence, verified backup
restore drill, fresh shadow migration и отдельные application checks перед release.
Known pre-release failure допускает verified snapshot + previous image restore в
fresh DB с сохранением rows/sequences. После release old snapshot restore запрещён.
Original-key replay не повторяет commands; partial Unknown удерживает target,
completed-release reconcile сохраняет post-release writes и container identity.
Исторические source gates опубликованного56f1217: PG suite3/3 и OCI1/1 PASS;
они не подтверждают последующий hardening. Evidence и пределы:
[task verification](TASK_DELIVERY_VERIFICATION.md), [ADR-0022](adr/0022-owner-local-postgres-shadow-delivery.md).
RPO=0 только для acknowledged pre-drain writes. Production/native SDLC admission
и full business acceptance недоступны; HTTP dispatch503/оба SDLC flags false.
Новый Forge SQL migration не добавлен,0039/0040 и Base pin сохранены.
PR87 ещё OPEN: текущий PR88 diff к main включает prerequisite0039 и own0040.
До release-ready нужен merge PR87 и повторный final diff/gate, где единственная
новая Forge migration —0040. Удаление prerequisite или force push не допускаются;
owner-local verification и исторические CI не закрывают этот release gate.

### OCI/read-only data source continuation, 2026-10-08

`forge-delivery --oci` добавляет bounded temporary Compose deployment preloaded
image и read-only data snapshot, actual image/commit/container/served identity,
application compatibility/acceptance и rollback без изменения snapshot. Mutable
DB/migration/restore и authoritative SDLC admission закрыты. Source/gates и пределы:
[task verification](TASK_DELIVERY_VERIFICATION.md), [ADR-0021](adr/0021-owner-local-oci-readonly-data.md).

### Owner-local manifest delivery source, 2026-10-08

`forge-delivery` добавляет bounded static-artifact publication в отдельный Unix
target: immutable intent, original-key replay, actual served manifest/bytes,
health/acceptance checks, last-confirmed rollback и crash reconciliation без
повторной publication. Это privileged local verification, не production install
или SDLC admission. HTTP SDLC POST остаётся503; machine-authenticated GET читает
history. Evidence и ограничения: [task verification](TASK_DELIVERY_VERIFICATION.md),
[ADR-0020](adr/0020-owner-local-manifest-delivery.md). Новый SQL migration не нужен.

### Task workspace и candidate evidence source, 2026-10-07

Отдельный source cut поверх PR87 добавляет blocked task operation ledger40,
physical preparation и readback candidate Git/pipeline/artifact evidence.
Readback не разблокирует dispatch, scoped Git write, deployment, acceptance или
rollback; authoritative Tracker counterpart отсутствует. Evidence и границы:
[task verification](TASK_DELIVERY_VERIFICATION.md). Исторический runner release
ниже принадлежит PR87; его SQL39 и source сохраняются без изменений.

### Release candidate 2026-10-07: runner workspace recovery

Из общей опубликованной ветки отдельно выделен `feat/runner-workspace-recovery-20261007`
от `main c606886`. Он содержит только runner-owned workspace/recovery и одну
новую migration0039; task-bound operation ledger0040 и preparation не включены.
Base pin остаётся принятым `875cac2edf1a18c3a8a59e2f67256d02a8fc04e4`.

Дополнительный release audit 7 октября обнаружил два frontend advisory:
GHSA-68fv-2mgg-jv7q и GHSA-xvq9-wjp8-hwqf. Неиспользуемый HTTP translation
backend удалён, `source-map-js` закреплён на исправленной1.2.2. Frozen install,
201 tests, lint/typecheck/build и `pnpm audit --json` проходят:0 advisories.
SBOM пересоздан из lockfiles (394 компонента); CI теперь проверяет его drift и
frontend advisories. Свежий Linux/PostgreSQL gate e3ee650c0465 проходит все
runner/migration/recovery проверки, release build и OpenAPI equality. Он также
подтверждает отсутствие `rsa` и `sqlx-mysql` в активном graph для всех target.
Поэтому уже документированное исключение RUSTSEC-2023-0071 остаётся применимым
только к optional lockfile path; необработанный `cargo audit` всё ещё сообщает
этот advisory, с указанным исключением остальные findings отсутствуют.
Hosted CI нового commit проверяется отдельно; эти результаты не означают
установку candidate или готовность всего SDLC.

На exact source355885f Compose `sdlc-qa-forge-merge-ff054300108e` проходит
Rust1.88 locked/offline fmt, workspace/all-target check/strict Clippy с integration,
весь `cargo test --workspace`, release workspace build, Rust OpenAPI equality
и8 целевых PostgreSQL-тестов migration/owned terminal receipt/embedded pinned run.
Все202 backend/deploy/OpenAPI/SDK inputs неизменны; containers/networks удалены,
external caches сохранены. Первый packet7d031f4a7da1 не прошёл из-за отсутствия
read-only mount seccomp profile в QA; тест не отключён и production profile не менялся.

Frontend с тем же SDK и Node22.20.0/pnpm10.28.1 проходит frozen/offline install,
typecheck/lint,201 tests/build, OpenAPI check и compatibility с main. Generated
client отдельно совпадает с committed schema. Docs verifier и heuristic scan
tracked export (450 text files) проходят. Нового UI layout нет; новые screenshots
этим candidate не заявляются. DB-тест receipt включает реальный curl к ephemeral
Axum HTTP listener; это не smoke установленного runtime. На исходном head ed69fb0
все четыре CI jobs (docs/backend/frontend/minimum-rust) проходят, включая полную
integration/CLI matrix. Для последующих commits нужны их собственные checks.
Installation остаётся отдельным gate. Это не full SDLC acceptance:
task admission, candidate/deployment/acceptance/rollback receipts не реализованы
этим PR и не выдаются из workspace marker или EOF.

Recovery-проверка расширена настоящим `forge-runner` против PostgreSQL/Axum:
отдельные процессы читают принятый completion, сохраняют workspace по флагу и
удаляют его только после нового readback. При expiry или revoked credential
локальный ACK не разрешает ни cleanup, ни polling. Семь наблюдённых GET, ноль
повторных completion POST и work poll. Fresh Composec94310c35af2 проходит весь
локальный Rust/PG gate выше;202 input hashes неизменны, own containers/network
удалены. Это recovery accepted completion, не повторное исполнение pipeline,
не доказательство безопасной остановки всего process tree и не deployment receipt.

### Source delta 2026-10-03, не installation evidence

Runner workspace foundation реализован в source: fresh attempt/lease/generation
directory, owner marker/path checks, full SHA + clean detached HEAD перед командами,
cleanup после проверенного terminal acknowledgement. Docker-job больше не видит
общий root всех попыток. Негативный или неизвестный completion/checkout сохраняет
папку. Scoped source tests перечислены в [DEVELOPMENT_GUIDE](DEVELOPMENT_GUIDE.md); полный интегрированный
gate и live acceptance ещё не выполнены. Это не закрывает весь B-SDLC-04:
см. [границы SDLC receipts](SDLC_DELIVERY_V1.md).

Source follow-up: runner-owned completion GET, durable local outcome/ACK journal,
offline inventory и explicit restart reconciliation. Unknown completion сверяется
без повторного POST; unresolved workspace не разрешает новое external polling.
Local ACK не является server proof: restart/cleanup повторно читает owner.
Expiry, другой attempt/fence и подмена marker не подтверждают результат. Source
tests/ограничения — [runner recovery gate](DEVELOPMENT_GUIDE.md#scoped-runner-recovery-source-gate).
Installed runtime и B-SDLC-04 task/assignment delivery receipts этим не закрыты.

Independent review follow-up: неподтверждённый child wait больше не превращается
outer handler в failed completion или idle capacity/poll. Pending migration 0039
отделяет принятый external completion от expiry cancellation; historical rows
не backfill-ятся. 32 scoped tests PASS, включая actual completion POST и actual
cancel-on-expiry GETfalse; status mismatch проверен при присутствующем признаке
completion. Component failure injection не является OS process-tree stop proof.

| Capability | Статус | Границы |
|---|---|---|
| Проекты CRUD | ✅ | name/repository_url/default_branch; удаление CASCADE |
| Git hosting (bare + Smart HTTP) | ✅ | public/private fetch ACL, optional token-protected push, code tree/blob, tags/releases; Smart HTTP RPC body limit 100 MiB с gzip post-inflate check; push → post-receive(old/new SHA) → idempotent pipeline per pushed object |
| Pipeline/стадии/джобы | ✅ | `.forge-ci.yml` legacy `stages/jobs/image/command` или v1 `version: 1` + top-level `jobs.commands/needs/tags/secrets`; fallback-шаблон при отсутствии config; trigger пытается читать config по resolved commit SHA; ручной trigger поддерживает `Idempotency-Key`; отмена/повтор в текущей job-модели |
| Pipeline plan snapshot | ✅ MVP | `pipeline_plans` хранит immutable snapshot: raw config/fallback template, `config_sha256`, parser version, normalised plan JSON, `plan_sha256`, dependency edges и v1 `required_tags`/`required_secrets`/`artifact_paths`. Current форматы: `legacy-linear` (`forge-legacy-linear/1`) и `v1-dag` (`forge-dsl/1.0.0`); v1 DAG исполняется через топологические `dag-*` стадии, а policy diagnostics/job-level dispatcher остаются target |
| Embedded runner | ✅ | Docker (`forge-job-<id>`) или host shell; lease-aware claim, стриминг stdout → attempt-owned `job_logs`; cancel через PID-map; можно отключить `CICD_EMBEDDED_RUNNER_ENABLED=false` |
| Execution attempts | ✅ MVP | `execution_attempts` создаются для каждой job; retry job/pipeline создаёт новую attempt и не удаляет старые логи |
| Job queue | ✅ MVP | `job_queue` материализует dispatch row на current queued attempt и копирует `required_tags`; trigger/retry/manual start enqueue-ят non-manual work, embedded claim берёт только untagged rows, external runner claim берёт совместимую работу через queue row + `SKIP LOCKED` + `required_tags ⊆ runner.tags` + current `shell` executor compatibility, unacknowledged external offer после `ackDeadline` requeue-ится, dispatch-eligible queued job без compatible execution path после `CICD_RUNNER_QUEUE_TIMEOUT_SECONDS` завершается с diagnostic, terminal/cancel/lease expiry закрывают row |
| Job leases | ✅ MVP | embedded runner создаёт active `job_leases` при claim, закрывает lease при terminal result/cancel и reconciler переводит expired/missing lease в failed; внешний runner protocol MVP выдаёт lease token, ack/renew/control/logs/complete, requeue-ит unacknowledged offer после `ackDeadline`, доставляет cancel signal через `cancel_requested_at`, проверяет fencing generation и защищает stale-runner offline reconciliation живой unexpired lease |
| External runner protocol + `forge-runner` | ✅ MVP | `/api/v1/runner/register`, heartbeat, `work:poll` с optional `waitSeconds` `0..30`, ack/renew/control/`secrets:resolve`/artifact upload/logs/complete; credential и lease token хранятся только hash-ами; `work:poll` claim-ит compatible durable `job_queue` row по `required_tags ⊆ runner.tags` и current `shell` executor compatibility (`capabilities.executorKinds` отсутствует или содержит `shell`), умеет bounded long-poll wakeup после enqueue/разблокировки стадии через in-process signal и PostgreSQL `LISTEN/NOTIFY` channel `runner_work_available`, отдаёт `workspace.checkoutUrl`, declared `attempt.secrets` и `attempt.artifacts`; отдельный `forge-runner` shell process умеет checkout, scoped secret resolve после ack, renew, active-lease heartbeat во время выполнения, polling cancel signal, declared artifact upload, stdout/stderr log append с masking и terminal completion; runtime maintenance requeue-ит unacknowledged offer после `ackDeadline`, fail-ит просроченную dispatch-eligible очередь без compatible runner-а и переводит stale online runner без unexpired active lease в `offline`, в том числе при `CICD_EMBEDDED_RUNNER_ENABLED=false`; richer log chunks, protected tags/pools, advanced capability matching и Kubernetes sandbox остаются target; resumable artifact sessions, project dispatch cap и Docker seccomp/resource classes реализованы |
| Логи | ✅ | append-only внутри attempt, sequence per attempt, совместимый REST array shortcut, bounded `/logs/page` с `limit/after/before/q`, tail-window metadata `total`/`has_more_before`, SSE stream текущей/последней attempt, latest attempt diagnostic на terminal job card в деталях pipeline и явный 1 MiB body limit на append endpoints |
| Артефакты | ✅ MVP | upload/download ≤50 MiB, route-level body limit 50 MiB, локальный `CICD_ARTIFACTS_DIR`; новые metadata привязаны к active/latest attempt, содержат SHA-256 и `expires_at`; download проверяет canonical path containment, checksum drift и не отдаёт expired/purged записи; retention worker удаляет expired local files и ставит `purged_at` |
| Секреты проектов | ✅ | AES-256-GCM at rest; значение не возвращается user API; execution выдаёт только job-declared имена секретов |
| Environments/deployments | ✅ MVP | metadata + append-only deployment history; protected environments require approval before backend starts the linked deployment pipeline, decisions are stored in `deployment_approvals`, and rollback creates a separate `rollback_of_id` deployment record. Richer policy rules, multi-approver workflows and rollback orchestration remain target |
| Reports | ✅ | агрегаты success rate/duration |
| Users/roles + API-токены | ✅ | хранение + enforcement при `CICD_AUTH_SECRET`; Dashboard user-create передаёт optional password для interactive login; session-bound access JWT с `users.token_version`, refresh session rotate/logout и session-family reuse revocation; новые PAT требуют project scope, scopes и expiry; глобальная роль ограничивает максимум прав |
| Audit log | ✅ | append-only; legacy array последних 200 сохранён, полный `/audit-log/page` поддерживает server pagination, exact action filter, literal search и stable ordering |
| Schedules | ✅ MVP | строгий 5-польный UTC cron, persisted `next_fire_at`, unique `schedule_fires` slot и idempotent pipeline trigger; строки с `last_fire_error` ждут явного PATCH/исправления; IANA timezone/DST/misfire и multi-replica leases остаются target |
| Outgoing webhooks | ✅ MVP | terminal pipeline event -> `domain_events`/`outbox_messages`; basic retry/backoff, optional HMAC |
| Outbox delivery history | ✅ MVP | project-scoped paged `/outbox-deliveries/page` API показывает всю фильтруемую историю, статус, attempts, `failed_at`/`last_error`; failed delivery можно явно requeue новой generation |
| Notifications | ✅ MVP | `in_app`/`sse` каналы создают durable local outbox event на terminal pipeline events; история доступна через `/notification-events`, live stream — через `/notifications/stream`; email/Slack adapters и inbound provider handlers остаются target |
| Login UI | ✅ | `/login` запускает Central Auth Authorization Code + PKCE, `/sso/callback` завершает обмен, access token хранится только в памяти; hard reload повторно использует центральную HttpOnly browser session без локальной password form |
| Project membership RBAC | ✅ MVP | `project_memberships`, фильтрация списка проектов, deny-before-load для project-owned API и name-based repo API; `admin` bypass, tenant isolation/SAT ещё target |
| Git auth/RBAC | ✅ conditional MVP | public read открыт; legacy resources сохраняют project membership policy. Подтверждённые Namespace repositories доступны доверенным людям по service/global role и scopes, без team ACL. Project-scoped PAT остаётся в своём delivery project; machines не получают human-доступ. Archive блокирует receive-pack до admin/legacy-token bypass. Без auth secret и Git token сохраняется только trusted local mode |
| Central human auth | ✅ | Central Auth владеет users, browser sessions и scoped personal tokens; backend проверяет ES256/JWKS, issuer/audience/expiry и live session, связывает profile только по `central_sub`, закрывает local login/user/PAT mutations и не использует local fallback при ошибке central service. Machine runner/service-account tokens остаются отдельными. |
| Runtime configuration | ✅ | Backend server загружает `RuntimeConfig` из `CICD_` один раз при старте/app construction: Database/Http/Git/Artifacts/Runner/Auth/Secrets. Startup/router validation покрывает bool, runner mode, CORS allowlist, artifact TTL, queue timeout и secrets key; AppState/supervisors используют config без повторного прямого чтения env. CLI и `forge-runner` остаются отдельными process-boundary tools на `clap`/env |
| Secret injection | ✅ MVP | embedded runner передаёт в env только `jobs.required_secrets`; external runner получает declared secrets через lease-scoped `secrets:resolve` после ack; stdout/stderr masking best-effort |
| Error envelope + request_id | ✅ | Backend returns `{error:{code,message,request_id}}` + `x-request-id`; raw internal SQLx errors are logged server-side and hidden behind generic `500 internal_error`; frontend transport maps structured envelopes, non-JSON HTTP failures, network failures and cancelled requests into typed `ApiError` while preserving status, `code`, `request_id`, safe details and `Retry-After` |
| Pagination | ✅ | limit/offset (cap 200) на проектах/пайплайнах |
| Rate limiting / body limits | ✅ MVP | in-process per-client fixed-window: auth, API read/write, Git Smart HTTP, internal hook и artifact upload возвращают `429`; явные body limits покрывают artifact uploads, Git RPC, JUnit upload и log append |
| Health/readiness/metrics | ✅ | `/api/v1/health` liveness без БД; `/api/v1/readiness` проверяет PostgreSQL и SQLx migration versions/checksums; `/metrics` Prometheus text |
| Compose packaging smoke | ✅ local gate | Локальный disposable Compose gate выполняет `docker compose config -q`, production image build, `docker compose up --build -d`, backend health/readiness и frontend nginx smoke с cleanup; hosted CI его не запускает |
| Browser E2E / accessibility / performance smoke | ✅ local gate | Локальный Playwright Chromium gate на собранном Compose stack использует deterministic `frontend/scripts/seed-evidence.mjs`, проверяет critical journeys, axe smoke и seeded regression budgets. Lighthouse, full keyboard/theme audit, load test и 30-day SLO evidence остаются target; hosted CI browser gate не запускает |
| RU/EN i18n contract | ✅ MVP | `frontend/src/shared/i18n/i18n-contract.test.ts` проверяет parity ключей `ru`/`en`, непустые значения, отсутствие raw-key fallback, runtime switch i18next и динамические переводы для стабильных API contract values: pipeline/change/PR/runner/delivery/notification statuses, PR actions и PAT scopes; full locale E2E и полный stable identifier contract suite остаются target |
| CLI | ✅ MVP | `cicd-cli` остаётся HTTP-only и покрывает runtime/platform операции: projects/pipelines/jobs/logs/attempts, runners, secrets, artifacts, environments/deployments, schedules, webhooks/outbox, notifications, reports/audit, users, project members и API tokens; поддержаны `CICD_API_TOKEN`/`--token`, `CICD_TIMEOUT_SECONDS`/`--timeout-seconds`, `CICD_OUTPUT`/`--output json|table`, `--limit`/`--offset` для projects/pipelines; real HTTP/API/PostgreSQL smoke покрывает project create/list, idempotent pipeline run, attempts, protected deployment approval, env/flag precedence, bounded timeout, JWT/PAT auth-mode, RBAC denial, project-scoped read-only PAT и token redaction на CLI failure; profiles (`config.toml`), NDJSON, stable exit codes и shell completion реализованы; OS keyring, YAML output, request tracing и расширенные redaction fixtures остаются target |
| Backup/restore helper | ✅ MVP | `scripts/forge_backup.py` + wrappers создают/проверяют/restoring local Docker Compose backup: PostgreSQL custom dump, Git/artifact volume copy, `SHA256SUMS`, `manifest.json`; off-site rsync via `FORGE_BACKUP_OFFSITE_DIR`, local `--retention N` and a PostgreSQL 17 restore drill are current; encrypted/PITR platform and scheduled monthly drill remain target |
| Dependency audit / secret scan / SBOM hygiene | ✅ local gate | Локальный release scope включает OpenAPI backward compatibility diff, SQLx optional MySQL/RSA feature guard, `cargo build --release --workspace`, `cargo audit --ignore RUSTSEC-2023-0071`, `pnpm audit --audit-level high`, `scripts/scan_secrets.py`, `scripts/generate_sbom.py --check` и Trivy image scan. Hosted CI выполняет docs/backend/frontend gates; OpenAPI examples validation, `cargo-deny`, deeper history/container secret scan и release SBOM publication остаются target |

## Не реализовано (Target approved — см. ADR + contracts)

Idempotent chunked log upload, Kubernetes sandbox, pool/protected-tag policy и advanced capability matching (ADR-0007), policy-aware pipeline planner поверх v1 DAG (`on`, retry, `artifacts.expire_in`, line/column diagnostics, job-level dispatcher), full secret redaction/rotation/environment policy, richer protected-environment policy rules, multi-approver delivery workflows и rollback orchestration, general idempotency storage for all retryable mutations, command spans/stream classification для диагностических логов, artifact object storage/tenant isolation/legal hold, encrypted/PITR backup platform and scheduled restore-drill evidence, external notification channel adapters (email/Slack), inbound provider webhook handlers, tenant isolation, service-account tokens, scoped Git credentials, schedule IANA timezone/DST/misfire и multi-replica leases, outbox lease/fencing/crash recovery, full dead-letter operator policy/metrics, OpenAPI examples validation/deprecation lifecycle, full locale E2E/stable identifier suite, Lighthouse/load reports и 30-day SLO evidence, `cargo-deny` license/source policy, broader container/image policy beyond current critical CVE scan, deeper history/container secret scan, release SBOM publication и distributed/proxy rate/time/concurrency limiting (сейчас in-process окно по forwarded client key и route-level body limits).

## Текущее runtime-дерево backend

```text
backend/
├── Cargo.toml          # workspace: server + domain/app/infra/api/cli
├── app/                # cicd-app: auth/session primitives and route policy
├── infra/              # cicd-infra: SQLx store and enqueue notify hook
├── api/                # cicd-api facade
├── src/                # cicd-server composition root; API verticals live in src/api/
│   ├── api/            # auth/projects/pipelines/jobs routes, dto, middleware, router, readiness
│   ├── platform.rs     # runners/secrets/artifacts/… /users/tokens
│   ├── git_host.rs     # bare repos, Smart HTTP, post-receive
│   ├── pulls.rs        # refs/commits/compare/pull requests
│   ├── runner.rs       # embedded executor (+job_leases, secret injection, маскирование)
│   ├── runner_protocol.rs # external runner protocol MVP
│   ├── bin/forge-runner.rs # external shell runner process MVP
│   ├── config.rs        # typed RuntimeConfig для server startup/AppState/supervisors
│   ├── outbox.rs       # ADR-0006: domain_events/outbox + scheduler worker
│   ├── authz.rs        # role-политики роутов + project membership enforcement
│   ├── rate_limit.rs   # in-process fixed-window route-class limiting
│   ├── metrics.rs      # /metrics Prometheus exposition
│   └── domain.rs       # re-export shim → cicd-domain
├── migrations/         # versioned SQLx migrations incl. 0023 artifact retention
├── domain/             # cicd-domain: чистые типы + JobStatus
├── cli/                # cicd-cli: HTTP-only runtime/platform commands
├── tests/              # api_contract, domain_transitions, integration_db (+ sql/init-roles.sql)
├── cli/tests/          # cli_contract + cli_real_api integration smoke
└── docker-compose.test.yml
```

## Известные dev-only риски

- Без непустого `CICD_AUTH_SECRET` API и Dashboard полностью открыты в trusted-network режиме. Пустой `CICD_CORS_ALLOWED_ORIGINS` оставляет permissive CORS для isolated development; shared deployment обязан задать allowlist origins.
- Loopback-only Caddy TLS profile (`docker-compose.tls.yml`) terminates HTTPS with an internal CA, removes direct backend/frontend host ports, forces secure refresh cookies and an exact CORS origin. Public ingress/ACME, host firewall/VPN, and client CA trust stay operator-owned; this profile does not turn the local stack into a public-production deployment.
- `CICD_GIT_INTERNAL_TOKEN` пустой по умолчанию только для isolated local development; shared-деплой обязан задать уникальный токен, а legacy `forge-internal-dev-token` отклоняется при старте.
- Auth/RBAC пока без tenant isolation, service-account tokens и scoped Git credentials; session-bound access invalidation, browser refresh cookie + CSRF, refresh rotate/logout/revoke, session-family reuse revocation, project membership, scoped PAT, configurable CORS allowlist и Git read/write checks реализованы как MVP-слой поверх глобальных ролей.
- Execution attempts / job queue / job leases — MVP-слой: old `/jobs/{id}/logs` читает текущую или последнюю attempt, bounded `/jobs/{id}/logs/page` supports `limit/after/before/q` tail windows and metadata, полный аудит попыток доступен через `/jobs/{id}/attempts`, `job_queue` переживает restart и является источником claim для embedded/external runners, embedded берёт только untagged rows, внешний runner protocol уже проверяет runner credential, lease token, fencing generation, tag compatibility и current `shell` executor compatibility по `capabilities.executorKinds`, принимает stdout/stderr log append, выдаёт только declared secrets после ack, принимает declared artifact upload, поддерживает bounded long-poll `work:poll` через in-process + PostgreSQL `LISTEN/NOTIFY` wakeup, requeue-ит unacknowledged offer после `ackDeadline`, fail-ит dispatch-eligible queue timeout без compatible execution path, доставляет cancel signal через lease `control` и переводит stale online runner без unexpired active lease в `offline`. `forge-runner` даёт отдельный shell process со scoped secret env + masking + artifact upload + active-lease heartbeat + cancel polling, но K4.4 Docker seccomp/resource classes, resumable artifact sessions and project dispatch limits are current; production runner-zone separation, richer log chunks, protected tags/pools/advanced capabilities and expanded restart/race suite remain target.
- Scheduler/outbox — MVP: есть строгий 5-польный UTC cron и уникальные fire slots, но нет IANA timezone/DST/misfire, lease/fencing/crash-safe dispatcher-а, full dead-letter operator policy/metrics и внешних notification adapters; bounded delivery history/requeue и `in_app`/`sse` local outbox projection уже работают.

## Верификационные команды

```bash
docker compose config -q
docker compose -f backend/docker-compose.test.yml config -q
just readiness
docker run --rm --entrypoint /bin/bash -v "$PWD/backend:/workspace" -w /workspace \
  -e CARGO_TARGET_DIR=/workspace/target rust:1.88-bookworm \
  -lc '/usr/local/cargo/bin/cargo test --workspace'
docker run --rm --entrypoint /bin/bash -v "$PWD/backend:/workspace" -w /workspace \
  -e CARGO_TARGET_DIR=/workspace/target rust:1.88-bookworm \
  -lc '/usr/local/cargo/bin/cargo test -p cicd-cli --test cli_contract'
cd backend && CICD_TEST_DATABASE_URL=<reachable test PostgreSQL> \
  cargo test -p cicd-cli --features integration --test cli_real_api -- --test-threads=1
cd frontend && pnpm test && pnpm build
cd frontend && pnpm e2e   # requires running seeded Compose stack
cd frontend && pnpm lint
python3 scripts/generate_sbom.py --check
bash scripts/scan_container_images.sh forge-cicd-backend:ci forge-cicd-frontend:ci
python3 scripts/verify_docs.py --canonical --links --current-state
```

## Frontend: 26 маршрутов / 24 рабочие страницы + /login + /sso/callback

Полный список базовых страниц — `docs/architecture/frontend-boundaries.md`; визуальный реестр — `docs/assets/screens/manifest.md`. Исполняемый route smoke — `frontend/src/app/router.test.tsx`: production `appRoutes` поднимаются в memory router, а рабочие Dashboard-страницы, `/login` и технический `/sso/callback` проверяются на первый рендер с mocked API DTO. `scripts/verify_docs.py --all` дополнительно сверяет, что visual manifest покрывает каждый production route. Real-browser baseline — `frontend/e2e/critical-flows.spec.ts` и all-route `frontend/e2e/accessibility.spec.ts` против собранного Compose stack с deterministic seed.
