# Forge task workspace и candidate evidence

**Статус 2026-10-07:** bounded source implementation; полного SDLC acceptance нет.

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
Task binding здесь declared test input: admission/scheduler/delivery не включены.

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

## Открытые dependencies и release order

Текущий Tracker main `5a38fd5fada0c79b1d4fb2ea6a3f48cd8949d759` описывает SDLC
как target, не предоставляет authoritative Forge source/workspace admission.
Base source-status также явно фиксирует этот producer gap. Нельзя выдумывать
endpoint, обходить `dispatch_allowed=false` или принимать caller boolean за grant.

Сначала PR87/migration39, затем этот task-owned migration40/source cut. Отдельные
upstream gates: trusted Tracker source/access/fence readback; Fleet/Workflow admission;
scoped candidate branch/write capability; immutable candidate packet; deployment
executor с exact served artifact/config identity; health/data compatibility и
scenario/requirements acceptance; last-confirmed rollback identity и actual rollback.
Generic deployment status/rollback pipeline record не закрывают эти gates.
Ни production release, ни installation на постоянные стенды здесь не выполняются.
