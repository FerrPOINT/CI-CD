# Native qualification общего Forge Kubernetes executor

Этот ignored integration test вызывает production Execution, а не kubectl или
mock transport. Он не запускает сервер Forge и не подменяет серверный lease ACK.
Проверка HTTP runner protocol и реального текущего CI-задания остаётся отдельным
owner gate. Image build и workspace deploy не входят в этот fixture.

## Вход и source binding

Owner запускает только на изолированном fixture с проверенными namespace/PVC UID,
job ServiceAccount, policy и точным immutable image из своего graph receipt.
Existing node и production namespace/PVC helper не создаёт и не изменяет.
Owner удерживает общий execution slot и исключительное владение fixture на весь запуск.
Образ должен штатно читаться выбранным job/node либо анонимно: ordinary Pod имеет
imagePullPolicy: IfNotPresent, без imagePullSecrets. Helper не меняет credential
verification policy, кеш или native admission. Clean-cache pull не заявляется.

Приватный JSON, owner UID, mode 0600, без aliases/hardlinks, не более 64 KiB:

| Поле | Значение |
| --- | --- |
| schemaVersion | 1 |
| kind | forge/native-kubernetes-executor/v1 |
| operationId | UUID, совпадающий с текущим admission grant |
| expectedSourceHead | полный SHA immutable Forge source, из которого собран тест |
| expectedExecutorSha256 | SHA256 байтов backend/src/execution/kubernetes.rs |
| runnerConfig | объект path (absolute), sha256 (байты runner JSON) |
| expectedConfig | полный публичный Config JSON; строго равен Config::from_env |
| image | квалифицированный immutable OCI reference с @sha256 |
| sourceCommit | 40 lowercase hex fixture identity; Git checkout этим тестом не проверяется |
| evidenceDirectory | новый absolute путь вне /workspaces, canonical existing parent |
| caseTimeoutSeconds | 60–180; наблюдение одного случая |

expectedConfig использует текущий camelCase schemaVersion=1 из
[KUBERNETES_RUNNER.md](KUBERNETES_RUNNER.md). builderEnabled=false,
deploymentRecipes=[]. Namespace, namespaceUid, clusterCaSha256, workspaceClaim,
workspaceClaimUid, installation и jobServiceAccount берутся из owner fixture,
а не production. ConfigMap projected config поддерживается штатно.

Обязательный runtime env:

- FORGE_NATIVE_EXECUTOR_INPUT: путь к приватному JSON;
- FORGE_NATIVE_EXECUTOR_INPUT_SHA256: SHA256 его байтов;
- CICD_KUBERNETES_RUNNER_CONFIG: тот же путь, что runnerConfig.path;
- CICD_KUBERNETES_ADMISSION_FILE: owner-produced свежий grant существующего ABI;
- KUBERNETES_SERVICE_HOST, KUBERNETES_SERVICE_PORT_HTTPS: штатный HTTPS API;
- CICD_QUIESCENCE_DIR отсутствует: fixture не регистрирует приложение в backup barrier.

При компиляции owner устанавливает FORGE_NATIVE_EXECUTOR_BUILD_HEAD равным
exact exported source SHA. Тест сверяет его с input и include_bytes! SHA
скомпилированного production executor. Отсутствующее/чужое значение закрывает
native run. Это source receipt owner-а, а не заявление о прочтении Git runtime.

Owner предоставляет стандартные readonly
/var/run/secrets/kubernetes.io/serviceaccount/{token,ca.crt} и существующий
RBAC controller-а: name-scoped Namespace GET, свой PVC GET, собственные
Pods GET/LIST/WATCH/CREATE/PATCH/DELETE и pods/log GET. Secret GET/LIST/WATCH,
host socket и host namespaces не нужны. Observer имеет только GET/WATCH; все
CREATE/cancel/finalizer/cleanup выполняет настоящий общий Execution.

/workspaces — controller mount отдельного fixture PVC, pinned UID из Config,
canonical root, уже выставленные configured fsGroup + group rwx + setgid.
UID/GID процесса должны совпадать с configured job runAsUser/runAsGroup:
fresh private checkout 0700 не требует broad chown. Job монтирует только
attempt-<attempt UUID>-<generation>-<nonce>/workspace в /workspace.
Evidence mount находится вне этого PVC. Helper не рекурсивно меняет PVC права.
Preflight отклоняет existing Forge Pods и любую незакрытую execution/workspace
custody; owner дополнительно подтверждает принадлежность PVC/namespace fixture.

