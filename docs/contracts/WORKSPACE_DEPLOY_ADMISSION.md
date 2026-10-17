# Task Workspace Deploy

Статус: source candidate trusted adapter и отдельного common integration patch.
Python host tests не квалифицируют Rust/PG/Engine. Установка, native запуск и
business acceptance этим документом не заявлены. До common patch прежний parser
остаётся закрытым. Readonly OCI v1, Auth, SDK и workflow transitions не меняются.

## Существующий Pipeline

В `.forge-ci.yml` v1 авторизованный task pipeline может объявить обычные repo-owned
`commands` вместе с закрытым spec:

```yaml
workspace_deploy:
  compose: deploy/task.compose.json
  images_manifest: reports/workspace-deployment.json
```

Это временный CI-owned workspace, не постоянный application target. Поле
`images_manifest` сохраняет имя из первоначального spec, но теперь задаёт путь
owner-generated output receipt: repository файл по этому пути не является proof.
Автоматически публикуется только этот receipt. `image_build`, пользовательские
artifact paths, `allow_failure` и external runner tags в том же job запрещены.
Нужны непустые repo commands; legacy job не может проигнорировать declaration.
У обычного job поле spec отсутствует в plan JSON, прежний plan не меняется.

Generic trigger без task intent отклоняется. Existing external runner уже
исключает task pipelines. Новых routes, credentials, issuers, tables, queues,
controller или caller ACK нет. Original task submission claim выполняется один
раз до подготовки deployment resources; restart не повторяет unknown invocation.

## Git И Image Provenance

Compose принимается только как JSON regular Git blob из точного admitted commit
и path immutable plan. Symlink/gitlink, mutable checkout, duplicate keys, deep/
oversize/nonfinite JSON и unknown fields отклоняются. Existing Git object hash,
config/plan hashes, repository ID, task/assignment и active embedded lease
проверяются до и после чтений. Backend не принимает caller-supplied image registry.

У service ровно один selector:

- `image: sha256:<64 lowercase hex>`: уже имеющаяся literal dependency;
- `x-forge-image-build: build_app`: существующий transitive upstream job key.

Extension не URI, не новый issuer и не env interpolation. Backend находит unique
artifact текущего generation того же task pipeline/commit, проверяет retained
artifact bytes/hash, original build intent/completion, terminal ACK и cleanup ACK.
Actual image labels сверяются прежним owner resolver. Extension удаляется при
рендере, вместо неё подставляется immutable `image`. Literal dependency сохраняет
origin `owner_inventory`, а не получает вымышленные source/build labels.

Actual ID, Linux/architecture, Config.User и Config.Volumes читаются на том же
verified daemon. Pinned missing ID не pull-ится. Optional Docker 29 metadata
читается через Go `index`. Config.Env и credentials не экспортируются.
Original Compose bytes не переписываются; Git blob/hash, rendered fragment hash,
final private manifest hash, producer mapping и repo commands hash фиксируются
в прежнем private per-attempt control directory.

## Закрытый Compose Subset

Разрешены только `services` и optional `volumes`. Не более 8 services и 8 volumes.
Service требует selector, `mem_limit` в bytes, `cpus`, `pids_limit`; optional:
`user`, `environment`, `volumes`, `depends_on`, `read_only`. Per-service bounds:
16 MiB..2 GiB, 0.1..2 CPU, 16..512 PID; суммарно 4 GiB/4 CPU/2048 PID.

Privileged, host network/PID/IPC, devices, capabilities/security overrides,
build, entrypoint/command, extra_hosts, env_file, ports, external secrets/configs/
networks, bind mounts и Docker socket запрещены. Нет host port publication.
Application endpoints доступны repo command container по service DNS внутри
того же temporary job network; бизнес probes определяет репозиторий.

