# Public-safe exact-source full12 controls

Этот source-only successor имеет непосредственный parent
`0527882c371ccc4247e2d2f3d74a9a8e8f74ef78`; все exact parent edges предыдущих
controls сохранены в `host.controls_history`, включая maintenance controls
`b9375dc4b4f070b0f6cff1733228f1a6865b1368`, safety controls
`4d97c54f35b485224a8229495763658fcbbd40e9`, qualification controls
`632ea8347602fe4af22e7369790ed9c75e53f76a`, original public-safe controls
`1dbedf85242c3540b70005ce5f0c20badb682c41` и product source
`25be2e82d4d42673897c8a16070eb8a9519244f0`. Это normal history, не импорт
retired/private controls. Product SDK остаётся
`19a7a381ae6dbea61a643bb96189e483fa64df5c`; product265, locks и schema неизменны.

## Closed Dependency

`maintenance-pin.json` v2 связывает merged Base commit
`66b7fafdee47ada41f663f07af2bfdf32363e467` и historical approval ref
`refs/heads/main` в private `FerrPOINT/services-base` (merged PR183).
Отдельный actual read-only qualification подтверждает точный tip до/после, full history, три Git
blobs и raw SHA256. `published_exact_commit` относится только к этой dependency,
не к native/full12 acceptance. Private717, installed packet, рабочие файлы,
SDK19a и старые helper hashes не являются fallback.

Exact66b7 прошёл существующий Linux qualifier в уже работающем Ubuntu WSL:
fresh authenticated main до/после, отдельный fetched full-history Git checkout,
три regular-file mode/blob и raw SHA256, идентичные квалифицированному660.
Опубликованные43,63 и660 не являются текущим candidate и отвергаются при новой qualification.
Product SDK19a и installed packet не менялись. Эта metadata qualification не
означает успешный hosted run, recovery capacity или native/full12 acceptance.

Новый Base contract сохраняет local-v1 default30GiB. Только reviewed disposable
hosted callers передают explicit `isolated-ci-v1`: parent/cache напрямую, native
sessions через узкий hosted adapter. Native adapter повторно проверяет hosted
admission/task/daemon/context до original constructor; Base defaults не меняются.
Реальный host reserve108279229428 и `SDLC_MIN_FREE_GIB` не снижены до5GiB.
Canonical absolute registry/output и original owner/cleanup semantics сохранены.

### Проверка candidate без записи

Из аутентифицированного full-history checkout с доступными exact66b7 objects проверить:

```sh
python3 -B scripts/hermetic_full12/maintenance_git.py qualify \
  /absolute/private/services-base 66b7fafdee47ada41f663f07af2bfdf32363e467 \
  refs/heads/main
```

Команда использует существующий Linux owned-process helper, включая WNOWAIT и
group cleanup: общий Git work budget 60 секунд и неизменный teardown tail.
Проверяются canonical origin до сетевого запроса, exact configured commit/ref,
опубликованный branch tip до и после readback, full history,
commit, три regular-file blob и raw SHA256 (без EOL normalization).
Вывод содержит только candidate pin JSON либо фиксированный отказ
`MAINTENANCE_PIN_UNQUALIFIED` (exit1). Нет fetch, установки/import helper, записи
pin, материализации private files, cleanup или допуска full12. Изменение branch
tip во время qualification readback запрещено. После approval ref сохраняется
как historical provenance, не как runtime freshness predicate: движение или
удаление branch не меняет approved commit/bytes. Отсутствующий approved object
закрывает runtime без fallback на новый tip. Приватный код
и credentials не входят в публичный checkout или artifacts.

Текущий checkout guard требует exact normal chain из `host.controls_history`,
full history, task-owned modifications относительно9e и add-only controls относительно25be.
Предыдущий workflow/host branch: `build-only/forge-bootstrap-full12-20261010`.
Изменение родителя в следующем reviewed commit требует сохранить всю chain,
не заменить проверку на произвольный ancestor. Candidate JSON и installed bytes
не доказывают native/SDLC acceptance. Source guards больше не блокируют из-за
старого branch/parent/null pin. Hosted execution остаётся отдельным разрешением
после review/publish controls, private checkout access и фактического admission.

