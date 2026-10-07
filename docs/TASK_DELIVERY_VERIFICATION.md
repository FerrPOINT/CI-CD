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
один static artifact и technical application policy — явные ограничения; OCI,
migration/data compatibility и полная requirements coverage не заявляются.

## Открытые dependencies и release order, 2026-10-08

Текущий Tracker main `5a38fd5fada0c79b1d4fb2ea6a3f48cd8949d759` описывает SDLC
как target, не предоставляет authoritative Forge source/workspace admission.
Base source-status также явно фиксирует этот producer gap. Нельзя выдумывать
endpoint, обходить `dispatch_allowed=false` или принимать caller boolean за grant.

Сначала PR87/migration39, затем этот task-owned migration40/source cut. Отдельные
upstream gates: trusted Tracker source/access/fence readback; Fleet/Workflow admission;
scoped candidate branch/write capability; authoritative immutable candidate packet;
production executor с OCI/config identity и migration/data compatibility;
полное scenario/requirements acceptance. Local static manifest identity, actual
application checks и last-confirmed rollback проверяются отдельным компонентом
выше и не заменяют эти upstream/runtime gates.
Generic deployment status/rollback pipeline record не закрывают эти gates.
Ни production release, ни installation на постоянные стенды здесь не выполняются.
