# Workspace Deploy: Validation Only

Статус: parser/type-validation source slice, execution **закрыт**. Не является
Docker sandbox, native qualification, разрешением deploy или SDLC acceptance.

## Parser

Предлагаемый закрытый spec у существующего job:

```yaml
workspace_deploy:
  compose: deploy/task.compose.json
  images_manifest: reports/images.json
```

Только разные portable relative paths; unknown fields и `null` отклоняются.
Даже корректный spec возвращает planning error до executable jobs/queue. Его
нельзя молча проигнорировать и выполнить `commands` как обычный job. Старые
command/image_build jobs и их plan JSON остаются прежними. Нового parser version
или executable action этот validation-only slice не публикует.

## Pure Recipe Check

`scripts/workspace_deploy_admission.py` выполняет только проверку памяти/bytes,
без файлового IO, Docker, subprocess, network или разрешения secret values.
Первоначальный subset принимает только JSON Compose, не произвольный YAML.
Duplicate keys, nonfinite numbers, oversize/deep/invalid Unicode отклоняются.

OwnerSeal — только доверенная server-side projection exact source commit,
двух actual byte hashes, verified service/image registry, разрешённых env keys
и declared project secret names. Это НЕ подпись и НЕ producer authority. Его
нельзя принимать из HTTP, repository report или job. Создание такого projection
из настоящего Git/artifact/owner readback ещё не подключено.

Manifest содержит ровно `schema=forge/workspace-images/v1`, `sourceCommit`,
`images`. Service/image map обязан точно совпасть с owner registry; image только
фактический `sha256:` ID, без tag, pull, mutable reference или interpolation.

В Compose разрешены только `services` и необязательный `volumes`. Service:
`image`, обязательные bounded `mem_limit` (bytes), `cpus`, `pids_limit`, optional
`environment`, `volumes`, `depends_on`, `read_only`. Не более 8 services/volumes;
per-service 16 MiB–2 GiB, 0.1–2 CPU, 16–512 PID; суммарно 4 GiB/4 CPU/2048 PID.
Нижние owner quotas по-прежнему обязательны у будущего executor.

Все остальные поля fail-closed, включая privileged, host network/PID/IPC,
devices/cap_add/security overrides, build, entrypoint/command, extra_hosts,
env_file, ports, external secrets/configs/networks. Начальный subset не разрешает
никаких host port allocations. Сеть и её explicit IPAM сможет задать только owner.
Volumes — именованные logical resources с `{}`; нет external/name/driver options.
Mounts только long-syntax declared volumes, без host binds/socket/system targets.

Env принимает literal strings либо ровно `${DECLARED_PROJECT_SECRET}`. Только
owner-approved service/env key и secret name; нет shell/default/mixed expansions,
DOCKER_/CICD_RUNNER_ keys или process-env fallback. Values здесь не разрешаются.
Typed result содержит только hashes/names/secret refs: не команду исполнения.

## Неподключённая Граница

`RemoteJob::prepare/job_command/compose_job` управляют существующим command
container, но не выполняют sealed recipe admission и owner-controlled запуск его
services. ComposeHelper v2 управляет ownership/cleanup, а не безопасностью recipe.
Перед execution нужны отдельный trusted adapter, actual registry/Git hash readback,
private immutable manifest, owner limits/network/IPAM, исходная attempt/task lease
и terminal/publication custody. Job не получает Docker socket/TLS/control journal.

Нельзя просто передать repo JSON в Base `write` или исполнить `docker compose`
из `commands`. Recipe validation не подтверждает daemon, actual image Config,
resource ownership, source provenance, task permission, stop ACK или cleanup.

Repo-owned commands/acceptance определяются только после PM confirmation.
Forge не знает бизнес API Pulse и не вводит ему migration flag. Существующий
readonly OCI executor, PG engine, common registrations, Fleet consumer, Auth
и task workflow transitions этим пакетом не меняются.

## Unconnected Owner Units

`runner_image_build/workspace_deploy.rs` — неподключённый child module существующего
image-build owner. Он читает реальные terminal upstream job/attempt/artifact records,
private intent/completion/cleanup ACK, сверяет Git config/plan и actual built image
прежним Client. Returned registry связан также с consumer attempt/source/daemon;
чтение не выдаёт permission, не обновляет lease и ничего не запускает. Constructor
из repo JSON/caller `verified` отсутствует. Общая registration остаётся у владельца.

`scripts/workspace_deploy_adapter.py` — pure owner-side assembly private service
fragment после проверки recipe. Он подставляет только разрешённые project secrets,
owner UID/quotas/tmpfs, explicit subnet и resource names; запрещает shadowing job/
transfer/workspace/cargo/sources, implicit undeclared image volumes, writable root
и пересекающиеся mounts. Fragment содержит секреты и не является public artifact.
Финальный literal escaping должен выполнять прежний `RemoteJob.save_manifest`
ровно один раз; передачи неразрешённого repo JSON в Docker здесь нет.

Execution не подключён; owner отдельно интегрирует child registration и read-only
runner hook. Rust/DB/Engine qualification пока нет. Registry сохраняет реальные
upstream backend image-build receipts отдельно от owner inventory dependencies,
не присваивая dependencies fake source/attempt labels. Config readback описан ниже.
Полный DB-stack путь здесь не объявлен готовым. Git bytes и lifecycle plumbing
описаны ниже;
они сами не подключают deployment execution или acceptance.
Существующий readonly descriptor подтверждает здесь только image provenance:
он не перелэйбливается и не доказывает совместимость с изменяемой БД/migrations.