Три hash связывают v2 helper с cleanup/installer. Политика Base сохраняется: exact
owned Compose down в finally, v2 daemon/manifest/owner проверки до удаления
disposable volumes, сохранение external caches/runtime/backups/evidence,
installer audit до регистрации scheduler. Global Docker-group audit остаётся
workspace контрактом Base (`scripts/audit_docker_groups.py`), а не разрешением
cleanup или копирования дополнительных private files в packet. Disk/RAM guards
обязательны; native execution здесь не разрешён.

Существующий authenticated SDK checkout сохраняет timeout2min/fetch-depth0;
его object database содержит SDK19a и отдельный maintenance commit. При каждом
raw readback проверяются strict origin, exact approved commit и три mode/blob/raw hashes,
без runtime tracking-tip equality, дополнительного token, registry fallback
или сетевого запроса из runtime. Existing SDK19a full-history checkout получает
merged66b7 через main; никакого дополнительного checkout action нет. Seals/aggregate связывают
repository/commit/ref/три hashes. Только metadata входит в public evidence.
Private bytes остаются в памяти/ephemeral VM, не в public Git/logs/artifacts.

Оригинальные native QA и observer source bytes не переписаны. Перед original
`load_sdk`/smoke binding адаптируется только expected `SDK_SHA256` из квалифицированного
Git proof; original loader, journal guards и assertions остаются строгими.
Private bridge proof использует ту же привязку. Его pure private-SDK cases не
запускают Docker и не заменяют full12 execution.

## Execution Contract

### Закрытая диагностика OCI

Только job C, stage `oci` и исходный `CommandFailure(101)` могут добавить optional
`error.oci_diagnostics` в существующий report. Нового artifact нет. Strict reader
должен использовать frozen `host.validate_safe_error`: вложенные ключи только
`compiler` и `panics`, каждый содержит до восьми уникальных записей. Compiler
record содержит только `code`, `file`, `line`, `column`; panic record вместо
`code` содержит exact `host.OCI_TEST` в `test`. Legacy errors остаются валидными;
неизвестные ключи, типы, пути и identities отвергаются.

Принимается только соседняя пара Rust `error[Edddd]` / source location или
Rust 1.88 panic header exact известного теста. Имена исходников берутся из
существующего frozen Rust source catalogue с exact workspace aliases.
Перед projection размер и SHA256 исходника должны совпасть с catalogue,
а location должна находиться в этом исходнике. Это attestation source/location,
не личности производителя лога: test output может имитировать compiler syntax.
Коды являются наблюдаемыми `E` плюс четыре ASCII-цифры, не независимо
аутентифицированным диагнозом compiler. Произвольные stack frames и panic
messages не сохраняются.

Читается только existing private `oci.log`, в памяти, до 1 MiB / 16384 строк.
Чтение лога и соответствующих исходников отвергает symlinks, hardlinks, чужого
владельца или device, nonregular files и изменившиеся metadata. Отсутствующие,
слишком большие, malformed или unattested observations не добавляют диагностику
и не скрывают исходный отказ. Existing finalization deadline остаётся обязательным
и propagates. Messages, snippets, panic bodies, SQL, args, env и raw log bytes
не копируются в report, marker или artifacts. Cleanup, все двенадцать stage
assertions, pins, resource floors и deadlines неизменны.

Текущий push-only workflow требует fresh A и B перед C. Запуск только C на новом
SHA невозможен без отдельно reviewed workflow change. Минимальный запуск без
изменения workflow: fresh full12 attempt после approval; historical A/B receipts
38058842111 не квалифицируют successor. Будущая явно разрешённая C-only
диагностика не будет full12 acceptance. C-only input, job, rerun или изменение
workflow здесь не реализованы.

### Безопасная bootstrap диагностика

Новый normal successor добавляет только optional `error.bootstrap_step` из закрытого
`host.BOOTSTRAP_STEPS`: download, dependencies, user namespace/manager, rootless
launch/context/socket readiness, identity guards, baseline и admission seal.
Это граница отказа, не доказательство root cause. Legacy error без hint остаётся
валидным; `host.validate_safe_error` отвергает unknown hint, лишние поля и hint вне
stage=bootstrap. Strict external readback должен вызвать этот validator, не принимать
произвольный exception attribute или текст как категорию.

