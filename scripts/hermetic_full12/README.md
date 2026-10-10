# Public-safe exact-source full12 controls

This build-only successor adds public controls to product
`25be2e82d4d42673897c8a16070eb8a9519244f0` with that commit as its sole parent.
It does not merge the retired controls chain or vendor private maintenance source.
Product SDK stays `19a7a381ae6dbea61a643bb96189e483fa64df5c`.

## Closed Dependency

`maintenance-pin.json` deliberately has `commit: null`. No published immutable
private commit was qualified for the three exact required Git blobs. Metadata
hashes are requirements, not proof of publication. The controls therefore stop
before private checkout in the workflow, and before any materialization/resource
effects in direct job invocation. No fallback to product SDK, task working files,
encoded payloads, moving refs or local installed SDK is allowed.

The dependency owner must publish the exact three files in the private
`FerrPOINT/services-base` repository. A reviewed successor can then replace the
null pin with the verified commit and `published_exact_commit`, regenerate the
component lock and repeat validation. Raw Git bytes, including line endings,
must match both blob IDs and SHA256. Current private main's older helper and
missing two companion paths do not qualify.

### Проверка candidate без записи

После публикации и review точного maintenance packet владельцем Base проверить
commit опубликованной ветки из аутентифицированного full-history checkout на Linux:

```sh
python3 -B scripts/hermetic_full12/maintenance_git.py qualify \
  /absolute/private/services-base <exact-40-lowercase-commit> refs/heads/<reviewed-owner-branch>
```

Команда использует существующий Linux owned-process helper, включая WNOWAIT и
group cleanup: общий Git work budget 60 секунд и неизменный teardown tail.
Проверяются canonical origin до сетевого запроса, опубликованный branch tip до и
после raw-Git readback, full history, commit, три regular-file blob и raw SHA256.
Вывод содержит только candidate pin JSON либо фиксированный отказ
`MAINTENANCE_PIN_UNQUALIFIED` (exit1). Нет fetch, установки/import helper, записи
pin, материализации private files, cleanup или допуска full12. Изменение branch
tip во время readback запрещено; moving refs не сохраняются в pin. Приватный код
и credentials не входят в публичный checkout или artifacts.

Отдельный reviewed follow-up должен независимо подтвердить metadata, заменить
maintenance pin точным commit и обновить component lock. Исходный workflow всё
ещё требует original build-only branch и exact single controls parent; этот
qualification-only successor не меняет execution guards. Перед публикацией
исполняемый successor требует отдельного review normal parent/branch binding.
Candidate JSON и совпадение локальных installed bytes не доказывают native/SDLC
acceptance.

Три hash связывают v2 helper с cleanup/installer. Политика Base сохраняется: exact
owned Compose down в finally, v2 daemon/manifest/owner проверки до удаления
disposable volumes, сохранение external caches/runtime/backups/evidence,
installer audit до регистрации scheduler. Global Docker-group audit остаётся
workspace контрактом Base (`scripts/audit_docker_groups.py`), а не разрешением
cleanup или копирования дополнительных private files в packet. Disk/RAM guards
обязательны; native execution здесь не разрешён.

The existing authenticated SDK checkout remains two minutes, now fetch-depth0.
The same private object database delivers product SDK19a and the distinct
maintenance commit. Qualification checks repository, full history, commit,
allowlisted tree modes/paths/blob IDs and raw hashes. Only metadata enters public
seals/provenance. Private bytes stay in memory/private ephemeral VM files and
never in the public Git tree, output, logs or artifacts.

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
`private_bridge_proofs.py` separately exercises the actual private SDK boundary:
it fails closed until the qualified pin is available in the sibling authenticated
services-base checkout. Its results and the full12 execution are PENDING, not
replaced by synthetic pure fixture tests or inherited historical acceptance.

No push, dispatch, Docker/Cargo/native execution is authorized by this packet.
