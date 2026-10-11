# Forge task workspace, candidate и owner-local delivery evidence

**Статус 2026-10-08:** bounded source implementation и local component verification;
полного SDLC acceptance нет.

## Latest Frozen Native Packet: 8 Октября

Epoch `20261008T172750Z-0f560932eb2b` завершился FAILED/unknown в OCI,
несмотря на PostgreSQL3/3 PASS. Python37/37, actual lossless row/SQL safety smoke,
locked all-target check и strict Clippy прошли. PostgreSQL suite:3 PASS,0 FAIL,
0 ignored,2920.07s; проверены24 negative cases, H/C/M/B и все пять SIGKILL
checkpoints, включая release-intent/writes-released. Это новый отдельный packet,
не объяснение причины исторического отказа8f596.

OCI suite:0 PASS,1 FAIL,0 ignored,27.21s. Отказ в исходном runner completion
wait `tests/support/task_delivery.rs:287`, после начала первого настоящего
Docker build. Это не terminal deployment receipt и не timeout ожидания artifact
API. Полная image/data/rollback и SIGKILL OCI приёмка не достигнуты;
normal/workspace/CLI/OpenAPI/release follow-ups не запускались. Исходные25s
completion и runtime command deadlines не увеличены.

На terminal повторно совпали все259 frozen/current inputs; cleanup complete,
собственные containers/networks/volumes трёх exact Compose projects отсутствуют,
pre-existing volumes/cache сохранены. Independent host readback подтвердил
отсутствие PID471224 и пустой task-labelled Engine inventory.

| 0f560 evidence | SHA256 |
| --- | --- |
| `postgres.log` | `1b7ef59858472972e08098cae75ebc4b99a8d6014af6e608aa440c4472f36276` |
| `oci.log` | `1bf76925e570a40a16071853d9668a9fdfdf134b687b0bafe8047373ce5c1d0e` |
| `native-packet.json` | `4db819760af1f449591d8baadd13b358b8c079925384560eed41ac1ff9c68e36` |
| `native-packet-cleanup.json` | `26d351b669248ac91b0c6b66a3d0b2d3262e09c54e0170b7168f52d52995319f` |

Catalogue SHA256: Forge
`1b7d83ccd6e553622d4048e058f735744d423e116c01c9ea48eaecb7fcd9d104`,
Base `d823584da8d75dda10142222dd97ff9595982fd80b65f8eacaa7e5927f3a2fc6`.
PR88/head56f1217 CI не подтверждает subsequent dirty source; Draft сохраняется.
Эта документация может быть опубликована раньше hardening-кода: source changes
не включены в documentation-only commit. Published runtime code остаётся56f1217;
новый documentation head/CI не является native acceptance локальных изменений.

Дополнительный read-only `journalctl --user -u docker.service` за
`2026-10-08 18:36:00..18:38:15 UTC` показывает DNS failure при запросе
anonymous auth Docker Hub для pinned Python reference и BuildKit session
healthcheck `only one connection allowed`, затем cancelled solve. Pinned image
присутствует в local Engine. Это наблюдения в том же временном окне, не доказанная
единственная причина completion timeout. Следующий шаг: доказать offline
preloaded-image path и fresh native OCI acceptance, без ослабления таймеров,
assertions, изоляции или замены настоящего build fixture-артефактом.

### Offline OCI Fixture Diagnostic: 4a94b411e888

Source candidate в `oci_delivery.rs` экспортирует только уже закреплённую базу
в private local OCI layout. Guard проверяет root/platform/blob hashes, daemon,
image и сохранение original references; Docker aliases не создаются и не
удаляются. Настоящие Git/pipeline/build/artifact/deployment/rollback assertions
и исходные25/50s таймеры сохранены.11 новых pure regressions и parent aggregate
51 Python/doc cases PASS. Это source evidence, не offline native PASS.

Свежий отдельный diagnostic epoch `20261008T190853Z-4a94b411e888` замораживает
260 inputs: изменён только OCI fixture, добавлен его unit file. Locked all-target
check и strict Clippy с integration/postgres-integration/oci-integration PASS.
OCI test0 PASS/1 FAIL/0 ignored,30.80s: `OCI_OFFLINE_BASE_GUARD_FAILED` при
preparation, до первого actual build. Причина внутри preparation не локализована;
elapsed time не доказывает timeout image export. Не увеличивать таймеры и не
объявлять проблему registry устранённой на основании этого failed probe.

Packet имеет явный scope `oci_fixture_diagnostic_only`, обе SDLC/full-native
acceptance flags false. PostgreSQL здесь не выполнялся:0f560 PG3/3 остаётся
историческим exact-source evidence, не результатом нового packet. Full follow-up
gates также не запускались. Terminal failed/unknown, current/frozen260 parity
true, exact-project cleanup complete, caches/pre-existing volumes сохранены.
Parent подтвердил отсутствие PID1463173 и task-labelled containers.

| 4a94 diagnostic evidence | SHA256 |
| --- | --- |
| Forge source catalogue | `d33648490f37a32809887e38cf1401fdfe0e3230d5eff1ee667804449f350b01` |
| `oci.log` | `877ae025545c19b1f9bfd92ca32ee7c12143fea1038131f08549e71b577c7e2a` |
| `probe.json` | `d8c560799e59092a5e59c59b622de4d1375b990c698c58a110246280496a55d2` |

Published documentation-only head `d51c260` имеет четыре CI SUCCESS в
run37828888927; локальный OCI guard и остальные pending hardening source edits
не входят в этот head. Следующий шаг: bounded redacted preparation diagnostic
на новом disposable packet, затем actual OCI/full gates. Ни publication, ни
этот probe не меняют accepted runtime, SDK pin, migrations или PR87.

### Offline OCI Runner Diagnostic: e6e304554e4d