Hint оборачивает только existing bootstrap operations. Исходные category/errno и
bounded exit_code сохраняются; SQL, warnings, paths, IDs, stdout/stderr и private
journals не копируются. Existing `CapacityFailure` сохраняет исходный exception и
structured capacity report без нового hint. При post-bootstrap ошибках также
сохраняется исходный exception.
Команды, их порядок, guards, socket90s и остальные deadlines/resources неизменны.
Новых Docker/systemd/journal inspection, retries/fallback или запуска gates нет.
Historical b937 failure не переписывается и не объявляется исправленным.

Normal child frozen3f добавляет fixed bootstrap hints только между сохранением
projects и cache: `project_ownership`, `disposable_prepare`, `image_pull`,
`image_readback`, `tools_build`, `tools_readback`, `cache_prepare`, `cache_fetch`,
`cache_cleanup`, `cache_seal`. Labels не содержат image refs, private paths,
command output или exception text; pull/readback labels общие для трёх pinned
images. Commands, порядок, timeouts, cleanup/finally и cache assertions прежние.
Receipt38039120713 доказывает admission/daemon/projects, но не cache/execution
seal, конкретную причину отказа или physical stage resource proof. Новый child
локализует следующую ошибку; он не выдаёт неизвестную причину за исправленную.

## Resource Admission Successor

Normal history `eb91d4f` -> `3a9bbaf` -> inode-guard follow-up использует hosted-only branch
`build-only/forge-delegation-full12-20261010`. Product25be, SDK19a, maintenance43,
265 source inputs, 12 stages, manifests, budgets и cleanup неизменны.
`Warnings` больше не является blanket admission predicate: это информационный
нестабильный текст Docker, он не парсится и не экспортируется.

Обязательны cgroup version `2`, driver `systemd` и exact boolean `True` для
`MemoryLimit`, `CpuCfsQuota`, `CpuCfsPeriod`, `PidsLimit`. Missing/null/false/числа/
строки не принимаются. Отказы имеют отдельные fixed hints
`identity_cgroup_version`, `identity_cgroup_driver`, `identity_cgroup_resources`.
Старый `identity_cgroup_warnings` остаётся допустимым только для readback старых
receipts; новый код его не выдаёт. Stage entry/exit сохраняет повторный admission.

До enable-linger/start manager/rootless launch bootstrap публикует runtime-only
drop-in `/run/systemd/system/user@<exactuid>.service.d/90-forge-full12-<token>.conf`
с `[Service] Delegate=cpu memory pids`. Уникальный token и intent сохраняются до
root effect. Root-only Python helper проверяет identity/root-owned nofollow
directories, полностью пишет и fsync файл в hidden owned directory, затем
публикует completed inode через atomic no-overwrite hard link и directory fsync.
Existing foreign drop-in не перезаписывается. Shared drop-in directory не удаляется.

После `daemon-reload`/start обязательны systemd source readback exact DropInPaths,
Delegate=yes, DelegateControllers cpu/memory/pids, exact ControlGroup и actual
`cgroup.controllers`. Separate fixed labels: manager_dropin/reload/linger/start/
readback/controllers. Старый manager остаётся readback-compatible, но не emitted.
Нет dynamic Delegate setter, restart/reboot/fallback или persistent `/etc` изменения.
Systemd255 разрешает этот D-Bus setter только для transient UNIT_STUB, не loaded
user@ service; это source-proven defect eb91. Исторический manager failure receipt
не локализует точный subcommand и не переписывается как доказанный root cause.

