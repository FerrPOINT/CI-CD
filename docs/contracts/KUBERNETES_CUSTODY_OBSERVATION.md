# Private Kubernetes custody observation

Цель — Base status/check при настоящих обычных OCI CI jobs. Это bounded read-only
наблюдение ownership, не idle/drain/stop/admission proof. Public HTTP API, DB schema,
SDK/UI pins, Shell/Compose branches не меняются. Builder/workspace-deploy остаются
отдельно unqualified; private observation не расширяет поддержку этих features.

## Private ABI v1

Выполнять только exact installed forge-quiesce binary внутри pinned Forge
controller process/container generation. Base сверяет source/image/CID или PodUID/
CRI identity своим native owner contract; JSON сам по себе не подтверждает binary.

    forge-quiesce --directory <existing control directory> inspect-kubernetes-custody --request-sha256 <SHA256>

Existing global directory argument/env CICD_QUIESCENCE_DIR сохраняется.
Этот subcommand dispatch происходит ДО Gate::open и не читает/создаёт Gate,
participants, marker, coordinator или migration state. Stdin — один UTF8 JSON,
raw hash включает реальные bytes/newline; owner обязан закрыть stdin EOF.
Bound64KiB и128 Pods, deny_unknown_fields, непустой canonical challenge UUID:

    {
      "schemaVersion": 1,
      "challenge": "<fresh UUID>",
      "installationSha256": "<sealed expected installation>",
      "configSha256": "<raw CICD_KUBERNETES_RUNNER_CONFIG hash>",
      "namespace": "<exact name>",
      "namespaceUid": "<exact UID>",
      "workspaceClaim": "<exact claim>",
      "workspaceClaimUid": "<exact UID>",
      "pods": [{"name":"<actual name>","uid":"<actual UID>","resourceVersion":"<actual RV>"}]
    }

Source config читается из existing mounted CICD_KUBERNETES_RUNNER_CONFIG;
workspace root фиксирован /workspaces, standard in-cluster token/CA — только
controller transport. Namespace/PVC UID и Bound state читаются обратно через GET.
DB URL остаётся в controller environment, никогда в input/output. Один temporary
pool/readonly transaction закрывается до ответа. No lease token/hash, credentials,
raw env/command, logs или Pod spec не возвращаются.

Success exit0:

    schema: forge/kubernetes-custody-observation/v1
    status: VERIFIED
    challenge,requestSha256,installationSha256,configSha256
    namespace,namespaceUid,workspaceClaim,workspaceClaimUid
    lifecycleEffectsPerformed: false
    sourceWritersClosed: false
    observations:
      - podName,podUid,resourceVersion
        jobId,attemptId,leaseId,generation,sourceCommit,runnerId
        executorKind: job
        authoredSpecSha256
        phase: pending|running|canceling|terminal-stopped|cleanup-pending

runnerId — null для embedded, exact registered UUID для external.
HOLD exit2: тот же schema, status=HOLD, challenge (null если malformed),
requestSha256, reasonCode=custody_unverified и два false поля. Нет partial
VERIFIED list или exception/body dump. Stdin/native transport failure вне
машиночитаемого результата тоже трактуется Base как HOLD.

## Causal chain и границы

Shared executor перед первым CREATE сохраняет private authored-spec.json:
schema/action SHA/normalized authored spec SHA/executor kind, без manifest или
secret values. Seal создаётся после effective template selection и admission,
но ДО issued marker/POST; общий путь одинаков для embedded/external.
Historical actual Pod без этого seal — HOLD, не labels-only fallback.

Readonly inspector проверяет issued+created authority, exact PodUID/RV/name,
installation/job/attempt labels, service account/mount policy и digest actual
spec против sealed authored spec. Все command/env/image/mount/security/unknown
поля сохраняются в hash. Убираются только перечисленные Kubernetes defaults;
CPU/storage quantity conversion точный rational, без float/rounding. Scheduler
nodeName допускается только потому, что authored template не может его задавать.
Непредусмотренный admission sidecar/default вызывает HOLD.

DB SELECT связывает exact scope с latest generation, job+attempt, queue lease
owner/stage/pipeline/source commit, embedded owner либо registered external runner
и его issued ACK. DB authority читается до и после Pod GET; drift — HOLD.
Terminal status/expiry не заменяют Pod stop proof. Для terminal phase требуется
полный существующий container termination validator; readonly вызов не пишет
terminal.json/ACK, не вызывает start/observe/reconcile/cancel/cleanup.
Known canceling никогда не объявляется остановленным.

Читается bounded custody/workspace catalog. Unknown entries/owners, omitted issued
unACKed Pod, multiple scope, wrong physical workspace mapping, overflow и stale
lease — HOLD. Legitimate pre-Pod source checkout допускается только по exact
current DB reservation/lease/source pin; API Pod без prepared intent при этом
не принимается. Retained cleaned records требуют matching terminal/ACK/
cleanup-intent, не удаляются и не используются как native current-stop proof.

Base сначала получает полный выбранный non-Deployment Pod census, вызывает verb
через свой exact installed-controller native transport, сравнивает полный UID/RV
набор с observations, затем перечитывает census/producer identity. New/missing/
changed objects делают observation stale: HOLD или fresh observation, без
публикации cached proof. Запрос — не право изменять другие Pods. Unknown Job/
Pod никогда не игнорируется по labels/name prefix. First-start finite idle gate,
application admission, actual workload health, backup/source closure и native
process stop остаются отдельными checks.

## Source tests и owner gate

Добавлены unit tests sealed shape/defaults/secret omission/tamper/unknown custody,
phase/expiry/terminal boundaries и actual private CLI no-Gate initialization.
Ignored real-PG regression использует existing migrated disposable fixture:
exact readonly SELECT, wrong identity/source/generation/reservation, external
ACK/runner mismatch, unchanged DB snapshot. Агент Rust/PG/native не запускал.

Owner на final formatted source выполняет scoped cargo check alltargets integration,
execution::kubernetes::inspection tests, kubernetes_custody_cli и
kubernetes_inspection_uses_exact_readonly_reservation_source_and_runner_chain
с disposable CICD_TEST_DATABASE_URL. Затем actual external real-main current-CI
native gate должен использовать новый final app/control source/ELFs; historical
69db receipt не выдаётся за qualification этого additive slice.