Fresh260-input epoch `20261008T191848Z-e6e304554e4d` сохраняет исходные
25/50s deadlines. Preparation с closed stage/reason error code завершилась;
actual runner accepted offer получен. Locked all-target check и strict Clippy
PASS, OCI0 PASS/1 FAIL/0 ignored,105.64s total: исходный runner completion
timeout `tests/support/task_delivery.rs:287`. Это не successful image/artifact
или deployment/rollback receipt. Причина внутри pipeline ещё не доказана;
успешная preparation не объясняет предшествующий4a94 failure.

Terminal failed/unknown; own runner PID1472251 отсутствует при parent readback.
Все260 current/frozen inputs совпали; exact-project containers/networks/volumes
пусты, cache и pre-existing volumes сохранены. PostgreSQL/full follow-ups здесь
не выполнялись;0f560 PG3/3 остаётся отдельной исторической приёмкой.
Обе `full_native_acceptance`/`sdlc_acceptance` flags false. Closed error-code
regression добавляет12-й pure OCI case; parent aggregate52 PASS до этого probe.

| e6e diagnostic evidence | SHA256 |
| --- | --- |
| `oci.log` | `35fbbfb72308466ad7892a36ffe942fa6e4a29705527cb871bf920e44f7de69c` |
| `probe.json` | `580134441955fd7d9837b5a8a34d22b6a44237a7afecd3f136de79a50d78e065` |
| `cleanup.json` | `a71cce7931296a710408df474be84d2531fc25cda1f2b7a309b8a0b81442d223` |

Next: bounded closed-stage build progress and runner completion diagnostics,
then a new actual OCI and full native gate. No timer relaxation, old-command
replay, installed-runtime update or claim of readiness is authorized by this result.

### Offline OCI Actual Acceptance: a7c3dc379a3c

Fresh epoch `20261008T193535Z-a7c3dc379a3c` passes locked all-target check,
strict Clippy and the actual OCI test:1 PASS/0 FAIL/0 ignored,237.88s total.
Original25/50/35s deadlines and acceptance assertions remain unchanged.
The local OCI base guard validates pinned manifest/platform/blob content and
preserves original image references; the real runner builds repository code and
uploads `product.txt`. Verified cases include exact image/commit, readonly data
readback, health503/acceptance422 rejection, exact-image rollback, incompatible
schema/migration/data-drift rejection, actual SIGKILL recovery without recreate,
and immutable verified-checks proof preservation after a second SIGKILL.

This is current-source owner-local OCI evidence, not full-native or SDLC
acceptance. PostgreSQL/normal/workspace/CLI/OpenAPI/release follow-ups were not
run in this diagnostic. Historical0f560 PG3/3 is separate; the new success does
not establish a unique cause for failed4a94/e6e epochs or cold-cache reliability.
The eight new progress/permissions/redaction regressions bring pure OCI coverage
to20 PASS on Windows/Linux. `completed` diagnostic phase does not imply pipeline
or business acceptance.

All260 current/frozen inputs match; Forge catalogue
`85f3732d615801f3dced6580faa72ec9b9d82ce35276d788fe58d3dafe6abe32`,
Base catalogue remains `d823584da8d75dda10142222dd97ff9595982fd80b65f8eacaa7e5927f3a2fc6`.
Terminal `passed_oci_diagnostic_only`, both acceptance flags false; exact-project
containers/networks/volumes empty, cache/pre-existing volumes preserved. Parent
readback confirms missing PID1482732 and empty owned-task container inventory.

| a7c3 actual OCI evidence | SHA256 |
| --- | --- |
| `oci.log` | `0c1dff4e9441990aed9d5a9cde67565e58b5703ba20a2c2a794b551232d24978` |
| `probe.json` | `79d1a31509e80f7f6818affb8eccf31498034059709e8fd088afe765fccf83fa` |
| `cleanup.json` | `98ebda5ef64e2e5825050002bcf557384eccd0681d3c81ea8ec8730e48cf122c` |

Next run the complete current-source native/follow-up gates before source
publication and readiness. PR88 stays Draft; no migration, admission authority,
SDK pin or installed runtime changes follow from this scoped pass.

### Previous Failed Packet: 8f596

Epoch `20261008T150543Z-8f596c025d25` завершился FAILED/unknown, не PASS:
Python/row-smoke/SQL-smoke/locked all-target check/strict Clippy прошли;
PostgreSQL suite:2 PASS,1 FAIL,0 ignored,2810.81s. OCI не запускался после
ошибки PG. На завершении frozen/current259 source inputs совпадали; последующие
diagnostic changes требуют отдельного нового epoch.

Прошли actual migration/restore/access-boundary и unknown-schema/stale-lease
сценарии. В SIGKILL suite checkpoints backup-verified/migration-intent/
restore-intent пройдены, но child завершился exit2 до release-intent.
Сохранённый diagnostic локализует отказ в backup rehearsal DB creation:
`execute -> backup -> create_database -> sql -> run`, reason
`owner_command_failed_or_unknown`. До этого child не был убит тестом;
release-intent/writes-released этим прогоном не приняты. Причина ненулевого
owner subprocess exit пока неизвестна: raw stderr/SQL/credentials не сохранены.
Это не доказательство прежнего PID1 orphan или timeout; assertions/deadlines
не расширяются, unknown side effect не повторяется.

Exact Compose finally-cleanup завершён: собственные containers/networks/volumes
в трёх QA projects отсутствуют, pre-existing volumes и cache сохранены.
Independent process readback подтвердил отсутствие PID3789829 после terminal;
parent Engine readback подтвердил пустой outer container/network inventory.

| 8f596 evidence | SHA256 |
| --- | --- |
| `postgres.log` | `d97f3d4cb1c966c8fdf3e716f36fa6f68f62a4001a0f5a884c839450a439f8d6` |
| `native-packet.json` | `aa63821aa2579ac13f04ce9f8d2acc15530520a99ee6d5f8cad81dbfe82e7bbe` |
| `native-packet-cleanup.json` | `b68789f01a4b59b40f68dd5f2e40781edb54078a3c94e5ee0d269a55bbb3b879` |

