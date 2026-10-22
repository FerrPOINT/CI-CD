# Forge runner в Kubernetes

Source candidate 2026-10-22. Установка, strict rootless feasibility и live task
qualification принадлежат оператору; source tests не подтверждают эти этапы.
Compose/host-shell paths и SDK/UI pins сохранены.

## Конфигурация

Backend и внешний runner используют один product-local adapter
`backend/src/execution/kubernetes.rs`. Включение: `CICD_RUNNER_MODE=kubernetes`,
`CICD_KUBERNETES_RUNNER_CONFIG=/etc/forge/runner.json`.
PVC root заранее имеет configured fsGroup, group rwx и setgid (например2770),
чтобы `OnRootMismatch` не менял весь PVC рекурсивно; source проверяет это до
эффекта. Controller монтирует свой workspace PVC в `/workspaces`; external work-dir
также должен быть `/workspaces`. Пример non-secret operator JSON:

```json
{
  "schemaVersion": 1,
  "installation": "pdlc-a",
  "installationSha256": "<exact installation manifest SHA256>",
  "clusterCaSha256": "<SHA256 of mounted in-cluster ca.crt bytes>",
  "namespace": "pdlc-a",
  "namespaceUid": "<exact Namespace UUID>",
  "workspaceClaim": "forge-workspaces",
  "workspaceClaimUid": "<exact Bound PVC UUID>",
  "jobServiceAccount": "forge-job",
  "builderServiceAccount": "forge-builder-job",
  "allowedImages": [],
  "builderImage": "<approved rootless BuildKit repository@sha256:digest>",
  "registryPrefix": "registry.example.test/pdlc-a",
  "registrySecret": null,
  "resources": {
    "cpuMillis": 2000,
    "memoryBytes": 2147483648,
    "ephemeralStorageBytes": 4294967296
  },
  "security": {"runAsUser": 10001, "runAsGroup": 10001, "fsGroup": 10001},
  "deploymentRecipes": []
}
```

Пустой `allowedImages` сохраняет выбор user image. Tags разрешаются перед новым
эффектом через pinned `crane digest`; Pod и protected custody получают immutable
digest. Непустой список — явная дополнительная installation policy. `crane`
v0.20.6 упакован с committed official archive checksums. Его registry reader
credentials остаются controller-only; operator может задать `DOCKER_CONFIG`.
Credentials процесса controller не копируются в job environment.

Controller читает стандартные in-cluster token/CA; redirects и HTTP отключены,
CA bytes сверяются с pin. Перед effects проверяются Namespace UID и Bound PVC UID.
Service accounts и scoped Role принадлежат deployment adapter. Job не получает
API token, host mount/socket, общий PVC root или `.kubernetes-control`.

`CICD_KUBERNETES_ADMISSION_FILE` по умолчанию
`/run/pdlc-admission/admission.json`. Обязателен свежий integer Unix-seconds guard
receipt: `schema_version`, `installation_sha256`, `namespace_uid`, `namespace`,
`allow_new_operations`, `observed_epoch`, `expires_epoch`, `operation_id`, `epoch`.
Максимальный TTL — 45 секунд. Неверный target, будущая/истёкшая дата и отсутствие
readback закрывают новые CREATE. Старые Pod observation/cancel/cleanup не требуют
нового допуска и сохраняют исходную epoch identity.

## Custody и recovery

Fresh workspace сохраняет прежний путь
`attempt-<attempt UUID>-<generation>-<nonce>/workspace`. Job монтирует только
этот `subPath` в `/workspace`. Labels: `pdlc.installation`, `pdlc.forge-job`,
`pdlc.forge-attempt`, `pdlc.forge-executor=job|builder|workspace-deploy`.
Annotations связывают workspace subPath/PVC UID, admission epoch и operation ID.

Единственный raw Pod имеет `restartPolicy: Never`; Kubernetes Job controller
не используется. Controller сохраняет intent и issued marker до POST. При
потере ответа или restart он читает то же имя/UID, не повторяя create/commands.
Изменение UID, source, installation policy или mount identity удерживает эффект.
Отсутствие Pod, NodeUnknown, timeout клиента и expired lease не доказывают stop.

Finalizer `forge.pdlc.acm-ai.ru/execution-custody` удерживает API object при DELETE
до exact terminated-container readback. Только terminal owner ACK разрешает
снять finalizer, удалить Pod и workspace. Existing supervisor обслуживает
embedded recovery; external startup/`--reconcile-workspaces` восстанавливает
исходную lease, читает/cancel-ит существующий Pod и завершает исходную попытку.

Original external lease token хранится отдельным private raw file mode0600
под controller-only parent0700 вне mounts/artifact roots; no-follow open, без
JSON/log publication. Original secret masks при recovery читаются только из
того же exact-UID Pod env. После ACK/cleanup lease token удаляется.

Expired completion по умолчанию отвергается. Additive failure-only exception
требует прежние runner/token/attempt/latest generation, уже terminal matching
failed/canceled DB projection и exact Kubernetes custody плюс actual stop
readback. Success, stale/foreign identity и новая generation запрещены.
Это actual owner termination ACK, не принятие expiry за ACK; projection,
finished_at и очередь не переписываются. Duplicate ACK идемпотентен.