Independent finally после остановки own daemon удаляет только published inode,
совпадающий с completed owned source и exact bytes/token, затем own source/directory
и daemon-reload. Он работает и без daemon-owner marker (ошибка bootstrap до launch).
Foreign replacement inode сохраняется и закрывает cleanup; corrupt/missing ownership proof закрывает
cleanup, не создаёт PASS. Cleanup result delegation_removed обязателен в aggregate.
Это только disposable GitHub-hosted VM после existing host/source qualification,
не инструкция менять локальный daemon. Docker rootless требует v2/systemd и CPU:
[Docker](https://docs.docker.com/engine/security/rootless/tips/#limiting-resources),
[systemctl255](https://github.com/systemd/systemd/blob/v255/man/systemctl.xml),
[Delegate setter255](https://github.com/systemd/systemd/blob/v255/src/core/dbus-cgroup.c).

Это source/pure fix, не native enforcement proof и не доказанная причина run38037085181.
Его safe receipt различает только прежний combined manager boundary. Новый stage
entry после owned Compose-up и до workload проверяет exact QA/Postgres container IDs,
images, running/owner/project/service labels, private cgroup namespace, actual
`HostConfig` NanoCpus/Memory и QA PidsLimit. В тех же существующих контейнерах
по inspected ID читает bounded `cpu.max`/`memory.max`/`pids.max`; missing/unlimited/
mismatch закрывает stage. Повторный inspect после чтения исключает смену limits,
image/state/owner в readback. Значения неизменны: QA CPU2/memory5GiB/PID512;
Postgres CPU1/memory1GiB без придуманного нового PID limit. Existing entry deadline300s
ограничивает все readbacks, каждый command имеет10s; budget не расширен.
Public receipt содержит только fixed numeric limits двух services; raw inspect,
container IDs, controller paths и содержимое warnings не экспортируются. Aggregate
требует resource_enforcement для всех12 stages и delegation_removed в cleanup.
Эта реализация проверена pure fixtures, actual HostConfig/cgroup и full12 acceptance
по-прежнему PENDING до запуска exact successor после независимого review/ACK.

Three independent jobs: A python/row-smoke/smoke/check/clippy; B postgres;
C oci/workspace/integration/cli/openapi/release. Each admits a fresh rootless
Docker29.8.2/Compose5.5.1 daemon before effects and uses real PG/OCI fixtures.
There is no runtime, database, image or cache custody transfer between jobs.
Only allowlisted redacted evidence crosses jobs. No receipt is acceptance by
itself: safe receipts are evidence, not transferable native runtime custody.
Final PASS requires all12 in the same workflow attempt, exact sources265,
component/maintenance parity, all assertions and independent exact cleanup.

Original stage budgets remain 2400/300/300/2400/2400/5400/2400/900/900/900/900/1800s.
Smoke retains 300s aggregate/30s call/10s wait, 30 assertions and13 markers.
Bootstrap is5400s from entry; stage overhead has300s entry/10s assertions/500s
independent teardown. Finally has1500+220+60s. Action/upload reserves and command
cleanup tails yield A20232/B14350/C21570s, all below21600s. No budgets increased.
Host reserve108279229428 bytes, data reserve71319483898 bytes and300000 inodes
remain enforced. Reclaim is only measured allowlisted disposable hosted paths.

### Повторный reclaim перед cache allocation

Initial admission и существующий `cache_allocate` вызывают один guarded reclaim:
после image pulls/tools build он может удалить оставшийся неиспользуемый hosted
SDK до original Base constructor. Floors,5400s bootstrap deadline, все12 stages,
product/SDK pins и protected images/caches не изменены. Недостаточный free-after
по-прежнему закрывает allocation; восстановление7.68GB или live PASS не заявлены.

Allowlist содержит только dotnet, Android, legacy `/opt/ghc`, актуальный
`/usr/local/.ghcup` и `/opt/hostedtoolcache`. Legacy path сохранён для прежних image layouts; exact image
`ubuntu24/20261004.327` устанавливает Haskell в `/usr/local/.ghcup`:
[official installer](https://github.com/actions/runner-images/blob/ubuntu24/20261004.327/images/ubuntu/scripts/build/install-haskell.sh).
Отсутствующие кандидаты не заменяются поиском других директорий.

Существующий `reclaim.json` остаётся bounded списком удалений и накапливается
между фазами. Записи проверяются по exact keys, allowlist/unique path, numeric
types, run/attempt и actual host/data devices + root inode. Foreign/malformed,
symlink/nonregular/hardlinked receipt или повторно появившийся удалённый path
закрывают effects; старый receipt без identity не принимается за новый proof.
Каждое удаление сохраняет actual candidate device/inode и free-before/after.
Final safe report повторно читает тот же проверенный cumulative receipt, включая
поздние удаления, без нового storage или экспорта непроверенных данных.
Candidate должен быть canonical directory без symlink ancestors, на том же
device, что host и data root, без mount at/below; identity и mountinfo повторно
проверяются после `du`. Удаление использует только точный literal и прежний
`--one-file-system`; это не atomic reservation или защита от всех filesystem races.

### Source qualification одного toolcache path

Добавлен только literal `/opt/hostedtoolcache`, не `/opt` или другие SDK paths.
Exact image `ubuntu24/20261004.327` создаёт этот каталог как
`AGENT_TOOLSDIRECTORY`/`RUNNER_TOOL_CACHE`; это preinstalled SDK cache disposable
hosted VM, не protected Docker images или наш locked dependency cache:
[environment recipe](https://github.com/actions/runner-images/blob/ubuntu24/20261004.327/images/ubuntu/scripts/build/configure-environment.sh#L34-L39),
[toolset](https://github.com/actions/runner-images/blob/ubuntu24/20261004.327/images/ubuntu/toolsets/toolset-2404.json#L1-L67).
Measured size/free gain и покрытие оставшегося дефицита3754814452 bytes UNKNOWN;
source qualification не означает live capacity PASS. Final capacity gate неизменён.

Complete A/B/C workflow не вызывает setup-python/node/go, CodeQL или toolcache
executables. Host Python3 и Git устанавливаются через apt, отдельно от cache:
[Python recipe](https://github.com/actions/runner-images/blob/ubuntu24/20261004.327/images/ubuntu/scripts/build/install-python.sh#L10-L11),
[Git recipe](https://github.com/actions/runner-images/blob/ubuntu24/20261004.327/images/ubuntu/scripts/build/install-git.sh#L23-L31).
`Configure-Toolset.ps1` создаёт внешние aliases только для Go; Go здесь не вызывается:
[exact configuration](https://github.com/actions/runner-images/blob/ubuntu24/20261004.327/images/ubuntu/scripts/build/Configure-Toolset.ps1#L44-L74).
Три pinned checkout/upload/download actions используют Node24 из runner
`externals`, не cached Node или PATH Node:
[runner handler](https://github.com/actions/runner/blob/v2.337.0/src/Runner.Worker/Handlers/NodeScriptActionHandler.cs#L116-L118).
Checkout post steps используют тот же runner Node и system Git; artifact steps
читают только job/workspace receipts. `/opt/actionarchivecache` не затрагивается.

Host helpers, exact product NativeCompose adapter и три hash-verified maintenance660
helper blobs не потребляют toolcache path/env references. Host Docker/rootless,
Compose и Buildx скачиваются с прежними hashes в owned `root/bin` и config;
apt dependencies/systemd/utilities остаются вне toolcache. Все12 workload stages
используют pinned container tools или system Python observer, не hosted SDKs.
Existing mounted-path/device/symlink/receipt guards применяются без изменений;
локально никакой SDK cache не удаляется и новый hosted run не запускался.

Cleanup retains WNOWAIT leader custody and absolute deadline checks through
actual native bridge close/record boundaries. Parent/nested Compose use v2
journals; disposable target/scratch/DB are distinct from external caches/evidence.
Timeout/unknown cleanup cannot produce PASS; no restart or replay is accepted.

## Validation

Public pure tests (no native/build/containers):

```sh
cd scripts/hermetic_full12
python3 -B -m unittest test_gate test_deadline_boundaries test_bridge_deadlines test_maintenance_git
```

`test_process_groups` is a bounded Linux subprocess-only proof suite.
`private_bridge_proofs.py` отдельно проверяет actual private SDK с fake engine,
temp filesystem и bounded Python subprocesses. Он требует qualified exact packet
в sibling authenticated services-base checkout и не использует installed SDK.
Full12 execution остаётся PENDING и не заменяется этими pure fixtures или
унаследованной исторической acceptance.

No push, dispatch, Docker/Cargo/native execution is authorized by this packet.