Catalogue SHA256: Forge
`65916d3eae16f68cefeb1ee8b1f9dfbe3546834ae48fe9c1c54c1ad891b6b324`,
Base `d823584da8d75dda10142222dd97ff9595982fd80b65f8eacaa7e5927f3a2fc6`.
Published PR88 head56f1217 and earlier native packets do not certify this
hardening source; PR remains Draft, main release and full SDLC remain blocked.

После этого failed packet добавлена failure-only диагностика owner subprocess:
returncode/elapsed и строго допустимый code-only SQLSTATE; raw stderr, SQL,
argv/env/URLs не выводятся.37 pure tests PASS на Windows (0.402s) и WSL/Linux
(0.051s), включая11 redaction/no-retry/unknown-receipt regressions. Новый
diagnostic source не меняет deadlines или recovery semantics и не объясняет
исторический отказ. Новый0f560 packet выше подтверждает PostgreSQL этого source,
но не OCI/full follow-up acceptance; старый frozen packet не заменяется.

## Hardening reader fence и OCI recovery, 2026-10-08

После опубликованного56f1217 выявлены reader DML через views/column grants,
sequence mutation, SQL wildcard в `pg_%`, отсутствующий large-object inventory
и OCI crash-window между successful checks и final receipt. Предыдущие3/3 PG,
1/1 OCI и4 CI SUCCESS не доказывают устранение этих дефектов.

Current source проверяет grants table/view/column/sequence/MAINTAIN, schema и
database CREATE/TEMP; literal prefix не скрывает `pgx`. Unsupported non-public,
materialized/foreign и large-object inventory удерживает snapshot. Drain закрывает
LOGIN/source CONNECT обоих application-role, завершает обе группы sessions и
проверяет source quiescence до/после dump; reader снова входит только в candidate.
Large-object mutation EXECUTE закрыт; writer column grants на SQLx history
проверяются. Drain отзывает CONNECT обеих ролей ко всем DB, а release/reconcile
проверяют единственный выбранный target, включая failed candidate после rollback.
OCI reconciliation удерживает исходный intent-bound historical proof и требует
новую observation; receipt timestamps не перезаписывают immutable файл.

Первый reader-fence packet прошёл10 pure Python regressions на Windows и WSL/Linux,
aggregate docs/tests16/16. Последующее независимое review выявило writer history
mutation через views, непрочитанный historical PG proof и rejection допустимого
released writer pool. Current source выдаёт DML только ordinary/partitioned tables,
не views/history; effective view/column DML и custom rewrite rules блокируются.
Readback проверяет historical proof против intent/manifest/generation/image/history/
точных probe hashes; reconciliation не переписывает этот файл и разрешает writer
sessions только подтверждённой released DB после fresh connection-scope проверки.
Original proof fingerprint точно равен immutable manifest; новые post-release rows
допустимы только в fresh reconciliation, не при подмене historical hashes.
Финальное review выявило ещё путь через пустой `INHERITS` от SQLx history:
исключение одного имени не защищает descendant от writer grants и parent SELECT.
Counter-review подтвердило и обратный путь: writer DML на ancestor меняет history.
Current source запрещает inheritance edges в обоих направлениях от history до
grants/evidence; отсутствие incident edges исключает и транзитивные пути.
Новые native negatives создают пустые child/grandchild и history-as-child.
Отдельный FK path обходил пустой non-internal trigger inventory: CASCADE/SET NULL/
SET DEFAULT на history выполняются внутренними RI triggers с owner правами.
Current source проверяет history `conrelid` и обе referential actions до Base
evidence/grants; native cases отдельно покрывают DELETE/UPDATE CASCADE. Финальное
независимое counter-review закрывает эти findings, без заявления live PASS.
Current Windows pure regressions22/22, aggregate docs/tests28/28 PASS. Новые native
tests удерживают writer connection при recovery, подменяют proof и проверяют отказ
при history-view/column/rule grants. Это source/control-flow evidence; acceptance
требует нового immutable source epoch и реального PG rerun.
Review-fixes epoch `20261008T090335Z-4e95ebcac21f` прошёл19 Linux pure tests и
locked/offline workspace/all-targets check/strict Clippy с integration,
postgres-integration, oci-integration; frozen/current hashes совпали на этих этапах.
Native PG gate этого epoch завершился FAILED:2 passed/1 failed,0 ignored,
2246.14s. Все19 negative permission/catalog cases и пять SIGKILL checkpoints,
включая foreign-proof/current-writer-pool recovery, прошли; это не общий PASS.
Провал — реальный M-case `Unknown` до DB fence: сохранённая diagnostic содержит
`TimeoutExpired` из `drain`/`run` при Compose stop. Source задаёт explicit5s stop
grace внутри прежнего20s command deadline; NOLOGIN/CONNECT/session quiescence
остаются обязательными. Unknown operation не повторяется. Эта последующая правка
двух Python inputs и последующий inheritance fix в трёх inputs требуют нового
frozen epoch; предыдущий gate не current PASS. Текущая native negative matrix
содержит24 cases, но FAILED epoch выше проверял19 и не доказывает новые пять.
Свежий epoch `20261008T100801Z-d92019b98cb3` фиксирует все три изменённых inputs.
На current frozen source прошли22 Linux pure cases, locked/offline all-target
check/strict Clippy с integration/postgres-integration/oci-integration и настоящий
PG17.11 SQL smoke. Он доказывает empty child/grandchild, history-as-child,
DELETE/UPDATE CASCADE/SET NULL/SET DEFAULT, passive-action contrast, restore ACL
seal и обе role drain/candidate-only connections. Это отдельные SQL component
cases, не полный delivery suite. Smoke log SHA256:
`7a957998bbbe80748dde927080eab600c3b0647b7a0d46523b55fade21a5c557`.
Forge source catalogue:
`e5bb756330f0eb78b61d5e1ed6e4130f4963897dfa854f0f5741304171ccb43a`;
Base catalogue остаётся `d823584da8d75dda10142222dd97ff9595982fd80b65f8eacaa7e5927f3a2fc6`.
Parent независимо сверил259 current input hashes/sizes:0 mismatches. Native gate
этого epoch завершился PG3/3 PASS2795.37s и OCI1/1 PASS90.73s;24 negative cases,
H/C/M/B, все пять SIGKILL checkpoints, foreign-proof rejection и released writer
pool recovery подтверждены. Workspace follow-up FAILED137/1 на
`same_size_restored_mtime_and_spoofed_stat_cache_cannot_hide_physical_bytes`:
ожидаемая physical-mismatch diagnostic не совпала, фактическая причина ещё
диагностируется. Failure не выдаётся за flaky/PASS; normal/CLI/export/release
не приняты. Frozen/current source parity true на завершённых этапах.

