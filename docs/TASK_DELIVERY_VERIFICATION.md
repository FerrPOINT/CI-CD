# Forge task workspace, candidate и owner-local delivery evidence

**Статус 2026-10-08:** bounded source implementation и local component verification;
полного SDLC acceptance нет.

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
