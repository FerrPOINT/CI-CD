# Native canary настоящего Forge main

Source-only ignored target: backend/tests/kubernetes_server_canary.rs.
Owner выполняет build/native gate. Сценарий проверяет local-auth и ordinary
embedded CI через настоящий main, PostgreSQL и общий Kubernetes executor.
Central SSO, external runner protocol и фактические задания пользователя остаются
отдельными gates. Image build/deploy recipes исключены.

## Owner build и staging

На exact frozen SHA с SDK2ad owner собирает только package/target и main:

~~~sh
FORGE_MAIN_CANARY_BUILD_HEAD=<exact-source-SHA> cargo test --locked --offline \
  -p cicd-server --features integration --test kubernetes_server_canary --no-run
~~~

Выполнить pure test canary_scope_requires_ordinary_feature_and_exact_build.
Native test остаётся ignored до подготовки fixture. Test запускает
env!("CARGO_BIN_EXE_cicd-server"); owner сохраняет actual compiled main binary
и staging делает по ЭТОМУ exact embedded absolute path, с соответствующим SHA256.
Runtime image/ELF loader квалифицируются отдельно, без install/fallback.

Использовать отдельный disposable worker/namespace/SA и два независимых PVC:
workspace /workspaces и qualification (private operation subPath /qualification).
Оба PVC root заранее 2770/exact fsGroup; private qualification operation — 0700,
job UID/GID. Jobs видят только exact attempt workspace subPath, без token/qualification.
NetworkPolicy разрешает worker только API endpoint и собственный PostgreSQL
TCP endpoint; loopback HTTP остаётся внутри worker. Job egress закрыт.

ВАЖНО: worker не имеет activeDeadlineSeconds и не удаляется внешним watchdog/timeout.
Нельзя запускать его через прежний f8cb helper с deadline2400 без отдельной
адаптации owner. Driver после unknown cleanup остаётся живым; Pod expiry/удаление
завершило бы неизвестный main. Нет TERM/KILL, kill_on_drop или abort fallback.
Owner держит native process/CID/PVC evidence; HOLD не является stop proof.

Runtime env driver:
FORGE_MAIN_CANARY_INPUT=<canonical-private-file> и
FORGE_MAIN_CANARY_INPUT_SHA256=<SHA256>.
Запуск: exact test binary actual_main_http_current_ci_canary --ignored --exact.
Стандартные readonly SA token/CA доступны в serviceaccount path. Production env,
DSN, credentials, Docker socket и application configuration не передаются.

## Private input ABI

CamelCase, denyUnknownFields, schemaVersion=1,
kind=forge/native-main-canary/v1:

- operationId: fresh UUID;
- expectedSourceHead: exact build SHA; expectedExecutorSha256: production kubernetes.rs;
- mainBinarySha256: SHA actual compiled CARGO_BIN_EXE main;
- image: owner-qualified immutable ordinary OCI ref;
- apiHost/apiPort: literal IPv4/port, bound to owner native route/network receipts;
- admissionFile: live guard payload mirrored owner-ом, private file, same operation;
- runnerConfig: {path,sha256}; expectedConfig: exact production Config v1,
  builderEnabled=false, deploymentRecipes=[];
- database: {path,sha256}, fresh owned private DSN file to EMPTY whole database
  forge_test_*, no URI query/search_path; root verifies native PG/node identity;
- migrationsDirectory/migrationsSha256: canonical exact source SQL tree. Digest:
  sorted immediate filenames, each filename UTF8 bytes + NUL + raw file bytes,
  SHA256 across concatenation; no symlinks/hardlinks/non-SQL entries.

Private references: regular canonical path, current UID, one link, no group/other
access, max64KiB. Root does not reinterpret an old DB/receipt as this new fixture.
Test creates NEW /qualification/main-canary-<UUID>; restart/partial existing
root is rejected, never cleared or replaced. Retain custody for owner reconciliation.

## Cases и lifecycle

Main env is explicit: fresh local signing secret, own DB/Git/artifacts, api+runner
roles, Kubernetes mode, embedded enabled, private quiescence participant canary.
Main itself applies exact migration chain. Fixture creates an Argon2 user and
obtains a real session/JWT via /auth/login; invalid/missing credentials must
return401. No middleware injection or auth disabling.

Owned bare Git has two pinned refs. Repository/project linkage is explicit in
isolated fixture DB; all pipeline trigger/poll/cancel, logs and artifact download
use real HTTP. Success proves repository plan/commit, source bytes, log marker and
downloaded artifact bytes. Cancel waits for an actual unique marker written by
running job in its exact attempt/lease/generation workspace, then HTTP cancel.

After case Result/panic/observation timeout, retained outer owner attempts
existing product quiescence. Exact main PID/start/boot/custody roots and live
kernel registration must match. Coordinator waits for recoverable idle
(DB + actual retained custody), closes its pool, publishes held and requests normal
exit while retaining fence. Only successful Child::wait plus all_exited allows
test process to return. No expiry/object absence is process-stop proof.

If cleanup fails/panics/times out: HOLD.json and stderr
FORGE_MAIN_CANARY_HOLD identify operation/PID/initial start/evidence; driver remains
alive indefinitely. Root must preserve worker/all custody and inspect/reconcile;
failed test is not permission to delete Pod/PVC or kill main.

Evidence includes planned.json, child.json, private main logs, cancel-marker.json,
http-cases.json, held.json, exited.json, result.json. PASS requires both HTTP cases
and actual normal exit. No datasets/control/evidence are automatically deleted/resumed.