| d920 evidence | SHA256 |
| --- | --- |
| `postgres.log` | `e16643f8a1853e85dc1a4867c66af6cbf6639cf0ccc4909e6fecb3ccaf1557f9` |
| `oci.log` | `1d7eee0955f132c511090f01c3869e11b1bfcab91e855b52737f2eeb6cfa39f4` |
| `workspace.log` | `19101ceb8cea76c5bfff7dacbb52d547b34484a0a037ff0cbe5339d623e86c1f` |
| `workspace-cleanup.json` | `2f3177c1f5361f945a0c6cf047c5ac0a5cebe09361b7c6405c641048531933f8` |

После native gate parent counter-review обнаружил дополнительный дефект:
`Owner.fingerprint` давал одинаковый data SHA двух разных numeric значений
`1.0000000000000000000000000001` и `1.0000000000000000000000000002`.
Dataset разрешал существующие numeric/raw-json, несмотря на ограничение типов
новой column migration. Pure exact-production reproduction сначала FAILED
с digest `dcbed99148040793f7880ebdfd233b4d1f3902f03c1045b41846b1139f775408`.
Current fix сериализует record text в array строк с фиксированными PostgreSQL
settings и domain hash; проверяет тип/512-row bound, NULL/empty и duplicates.
Только два Python inputs изменены после законченного epoch; Rust не менялся.
RED repro теперь GREEN,26 pure/aggregate32 PASS. Новый real SQL smoke и full
native/normal/release нужны отдельно; d920 proof не переназначается этому source.
Historical receipts/manifest hashes не переписываются и не implicit-upgrade.
Исторический failed epoch очищен exact Compose: собственных containers/networks/
volumes нет,20 pre-existing volumes сохранены. Fresh Windows global Docker audit
видит Desktop35 и sdlc2-runner0 без violations, но sdlc1-runner registry endpoint
недоступен: complete=false/exit1. Отдельный native rootless owned audit complete;
это не полный глобальный PASS и не основание менять защищённые endpoints.
Свежий WSL/rootless Rust1.88 gate
`sdlc-qa-forge-hardening-20261008t073407z-93ec451b8e2b` PASS: fmt, locked fetch,
offline workspace/all-targets/all-features check, strict Clippy и219 workspace tests
(0 failed/ignored), включая4 OCI recovery unit tests. Native integration targets
скомпилированы check/Clippy, но PG17/OCI runtime suites и release build этим gate
не выполнялись. Все259 scoped source inputs и frozen copies неизменны; parent
повторно сверил259 hashes с файлами на тот момент,0 mismatches. Exact Compose cleanup
complete, собственных containers/networks/volumes нет; shared resources сохранены.

Source catalogues: Forge
`82de9b19b9a79987c0f1c041a437bc1f270b6b2865c96c2a0ff8251300427709`, Base
`d823584da8d75dda10142222dd97ff9595982fd80b65f8eacaa7e5927f3a2fc6`.
Retained report `.local/rootless-hardening/runs/20261008T073407Z-93ec451b8e2b/report.md`
SHA256 `51b6c83afab47897a9a5d10f060fb211332e6bbe7160b150ce58bc5b1ee10354`;
summary SHA256 `a07972ec3394beb0a341e76fb7e9a2d68712cc0508a4c709f4921b92c3b4601e`.
Предыдущие preflight/прерванные rootless попытки сохранены как failed/nonacceptance;
они не заменены PASS. Desktop Engine не использовался, существующие services/volumes
не менялись. PG17/OCI native gate и hosted exact-head CI остаются отдельной приёмкой.
Последующая настоящая PG17 SQL smoke выявила evaluation-order ошибку:
`has_sequence_privilege` вызывался на toast relation до фильтра `relkind`.
Reader privilege queries исправлены явным CASE, обновлён Python regression.
Rust source не изменился, но2 Python inputs после Rust gate изменились;
его catalogue нельзя выдавать за полный current source. Failed SQL smoke сохранён;
Новый source epoch `20261008T080321Z-71e23dcb02ec` прошёл настоящий PG17 SQL
smoke: проверены large-object ACL, сохранение seal после dump/restore, drain обеих
ролей и candidate-only reader access. Это SQL component evidence, не full delivery
suite. Первый full native PG rerun0/3 отклонён до запуска Python: QA image содержал
symlink `python3`, который production trusted-path guard запрещает. Не меняя guard,
QA tools пересобраны с обычным executable; failed log и snapshot сохранены.
Текущий full native rerun и release gate ещё не приняты.
Свежий frontend gate Node22.20.0/pnpm10.28.1: OpenAPI check/compat(main), lint,
201 tests, typecheck/build и audit0 advisories PASS. Existing Vite chunk warning
сохраняется. Frontend и API schema bytes не менялись; новый browser/screenshots
gate для этих backend-only fixes не выполнялся. Docs verifier, SBOM drift и
scoped secret scans backend/scripts/docs/.github0 findings PASS.
PR88 остаётся Draft, production/SDLC admission закрыт. Новый SQL migration,
Base pin, dependency PR87 и frontend не изменяются.

