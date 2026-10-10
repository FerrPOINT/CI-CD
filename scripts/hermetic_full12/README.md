# Public-safe exact-source full12 controls

Этот source-only successor имеет непосредственный parent
`b9375dc4b4f070b0f6cff1733228f1a6865b1368`, затем safety controls
`4d97c54f35b485224a8229495763658fcbbd40e9`, qualification controls
`632ea8347602fe4af22e7369790ed9c75e53f76a`, original public-safe controls
`1dbedf85242c3540b70005ce5f0c20badb682c41` и product source
`25be2e82d4d42673897c8a16070eb8a9519244f0`. Это normal history, не импорт
retired/private controls. Product SDK остаётся
`19a7a381ae6dbea61a643bb96189e483fa64df5c`; product265, locks и schema неизменны.

## Closed Dependency

`maintenance-pin.json` v2 связывает parent-reviewed safety successor
`43d02057d96b326b4ea077388277e6064602ef61` и explicit published ref
`refs/heads/fix/maintenance-admission-and-installer-20261010` в private
`FerrPOINT/services-base` (Draft PR183). Parent publication ACK и отдельный actual
read-only qualification подтверждают точный tip до/после, fetched ref, три Git
blobs и raw SHA256. `published_exact_commit` относится только к этой dependency,
не к native/full12 acceptance. Private717, installed packet, рабочие файлы,
SDK19a и старые helper hashes не являются fallback.

Новый Base contract сохраняет local-v1 default30GiB. Только reviewed disposable
hosted callers передают explicit `isolated-ci-v1`: parent/cache напрямую, native
sessions через узкий hosted adapter. Native adapter повторно проверяет hosted
admission/task/daemon/context до original constructor; Base defaults не меняются.
Реальный host reserve108279229428 и `SDLC_MIN_FREE_GIB` не снижены до5GiB.
Canonical absolute registry/output и original owner/cleanup semantics сохранены.

### Проверка candidate без записи

Из аутентифицированного full-history checkout с актуальным fetched
`refs/remotes/origin/fix/maintenance-admission-and-installer-20261010` проверить:

```sh
python3 -B scripts/hermetic_full12/maintenance_git.py qualify \
  /absolute/private/services-base 43d02057d96b326b4ea077388277e6064602ef61 \
  refs/heads/fix/maintenance-admission-and-installer-20261010
```

Команда использует существующий Linux owned-process helper, включая WNOWAIT и
group cleanup: общий Git work budget 60 секунд и неизменный teardown tail.
Проверяются canonical origin до сетевого запроса, exact configured commit/ref,
опубликованный branch tip до и после readback, fetched tracking tip, full history,
commit, три regular-file blob и raw SHA256 (без EOL normalization).
Вывод содержит только candidate pin JSON либо фиксированный отказ
`MAINTENANCE_PIN_UNQUALIFIED` (exit1). Нет fetch, установки/import helper, записи
pin, материализации private files, cleanup или допуска full12. Изменение branch
tip во время readback запрещено; ref сохраняется как дополнительное ограничение,
а не вместо immutable commit. Приватный код
и credentials не входят в публичный checkout или artifacts.

Текущий checkout guard требует ровно HEAD ->131->b937->4d97->632->1db->25be,
full history, task-owned modifications относительно131 и add-only controls относительно25be.
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
raw readback проверяется exact fetched private branch tip, без дополнительного
token, registry fallback или сетевого запроса из runtime. Seals/aggregate связывают
repository/commit/ref/три hashes. Только metadata входит в public evidence.
Private bytes остаются в памяти/ephemeral VM, не в public Git/logs/artifacts.

Оригинальные native QA и observer source bytes не переписаны. Перед original
`load_sdk`/smoke binding адаптируется только expected `SDK_SHA256` из квалифицированного
Git proof; original loader, journal guards и assertions остаются строгими.
Private bridge proof использует ту же привязку. Его pure private-SDK cases не
запускают Docker и не заменяют full12 execution.

## Execution Contract

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

## Resource Admission Successor

Normal child `1310958` использует отдельный hosted-only branch
`build-only/forge-resource-full12-20261010`. Product25be, SDK19a, maintenance43,
265 source inputs, 12 stages, manifests, budgets и cleanup неизменны.
`Warnings` больше не является blanket admission predicate: это информационный
нестабильный текст Docker, он не парсится и не экспортируется.

Обязательны cgroup version `2`, driver `systemd` и exact boolean `True` для
`MemoryLimit`, `CpuCfsQuota`, `CpuCfsPeriod`, `PidsLimit`. Missing/null/false/числа/
строки не принимаются. Отказы имеют отдельные fixed hints
`identity_cgroup_version`, `identity_cgroup_driver`, `identity_cgroup_resources`.
Старый `identity_cgroup_warnings` остаётся допустимым только для readback старых
receipts; новый код его не выдаёт. Stage entry/exit сохраняет повторный admission.

До rootless launch existing hosted bootstrap выполняет для exact current UID:
`sudo -n systemctl set-property --runtime user@<uid>.service 'Delegate=cpu memory pids'`.
Затем требует наличие всех трёх controllers в exact user-manager
`cgroup.controllers`. Нет restart/reboot/fallback или persistent `/etc` изменения;
неподдерживаемая команда/отсутствующий CPU закрывает bootstrap в `manager`.
Это только disposable GitHub-hosted VM после existing host/source qualification,
не инструкция менять локальный daemon. Systemd поддерживает runtime controller
delegation; Docker rootless требует v2/systemd и отдельного CPU delegation:
[Docker](https://docs.docker.com/engine/security/rootless/tips/#limiting-resources),
[systemctl255](https://github.com/systemd/systemd/blob/v255/man/systemctl.xml),
[Delegate setter255](https://github.com/systemd/systemd/blob/v255/src/core/dbus-cgroup.c).

Это source/pure fix, не native enforcement proof и не доказанная причина run38035575536.
Его safe receipt различает только прежний combined boundary. Existing stage entry
проверяет daemon ID, но не actual `HostConfig`/cgroup limits. Bounded follow-up перед
workload: после owned Compose-up проверить exact QA/Postgres container IDs и
`HostConfig` против unchanged manifest; в тех же контейнерах прочитать bounded
`cpu.max`/`memory.max` и QA `pids.max`, отклонить missing/unlimited/mismatch и
сохранить лишь fixed checks/numeric limits, не raw inspect. Это отдельный reviewable
patch; в данном successor он не реализован. Full12/native acceptance остаётся pending.

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
