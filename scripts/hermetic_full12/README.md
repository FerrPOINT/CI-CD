# Public-safe exact-source full12 controls

Этот source-only successor имеет непосредственный parent
`4d97c54f35b485224a8229495763658fcbbd40e9`, затем qualification controls
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

Текущий checkout guard требует ровно HEAD ->4d97->632->1db->25be, full history,
task-owned modifications относительно4d97 и add-only controls относительно25be.
Новый workflow/host branch: `build-only/forge-safe-maintenance-full12-20261010`.
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