## Exact Git Source And Existing Journal

Неподключённый `read_owner_compose_source` получает Compose path только из
immutable plan текущего task pipeline. Existing current-plan readback сверяет
project/repository/source intent DevOps, active embedded attempt/lease, exact
Git `.forge-ci.yml` bytes и plan hash. Затем existing sealed Git helpers читают
literal path из exact commit: один regular blob, не symlink/gitlink,
не mutable checkout. Bytes ограничены 256 KiB и проверены по Git object ID;
после чтения plan/lease наблюдаются снова. Внешний deadline ограничивает весь IO.
Typed result не принимает caller `verified` и не переиспользуется другой attempt.

Private projection содержит job/attempt, commit, plan hash, path, Git blob ID
и SHA-256. Pure Python assembly требует эту projection до secret resolution и
проверяет оба byte hashes, source и attempt. Projection не является подписью или
новой authority: её нельзя принимать из agent/job/API body. Read-only bridge ниже
не заменяет fresh Tracker admission перед effects.

У существующего `RemoteJob` штатные journal/manifest writes теперь awaited
`spawn_blocking`, с прежними create-new private temp, file sync и rename. Один
per-object lock и write sequence не дают отменённой queued write затереть более
новую phase. `Drop` сохраняет прежний synchronous fail-closed marker, закрывает
late writes и не получает права stop/delete/terminal ACK. Это прежний private
RemoteJob journal, не Base v2 и не новая registry. Остальные filesystem reads и
initial directory setup этим source slice не объявлены полностью async.

Existing stop/readback выбирает все фактически наблюдаемые declared services,
включая application/database, а не только `job`. Только `transfer` оставлен для
copy-back до cleanup. Unknown/duplicate service, invalid ID и unknown/running/
paused/restarting state удерживают termination. Partial creation/removal может
дать отсутствующие containers: это не replay и не доказательство app readiness.
Engine ownership/foreign-storage checks и terminal DB ACK перед scoped Compose
down остаются прежними; lost stop ACK не превращается в success.

Parser по-прежнему всегда отклоняет `workspace_deploy`. Native service creation,
protected owner network allocation, execution
authority/lease monitor, repo-owned acceptance и publication custody ещё не
подключены. Rust/PG/native qualification этого расширения не заявлена.

## Dependency Inventory And Closed Runner Hook

`Client.runtime_image` принимает только exact `sha256:` selector и использует
прежний owner `Client.image` resolver. Для immutable ID отсутствующий image не
pull-ится. Actual ID, Linux/platform, Config.User и Config.Volumes читаются
bounded Engine readback на том же daemon; daemon identity/platform проверяются
до и после. Readback ограничен 64 KiB/30s, общий read-only preflight — 30s.
Optional User/Volumes Docker 29 читаются через Go `index`; отсутствие User
сохраняется как null, а не превращается в угаданный UID. Нет Config.Env, keys
или credentials в projection. Existing readonly OCI/build volume guard не ослаблен.

Recipe IDs берутся только из exact verified Compose Git bytes. Existing upstream
artifact/intent/terminal/cleanup checks подтверждают подходящие build receipts;
прочие images остаются явно owner-inventory dependencies. Для каждого выбранного
image actual Config читается повторно, plan/lease/source и project repository ID
наблюдаются снова. Backend создаёт собственный images witness; caller/job report
не становится proof. `images_manifest` пока только declared witness path: файл
не публикуется и не принимается за mutable input authority или delivery receipt.

`inspect_requested` предназначен для existing embedded runner перед checkout,
secrets и RemoteJob.prepare. Он не claim-ит task submission и не создаёт resources.
Для обычного job возвращается None; наличие workspace_deploy, включая null,
никогда не разрешает command fallback. Общий owner hook/registration интегрируется
отдельно; нового dispatch/controller или table нет.

Python bridge запускается isolated `python3 -I` с очищенным environment и
компилированными в backend исходниками admission/adapter, не с repo paths.
Private stdin ограничен 1 MiB, stdout — 4 KiB, stderr не публикуется. Он проверяет
actual Config facts и оба source hashes, но может вернуть только фиксированный
HOLD. Missing interpreter/metadata/unknown result не открывает execution.
Host tests запускают именно этот Python bridge; они не квалифицируют Rust/Engine.

Implicit image volumes должны быть явно покрыты declared owned mounts. Наличие
mounts либо root/пустой/именованный User требует trusted volume/user preparation:
UID и права на новый volume нельзя выдумать. Даже volumeless nonroot recipe без
secrets остаётся HOLD до owner network/lifecycle wiring. Нет fake subnet, secret
values, volume ownership или готового deployment. Полный launch не включён.

Текущий literal image-ID subset также не задаёт owner reference для нового image,
собираемого из того же commit: его будущий ID нельзя заранее записать в Git.
Будущая минимальная extension должна связать такой reference с существующим
upstream image_build receipt; static fixture/старый image не заменяют это proof.
Новый selector, migration engine или Pulse business acceptance здесь не добавлены.