В текущем guard ABI grant живёт максимум 45 секунд. Owner обеспечивает обычное
обновление на протяжении fixture с тем же operationId; helper не пишет grant
и не создаёт искусственную admission epoch.

## Команда owner gate

Из exact exported backend/, в закреплённой Rust 1.88 / SDK 2ad среде:

~~~sh
FORGE_NATIVE_EXECUTOR_BUILD_HEAD=<exact-source-SHA> \
cargo test --locked -p cicd-server --features integration \
  --test kubernetes_native_executor \
  actual_kubernetes_executor_native -- --ignored --exact
~~~

Сначала допустим scoped compile test target без native run. Pure tests
private_abi_rejects_wrong_build_and_builder_scope и
exact_native_quantities_accept_canonical_neighbors_reject_mismatch выполняются без сети.
Обычный cargo test не запускает ignored native case. Тест не создаёт app image,
не вызывает registry publisher, Docker, kubectl, CRI CLI или shell с operator input.

## Случаи и cleanup

Последовательно выделяются отдельные job/attempt/lease UUID и generation=1.
Команды фиксированы в исходнике: shell пишет синтетический marker через настоящий
PVC и log; длительные случаи затем выполняют exec sleep.

1. Success: exit 0, точные байты artifact, callback настоящего stream_logs.
2. Timeout: процесс уже записал marker; native Pod DeadlineExceeded и ненулевой
   exit после activeDeadlineSeconds=5.
3. Cancel: процесс ещё running, затем public cancel и полный terminal proof.
4. Reopen/reconcile: после actual start marker открываются сохранённые
   OwnedWorkspace/Execution, сохраняется тот же Pod UID; новое CREATE не нужно.
5. LostCreateAckBeforeCustodyCommit: READONLY native WATCH от list resourceVersion
   наблюдает ADDED exact Pod. Только незавершённый caller future actual start()
   отбрасывается. Должно быть реально видно issued.json без created.json; после
   reopen/reconcile сохраняется тот же watched UID. Custody JSON не редактируется.

Пятый случай — узкое реально наблюдённое окно потери локального durable ACK,
не proof потери HTTP-пакета. Если start успел сохранить created.json раньше
WATCH, окно не достигнуто: случай HOLD и общий результат не PASS, даже если
остальные случаи PASS. Случаи не пропускаются ради зелёного результата.

После каждого случая helper читает полный terminal proof через executor и
проверяет exact Pod UID/namespace/PVC/subPath, operation, образ, SA и основные
native security/mount boundaries. Fixture-owner ACK сохраняется приватно до
public acknowledge(). Только после подтверждённого terminal состояния и
точного UID cleanup проверяется исчезновение API Pod, затем public workspace
completion/ACK/cleanup и finish_workspace_cleanup(). Это не server lease ACK.

При ошибке helper пытается только public cancel/observe для original custody.
Отсутствие Pod никогда не доказывает остановку. Неизвестное состояние, замена UID,
never-started Pod без полного stopped proof, ошибка cleanup или deadline —
HOLD с сохранёнными custody/finalizer/workspace; последующие случаи NOT_RUN.
Нельзя выполнять force delete, снимать finalizer вручную или запускать fixture
заново поверх этих evidence/custody. Owner сначала разбирает exact native state.
Cleanup имеет отдельный bounded stop budget 90 секунд и общий budget 150 секунд;
превышение не означает остановку процесса.

В новом private evidenceDirectory сохраняются preflight.json, по каждому
случаю scope/native-identity/output/fixture-owner-terminal-ack/result.json и
точные синтетические bytes result.txt, затем общий result.json. Оставшаяся
закрытая .kubernetes-control evidence не удаляется. Токены/config credentials
и operator secret values не записываются. Result отдельно указывает
serverLeaseProtocolQualified=false, sourcePreparationQualified=false,
cleanPullProven=false. PASS требует всех пяти случаев PASS и полного cleanup.
Этот документ описывает source contract; native выполнение подтверждает owner.