Fresh release preflight 8 октября подтверждает OPEN/non-Draft/main PR87
`fc3e107` с четырьмя CI SUCCESS и OPEN/Draft/main PR88 `56f1217`.
После fresh fetch текущий migration diff PR88 содержит0039 и0040: prerequisite
ещё не принят в main. Перед ready повторно проверить final diff после merge87
и обычного history reconciliation: только одна новая migration0040, неизменные
исторические bytes и новый exact-head gate. PR87 и accepted runtime не менялись.
QA инструкции AGENTS теперь исключают direct Docker/bare permanent Compose и
требуют точный временный проект, owner/purpose и finally cleanup. Повторно прошли
28 docs/Python tests, docs verifier и SBOM drift; scoped secret scans проверили
backend130/scripts24/docs172/.github6 text files,0 findings. Parent повторно сверил
259 current native inputs по SHA256/size,0 mismatches; doc-only правки не входят
в этот executable catalogue. Это не завершение ещё выполняющегося native gate.

## Реализованный срез

Immutable blocked workspace operation связывает original key/hash, exact Tracker
namespace/project/task/root/assignment/execution/routing/revision/fence и Forge
repository/source/attempt/lease/generation. Machine/project ACL, append-only
ledger, replay/conflict и fresh local lease readback не дают business admission.
Migration40 принадлежит этому срезу; migration39 и runner recovery — зависимости
[PR87](https://github.com/FerrPOINT/CI-CD/pull/87), сохранены без изменений.

Existing source из `ef7a376` переносится выборочно поверх PR87 `fc3e107`.
При переносе сохранены все новые runner binary/restart regression tests PR87.
Base pin `875cac2edf1a18c3a8a59e2f67256d02a8fc04e4` не изменяется.
Исходный broad checkout, его branch и runtime не редактируются.

Owner-local preparation создаёт isolated pinned checkout, сверяет физические
bytes/modes/tree/index/config и сохраняет immutable intent/active/final journals.
Unknown Git outcome держит ресурс; replay/readback не повторяют clone/checkout.
Candidate GET выводит pipeline из original lease, проверяет repository config,
full SHA, config/plan digests, все latest runner completions и retained artifact
bytes. Исторические artifacts другого attempt не подменяют current artifact.
Это текущее readback с immutable operation, не trusted delivery receipt.

## Actual producer test

PostgreSQL + ephemeral Axum + настоящий `forge-runner --once` + Git bare source.
Конфигурация `.forge-ci.yml` прочитана настоящим pipeline trigger по exact commit;
настоящая shell-команда создаёт artifact, runner загружает его через owner HTTP,
завершает lease через completion endpoint. Curl получает readback с проверенным
SHA256, exact pipeline/attempt и original receipt. Ни pipeline/ACK/artifact metadata,
ни artifact file для положительного результата тест не подставляет fixtures.
Task binding — declared test input. В исходном candidate-срезе07
admission/scheduler/delivery не включены; local executor08 описан ниже.

Негативные cases: до completion, bytes tamper, missing accepted ACK, changed
original hash, superseded local generation, missing plan/SHA/artifacts, machine
credential rejection, missing/null revision identity в PostgreSQL. History deletion
возвращает409, не удаляя project/repository/workspace или operation receipt.
Отдельные существующие tests проверяют unknown completion restart и owner GET.

## Gates и происхождение evidence

Полный source gate `sdlc-qa-forge-delivery-14ec9b1a4c29` PASS на Rust1.88:
locked/offline fmt, workspace/all-targets check и strict integration clippy,
215 workspace tests +78 PostgreSQL tests +2 real-API CLI tests (0 failed/ignored),
CLI integration clippy, OpenAPI exporter equality и release workspace build.
Активные dependency graphs `rsa`/`sqlx-mysql` пусты. После finally Compose down
собственных containers/networks нет; SHA256 всех212 CI-CD/pinned Base inputs
совпадают до/после gate. Это source evidence; hosted checks проверяются отдельно
на опубликованном exact head и не заимствуются из PR87.

Локальные evidence files сохраняются в untracked `.local/qa/` (не runtime backups):

| File | SHA256 |
| --- | --- |
| `sdlc-qa-forge-delivery-14ec9b1a4c29.log` | `84a0fde7ea6ae101d7490499fab6d9d0b9248af865d53bf35cac13491b61bb1a` |
| `sdlc-qa-forge-delivery-14ec9b1a4c29-sources.json` | `04517a631c24a71bef6c622ee2860428df17f86a5c92f7da660888b7dda64b1f` |
| `sdlc-qa-forge-delivery-14ec9b1a4c29-cleanup.log` | `64d9b3fc8c4d3d41b99ecf5bb8ca7f88c4d61461640eca91c1e00501df1a9121` |

Frontend Node22.20.0/pnpm10.28.1: frozen/offline install, generated contract equality,
OpenAPI compatibility с main, typecheck/lint,201 tests, build и audit PASS
(0 advisories). После окончательного OpenAPI export повторены codegen/check,
compat с main и typecheck: PASS. `.local/frontend.log` SHA256:
`7becb72a33d7a46b1b16fec6f6db04337e7854923472df5eb393a10da89eab09`.
Source UI не меняется; browser screenshots/live UI acceptance не заявляются.

Docs verifier/SBOM drift и6 regression cases PASS; tracked-export secret scan:
464 text files,0 findings (heuristic, не DLP certification). Docker-group audit:
complete=true, Desktop37/оба rootless runners0, violations=[], exit0.
Повторный audit после окончательного cleanup: Desktop35, оба rootless runners0,
complete=true, violations=[], exit0. Tracked worktree после commit чист;
untracked `.local/` не включается в PR.
Каждый собственный QA project использует labels task/purpose, isolated network,
tmpfs PostgreSQL, read-only source mounts и finally Compose down; caches сохраняются.

Первый полный source gate `3cda36301090` не прошёл:75/77 PostgreSQL cases PASS,
два checksum-negative catalog fixtures исключали новую40 и получали MissingVersion
раньше ожидаемого checksum mismatch36/38. Исправляется только test catalog ceiling;
SQL/checksums migrations1–39 и assertions unchanged. Исправление подтверждено
повторным полным gate выше. Scoped run `47cbb64ed434` также выявил503 в новом
deletion test: использовался unauthenticated harness. Переведён только тест на
existing authenticated admin helper; authorization fallback не добавлялся.
Scoped `95c67e095640`:12 workspace/candidate cases и6 migration regressions PASS.
Провалы не выдаются за PASS.

## Owner-local delivery source, 2026-10-08

`forge-delivery` публикует один реально построенный retained static artifact в
изолированный target. Manifest содержит полный original receipt и actual
pipeline/artifact/attempt/config/plan/policy digests. CLI использует existing
project machine credential, sealed owner candidate observer и explicit
`local-verification` mode. Null expected-current означает только пустой target;
rollback берёт единственный сохранённый last-confirmed manifest. Journal и Unix
lock сохраняют unknown outcome; original-key replay/readback не публикуют снова.
Reconciliation читает уже served manifest и HTTP bytes/checks, сохраняет отдельный
terminal receipt и исходный unknown receipt.

QA приложение обслуживает manifest и реальные artifact bytes через отдельный
Compose service. Health и acceptance зависят от actual payload. A подтверждается;
B health503 и C acceptance422 не меняют last-confirmed A; explicit rollback
возвращает manifest/served bytes A. D публикуется, затем CLI получает SIGKILL во
время probe; новая операция блокируется, reconciliation подтверждает D без
изменения current-pointer mtime. Недоступный version endpoint даёт unknown, а
повторное наблюдение после восстановления — отдельный verified receipt.
Каждая A/B/C/D версия создаётся real pipeline trigger + runner + artifact upload
+ completion; положительные pipeline/ACK/artifact records не seeded fixtures.

Негативные cases: unavailable artifact, fabricated empty-target CAS, changed
original fence/artifact, no local mode, read-only/revoked machine credential и
forged durable dispatch flag. Curl проверяет пять workspace/candidate/delivery
routes; SDLC POST остаётся503 даже при configured local target. Readback —
историческая technical evidence, не fresh continuous service-health guarantee.

Scoped final-source gate `sdlc-qa-forge-delivery-aff4e88982fb` PASS: Rust1.88
locked/offline fmt, workspace/all-target integration check/strict Clippy, оба
actual delivery PG tests и exporter equality. До/после совпадают242 CI-CD/Base
source hashes. Finally удаляет exact project containers/network и единственный
own disposable volume; external caches сохраняются. Evidence folder
`.local/task-delivery-qa/sdlc-qa-forge-delivery-aff4e88982fb/`:

| File | SHA256 |
| --- | --- |
| `compose.log` | `8a92ce2ff52eb7dec4749036058387ce4517be72800582b1782bf8409805b4b5` |
| `sources.json` | `68f5b99828b9d5905ac2f3b6e7d5b9f5737b3e0ab212cc120b92ab52e2668dde` |
| `cleanup.log` | `54b25e0b2561b4694d120df2acbf6bc948353aee85b24c176714b3795d5ae32b` |

Полный final-source gate `sdlc-qa-forge-delivery-4dc5be54f923` PASS:215 workspace
tests +80 PostgreSQL tests +2 real-API CLI tests,0 failed/ignored; strict
workspace/integration/CLI Clippy, fmt, release workspace build и exact exporter
equality. Активные graphs `rsa`/`sqlx-mysql` пусты. Те же242 inputs неизменны.
Evidence folder `.local/task-delivery-qa/sdlc-qa-forge-delivery-4dc5be54f923/`:

| File | SHA256 |
| --- | --- |
| `compose.log` | `0540b0d80898472e25ba02c71355b079f557123a08667020f1e9f663ff5925c3` |
| `sources.json` | `68f5b99828b9d5905ac2f3b6e7d5b9f5737b3e0ab212cc120b92ab52e2668dde` |
| `cleanup.log` | `82cee1cc6602bcee8480f9eadabd34b6e93e9924168286d954dc5576fb965657` |

Finally exact down подтвердил0 own containers/networks и удалил только own
disposable volume; PostgreSQL tmpfs после QA exit0 получил stop-timeout137
во время shutdown, это не restore/runtime/data acceptance. Post-cleanup Docker
audit complete=true: Desktop37, оба rootless runners0, violations=[], exit0.
Чужие valid Compose resources, permanent pins/snapshots/secrets/volumes сохранены.

Fresh frontend frozen/offline install, codegen/check/compat(main), lint,
201 tests, build и audit PASS (0 advisories); Vite сообщает существующий
chunk-size warning. `.local/frontend-manifest-20261008.log` SHA256:
`70e8a74a3f70e0e7970768c904bc39c351d6ef03082899a6816bf3fd29e084dc`.
Docs verifier,6 hosted docs regression cases, SBOM drift и tracked-export
secret scan475 text files/0 findings PASS; scanner heuristic, не DLP guarantee.

Предшествующие portable runs `d8e63b5d41d4` и `82e5785401dd` завершились25s
test deadline на runner/CLI; их причина не подтверждена и timeout не увеличен.
Добавлена bounded PG wait diagnostic. `93d5a0ae1486` остановился на Clippy type
complexity в этой diagnostic (исправлен type alias), `5df321b4cc35` на fmt drift
(исправлен fmt). Это не успешные gates. Old scoped `44e301a66fe6` PASS принадлежит
предыдущему source до final auth/receipt/curl guards; final-source gate выше.

Новый SQL migration отсутствует: единственный task-owned0040 сохранён. HTTP
dispatch и SDLC acceptance flags остаются false. Trusted root/origin/Unix storage,
один static artifact и technical application policy — ограничения этого среза; OCI,
migration/data compatibility и полная requirements coverage не заявляются.

## Открытые dependencies и release order, 2026-10-08

Текущий Tracker main `5a38fd5fada0c79b1d4fb2ea6a3f48cd8949d759` описывает SDLC
как target, не предоставляет authoritative Forge source/workspace admission.
Base source-status также явно фиксирует этот producer gap. Нельзя выдумывать
endpoint, обходить `dispatch_allowed=false` или принимать caller boolean за grant.

Сначала PR87/migration39, затем этот task-owned migration40/source cut. Отдельные
upstream gates: trusted Tracker source/access/fence readback; Fleet/Workflow admission;
scoped candidate branch/write capability; authoritative immutable candidate packet;
production executor с OCI/config identity и mutable migration/data compatibility;
полное scenario/requirements acceptance. Local static manifest identity, actual
application checks и last-confirmed rollback проверяются отдельным компонентом
выше и в owner-local OCI продолжении ниже не заменяют эти upstream/runtime gates.
Generic deployment status/rollback pipeline record не закрывают эти gates.
Ни production release, ни installation на постоянные стенды здесь не выполняются.

## OCI продолжение, 2026-10-08

Продолжение того же PR88 после static source `9f530ab`; migration0040, PR87,
Base pin и accepted runtime не меняются. Linux-only `forge-delivery --oci`
поддерживает один preloaded immutable local image и `readonly_snapshot_v1`.
Actual retained runner artifact содержит descriptor image ID/source commit;
Git/config/plan/ACK/artifact observer остаётся тем же sealed owner component.
Actual Docker revision label проверяет соответствие producer output, не signed
provenance. Effect не build/pull, не принимает caller Compose или shell.

Owner policy фиксирует exact daemon, unique temporary Compose project, принадлежащие
QA network/volume, trusted binaries, isolated root, host-visible execution specs,
snapshot SHA и application checks. Application получает два read-only mounts,
non-root UID, read-only rootfs, dropped capabilities, resource limits и internal
network. Docker socket controller — privileged boundary, не полноценный sandbox.

Verified требует actual image/container/config/mount/network, served manifest
до/после checks, health, отдельный application acceptance и compatibility readback
exact snapshot bytes. Actual schema/SHA проверяются до/после. Failed checks не
меняют last-confirmed; explicit rollback берёт его image/manifest и повторяет
все checks без записи snapshot. Unknown client/engine/version outcome удерживает
target. Durable child PID/start/exit + Linux parent-death signal ограничивают
Compose writer; missing identity после crash требует owner investigation.
Reconcile наблюдает уже запущенный container, не вызывает Compose up.

Final-source OCI gate `sdlc-qa-forge-delivery-4317be425055` PASS: Rust1.88
locked/offline fmt/check/strict Clippy с `integration` и `oci-integration`, один
actual daemon integration scenario (80 остальных отфильтрованы), exporter equality.
Сценарий выполняет настоящие runner Git checkout/build/upload/completion и A/B/C/D
container processes. A verified; B health503 и C acceptance422 дают failed evidence
и actual rollback к exact A image; schema2, migrations list и wrong image revision
отклонены до effect. SIGKILL во время observed health delay оставляет Unknown и
блокирует другой key; reconciliation получает verified для того же container ID
с неизменным current pointer mtime и исходным Unknown receipt. Snapshot неизменен.
Actual owner-side schema/data drift блокирует rollback; данные восстанавливает
только test fixture, executor не делает repair. Это technical fixture acceptance,
не acceptance реального продукта/полная requirements coverage.

Все247 CI-CD/pinned Base inputs совпадают до/после. Exact child Compose down
в Drop и wrapper finally, parent containers/network/disposable volume удалены;
temporary Windows junction удалён, реальные specs/evidence и external caches
сохранены. При работающем OCI child root Docker audit: complete=true, все три
daemon checked, violations=[], exit0 (Desktop42, runners0+0). Evidence folder
`.local/task-delivery-qa/sdlc-qa-forge-delivery-4317be425055/`:

| File | SHA256 |
| --- | --- |
| `compose.log` | `04bd49cbf806beabc8b3a1a34ce1f7cadb234d6c0db8bf8b618d47c389a2997c` |
| `sources.json` | `6200bf29bc5ae9fecbe77ae3bae4a787f793b2115f75b5adcc12b719a2827eea` |
| `cleanup.log` | `326ada899b07f5e8947933ee058bf8f70ac5f91bd67eb5114424fdbb578a09ef` |

Full final-source gate `sdlc-qa-forge-delivery-eff9c0038fb6` PASS:215 workspace
+80 PostgreSQL +2 real-API CLI tests,0 failed/ignored; strict integration/CLI
Clippy, fmt/check, release workspace build и exporter equality. Active `rsa` и
`sqlx-mysql` graphs пусты. Те же247 inputs неизменны. Evidence folder
`.local/task-delivery-qa/sdlc-qa-forge-delivery-eff9c0038fb6/`:

| File | SHA256 |
| --- | --- |
| `compose.log` | `4aa939b50f8ee29821e653ad6d4d689e2a556b27c70bb5afba06a78bde25a46d` |
| `sources.json` | `6200bf29bc5ae9fecbe77ae3bae4a787f793b2115f75b5adcc12b719a2827eea` |
| `cleanup.log` | `d91fc4823cc6de88be9e59305739328870d7a4d3da9574c861e0379f5703c93b` |

OCI controller image ID `sha256:b6cd7f996a5ff7a23ec9667c1f6077a64698268d3ae1fbbb6e81a0a3c1cb8a43`
с Rust1.88/standalone Compose; full Rust ID
`sha256:af306cfa71d987911a781c37b59d7d67d934f49684058f96cf72079c3626bfe0`,
PG ID `sha256:b0f9560a2de083e2cc7382e75f808c7381a32852a7ec49117deedb300e552b24`.
Script проверил actual immutable IDs. Parent PostgreSQL после tests получил exit0;
shutdown не служит production database/restore evidence. После обоих gates:
0 own containers/networks/volumes; junction отсутствует; root audit complete=true,
Desktop37/runners0+0, violations=[], exit0. Accepted resources сохранены.

Fresh frontend frozen/offline install, OpenAPI check/compat(main), typecheck/lint,
201 tests, build и audit0 advisories PASS; existing Vite chunk warning сохраняется.
`.local/frontend-oci-20261008.log` SHA256
`48871015ee963d9e0a3c59673fb6f0fefd1e264a4889a45491d9521c9a0f9eb3`.
Docs verifier/6 regression tests, SBOM drift, tracked-export secret scan481 text
files/0 findings PASS. UI не менялся; Playwright/axe/screenshots и full release
cargo-audit/Trivy pack не выполнялись. Hosted normal CI не запускает opt-in actual
OCI daemon test; exact-head result указывается в PR после push.

Предшествующий OCI run `0a54cb13df11` остановился на compile error helper return
(исправлен). `61b4d7fdba1f` обнаружил recovery unavailable: SIGKILL был до входа
actual application в health check, test synchronization исправлена по observed
application log, timeout не повышен. `29219e6cc270` actual OCI scenario прошёл,
но live host Docker audit exit1: реальный Linux Compose spec не был доступен
Windows auditor. Execution spec теперь хранится в host-visible bind directory с
temporary owned junction; final live audit выше прошёл. Эти runs не подменяют
final-source gate; все их own QA resources очищены.

Typed preflight `forge/local-oci-rejection/v1` даёт blocked reasons для unsupported
mutable data/migration, incompatible schema и snapshot drift. Generic errors дают
`unknown_or_rejected`, exit1: проверить original-key readback, effect мог начаться.
CLI readback exit0 — verified, exit2 — failed/unavailable/unknown; history не fresh
continuous health. HTTP POST остаётся503, static HTTP GET не читает OCI root,
`dispatchAllowed`/`sdlcAcceptanceVerified` false. Mutable PostgreSQL compatibility,
migrations/backfill/restore, multi-service/registry/native attestation, authoritative
Tracker/Fleet/Workflow admission и production/full business acceptance открыты.

## PostgreSQL source packet, 2026-10-08

`forge-delivery --postgres` добавляет отдельный isolated shadow protocol; OCI
read-only запреты выше сохраняются. Exact sealed artifact/image/source/schema,
complete immutable SQLx catalog, target lease/advisory guard, physical HBA/role
fence и actual session drain предшествуют backup/migration. Dump проходит real
fresh restore drill и schema/rows/sequences fingerprint. Candidate migrates fresh
DB, получает distinct identity/health/acceptance checks и отдельный writer release.

Known pre-release failure допускает explicit verified snapshot + last-confirmed
image restore в ещё одну fresh DB. Post-release old snapshot restore запрещён,
так как новые user writes могли быть приняты. Original-key replay не повторяет
dangerous effects; partial Unknown удерживает target. Reconcile может наблюдать
только фактически завершённый release. RPO=0 относится к acknowledged pre-drain
writes в supported local fence window; это не production PITR/DR/admission.

Final-source PostgreSQL gate `sdlc-qa-forge-delivery-ec6da7adab7c`:3/3 PASS.
Real A/replay/POST rows→B schema2/new write5; migration failure, health503 и
acceptance422; restore exact A image/rows1..4/next ID5. Corrupt backup, unknown
role/session/object owner/schema/history checksum, PUBLIC admission, reader write
privilege, data drift/stale lease reject. Пять SIGKILL checkpoints: backup-verified,
migration-intent, restore-intent, release-intent, writes-released. Original receipt
и checkpoint bytes/mtime сохраняются, новый key не обходит Unknown hold; completed
release reconcile сохраняет user write5/container ID и допускает new write6.
Old snapshot restore после release запрещён. Exact own cleanup завершён;
258 source hashes unchanged, live/final all3-endpoint audit violations0.

OCI regression `sdlc-qa-forge-delivery-6817d8d05dda`:1/1 PASS, actual image/data/
health/acceptance/rollback и SIGKILL/no-recreate recovery. Exact cleanup и258
inputs unchanged. Общий source manifest SHA256 обоих gates:
`2c18d685e07a0f4a549dab436603d14fabdd2ec36ff3da1ebd43b41a8b1b1c50`.
Normal backend gate `sdlc-qa-forge-delivery-9d4070b6c316`:297 tests
(workspace215/PG80/CLI2), fmt/check/strict Clippy/release/exporter PASS.
258 inputs unchanged; source manifest SHA256:
`e9ab43b279772dad9fd6446b1ef84f3ca0339c65c5dd3bb1c7f946262a95b565`.
Единственное отличие от PG/OCI manifests — QA Compose memory512MiB→1GiB
после подтверждённого cgroup OOM в normal run `e79657111588`; protocol/backend/
test/SDK bytes совпадают. Actual full QA использовал560MiB, tmpfs остаётся512MiB.
Failed/interrupted runs и cleanup перечислены в ledger.

Frontend Node22.20.0/pnpm10.28.1: frozen/offline install, OpenAPI check/compat(main),
tsc/lint,201 tests/build/audit0 PASS. Docs verifier/16 Python regressions, SBOM
drift и tracked-export secret scan492 text files/0 findings PASS. UI/lockfile/
generated OpenAPI не меняются. Screenshots/Playwright/axe/full cargo-audit/Trivy
не выполнялись; общий release security pack не заявляется. Hosted normal CI
проверяется на exact published head в PR; opt-in PG/OCI daemon suites в него не входят.
[Отдельный plan/ledger](../plans/2026-10-08-mutable-postgres-delivery.md),
[contract](contracts/MUTABLE_POSTGRES_DELIVERY.md), [ADR-0022](adr/0022-owner-local-postgres-shadow-delivery.md).
Нет нового Forge SQL migration; source0039/0040 bytes и PR87 сохраняются.

Production writer inventory/fence/drain ACK/LSN, traffic switch/incident decisions,
Tracker/Fleet/Workflow producers, native signed attestation, multi-service rollout,
arbitrary backfill/DDL и полная business acceptance остаются открытыми.
HTTP dispatch503 и оба SDLC flags false. Privileged owner/daemon/local filesystem
и binaries остаются trust boundary. Technical fixtures не выдаются за producer grant.