`user` может явно задавать положительные numeric UID:GID из repo config. Без
override нужен фактический numeric nonroot Config.User; root/пустой/именованный
default не превращается в угаданный UID. Rootfs всегда read-only, cap_drop ALL,
no-new-privileges, restart=no; owner tmpfs ограничены `/tmp` и `/run` по 16 MiB.
Repo не может изменить owner resource names/labels или затенить job/transfer/
workspace/cargo/sources. Implicit image volumes должны быть полностью покрыты
declared mounts. Volumes только disposable named `{}` без external/name/driver.
Для общего volume требуется один UID:GID, unused volumes отклоняются.

Env допускает literal либо ровно `${DECLARED_PROJECT_SECRET}`. Нет default/mixed/
shell expansion или process-env fallback. Значения поступают из существующего
project-secret resolver только для declared names текущего job. Backend private
bridge использует isolated `python3 -I`, compiled admission/adapter sources,
очищенный environment и bounded private stdin/stdout; это renderer, не permission
producer. Jobs не получают Docker/TLS credentials или control journal. Private
manifest с secret values не публикуется; literal escaping выполняет прежний
`RemoteJob.save_manifest` ровно один раз.

## RemoteJob Lifecycle

Переиспользуется существующий per-attempt RemoteJob Compose project и job network.
Recipe не задаёт subnet/IPAM и не доказывает отсутствие внешних host routes.
Нового allocator нет; actual network ID/IPAM сохраняются в owner observation.
Существующие verified endpoint/daemon/foreign-storage guards обязательны.

Перед effects проверяются original submitted task/assignment/lease и monotonic
deadline. Existing helper готовит только новые собственные volumes: существующее
имя блокирует prepare; actual helper mounts/volume labels проверяются до chown.
Фиксированный helper script требует пустой directory и non-symlink, устанавливает
ровно numeric UID:GID и mode 700, затем проверяет результат. Нет recursive chown
данных, reset/replay или выдуманного caller ownership ACK.

Private intent записывается create-new до Compose create. Применяется только
owner-rendered fragment: `up --no-start --no-deps --no-build --pull never`, затем
`start` точного service set. Created и Running observations сохраняют actual
container IDs/images/start epochs, user, mounts, security/resource settings и
network ID. Неизвестный ACK не превращается в success или fresh PID adoption.
Перед repo commands выполняется свежий authority/lease readback; command timeout
не выходит за исходную authority deadline. Existing cancel/termination path
останавливает все execution services, включая DB/app, кроме transfer helper.

До stop/delete сверяются исходные IDs/images и, после Running observation,
StartedAt. Changed identity или incomplete observation удерживает исходный
journal. Reconcile читает original observations; unknown create без created
observation не разрешает stop по новым IDs или повтор create/start.
После commands все services останавливаются, actual stopped readback должен
совпасть с исходной Running observation и сетью. Failed/lost stop ACK удерживает
completion. Transfer остаётся только для прежнего copy-back до cleanup.

## Historical Receipt И Cleanup

Owner-generated `forge/workspace-deployment/v1` receipt расположен вне job mounts.
Он связывает exact task/source/attempt, producer mapping, command/manifest hashes,
actual container/network observation и process exit code. `validation=verified`
означает exit 0 repo commands, а не Forge-coded HTTP/business acceptance.
Для ненулевого exit используется `validation=failed`. Runtime state всегда
`stopped-after-validation`, никогда current healthy после cleanup.

Fresh publication authority и existing atomic terminal success CAS сохраняют
исходные lease/attempt/job/queue guards. Только durable terminal ACK разрешает
прежний scoped Compose down с disposable volumes. Никаких permanent Pulse
workspace, production/default-ref изменений или application-specific CRUD/migrate
проверок в Forge нет. PM requirements/confirmation предшествуют repo business
tests. Этот receipt сам по себе не является Tracker transition или delivery
acceptance; соответствующую consumer integration квалифицирует её владелец.

## Проверки

Host tests выполняют pure validator/renderer и реальный isolated Python bridge с
synthetic values. Rust regressions охватывают selector exclusivity, source binding,
actual configuration/mount drift, original ID/start/network custody, unknown
create reconciliation и all-service termination. Без Cargo эти Rust tests не
объявляются прошедшими. PG/Engine/native evidence должно быть получено отдельно.