Локальная pre-crash completion intention не считается server ACK. При принятом
failure/cancel recovery появляется отдельная immutable recovery intention/ACK;
первоначальные bytes сохраняются, acknowledged success не заменяется.
Cleanup intent пишется после durable owner ACK до снятия finalizer. Только эта
связка с prior exact terminal proof разрешает восстановить cleanup при уже
отсутствующем Pod; отсутствие само по себе по-прежнему удерживает эффект.
Embedded recovery использует original secret masks из exact-UID Pod env,
поэтому последующая ротация project secret не раскрывает старое значение в logs.
На shared PVC external startup пропускает чужие embedded leases только по
аутентифицированному receipt readback и отсутствию external-issued marker.


Pipeline artifacts stage/return сохраняют прежние managed pipeline directories;
job не получает общий root. Symlinks и переполнение inventory удерживают работу.
Logs маскируются перед существующим job_logs API/DB. Completed logs читаются потоково без усечения общего объёма; single-line bound
1 MiB удерживает pathological output. Live follow/resumable chunks остаются
отдельным расширением; source candidate передаёт логи при terminal readback.

## Typed image build

Backend сохраняет прежние authenticated task intent, immutable plan,
exact object export, Tracker authority, lease/generation и atomic completion.
Private bounded exporter используется повторно; mutable checkout не является
build context. Builder получает isolated exact source и scoped writable output.
Image создаётся отдельным rootless BuildKit OCI/native Pod и публикуется в
operator registry; job/agent не получает push credential. Registry Secret
допустим только как фиксированный readonly builder mount `/registry-auth`.

Builder: UID:GID1000:1000, configured fsGroup, seccomp RuntimeDefault,
cap_drop ALL, no escalation, readonly rootfs. Writable state — bounded `/tmp`
и `/home/user/.local/share/buildkit`; `TMPDIR=/tmp`,
`XDG_RUNTIME_DIR=/tmp/buildkit-runtime`,
`BUILDKITD_FLAGS=--oci-worker-snapshotter=native`.
Scoped build-output0770 должен принадлежать configured fsGroup; иначе fail closed.
Broad PVC chown, privileged, unconfined и no-process-sandbox fallback отсутствуют.

Текущий native gate на выбранном стенде — **FAIL**, cutover удерживается:
[Feasibility report](KUBERNETES_BUILDER_FEASIBILITY_20261022.md).
Owner обязан отдельно доказать native kernel/userns/security feasibility.
Неудача этого gate удерживает runtime rollout. После build backend сверяет
registry manifest/config digests, source/attempt labels и создаёт прежний
`forge/oci-candidate/v1` descriptor с config image ID. Retained registry candidate
дополнительно связывает manifest ref, Namespace/PVC и original lease/generation.
Descriptor не является deployment или business acceptance.

## Configured workspace deployment

`deploymentRecipes` — bounded operator mapping по exact Git `composeSha256`,
не произвольный apply. Элемент содержит `composeSha256`, `services`; service:
`name`, ровно один `image` (registry digest) / `imageBuildJob` (upstream job key),
`port`, optional `healthPath`, `environment`, `mounts` (named disposable volume
→ absolute app path), `cpuMillis`, `memoryBytes`.

Обязательны unique service names/ports, полный match Git service inventory,
source environment/mount/CPU/memory/user mapping. Build selectors проходят
существующий dependency graph, retained artifact SHA/size, terminal ACK и exact
producer Kubernetes custody. Literal dependency config digest сверяется с Git.
Ненастроенный target, конфликт портов или изменившаяся recipe закрыты до effects.
Production targets автоматически не добавляются.

Один Pod содержит command и app/DB containers. Localhost hostAliases сохраняют
service names внутри isolated Pod network. Controller ждёт dependency readiness,
затем открывает scoped command gate. После validation он останавливает весь Pod,
проверяет stop всех исходных containers и выпускает owner-generated исторический
`forge/workspace-deployment/v1` с `adapter=kubernetes`, producer mapping и
`runtime_state=stopped-after-validation`. Это не current healthy и не business
acceptance. Exact source/task/authority проверяются перед publication.

CPU/memory суммарно укладываются в configured Pod budget. Ephemeral storage:
каждому companion floor(total/(N+1)), execution — остаток; requests=limits.
Disposable shared data volume получает минимум долей его consumers, поделённых
на их количество mounts. Per-service tmp/run — bounded Memory emptyDir.
Kubernetes не предоставляет Docker per-container pids_limit: operator обязан
квалифицировать aggregate Pod/node PID policy для configured target отдельно.

## Проверки source candidate

Нужны scoped Rust check, unit/API-double lifecycle tests, реальные PostgreSQL
expired-ACK ownership/generation/race tests и Compose regression. Установка,
два namespace real jobs, rootless build/push, configured deployment, namespace
isolation и recovery через настоящий API — отдельный operator gate.
