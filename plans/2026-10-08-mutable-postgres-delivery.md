# Task packet: mutable PostgreSQL delivery, 2026-10-08

**Статус 2026-10-08:** owner-local source реализован; PostgreSQL/OCI suites verified.
Общий local quality gate PASS; публикация в существующий Draft PR88.
Local component готов к review; full SDLC admission blocked.
План/контракт сохранены до первого изменения кода.

## Исходный scope и публикация

Отдельный вертикальный packet после OCI/read-only source PR88/head
`205a7b9e7d9925ac636533ee4ca8652af31ab462`. PR88 OPEN/Draft/main; PR87 OPEN,
`fc3e107fd12f2d922f0c5c308df528782d5c7ba1`, owns0039; PR88 owns0040.
Applied bytes и соседние PR не менять. Новый SQL migration Forge не нужен:
application migration packet относится к отдельной target DB, не Forge schema.
Пользователь подтвердил продолжение существующего PR88 без новой SQL-миграции.
Срез публикуется отдельным коммитом; сохранён отдельный plan/contract.
Рабочий scope — только этот CI-CD checkout; Base/Fleet/Tracker/Workflow read-only.
Permanent runtime, accepted pins/snapshots/secrets/volumes не менять.

## План

1. Сверить принятые MIGRATION_CONTRACT/DATA_LIFECYCLE/DEPLOYMENT и pinned Base
   backup utilities. Зафиксировать protocol и honest authority/drain/RPO boundary.
2. Реализовать versioned sealed candidate manifest и owner-local coordinator:
   fresh machine write auth, target lease/advisory lock/fence, immutable journal,
   writer drain, verified backup/restore drill, isolated shadow migration,
   exact image/database promotion и отдельные application checks.
3. Реализовать original-key readback/replay и observation-only reconciliation.
   Unknown dangerous command не повторять. Restore только fresh isolated DB,
   из verified backup, до release writes; unsafe rollback блокировать.
4. Disposable Compose QA с real PostgreSQL/images/runner API: A/user writes→B,
   migration failure, health503, acceptance422; SIGKILL checkpoints; corrupt
   backup, stale lease, unknown schema, unquiesced writer/data drift; exact
   restored A image и согласованные rows. Зафиксировать RPO и cleanup evidence.
5. Required quality/contract/docs/security gates, own commits/PR publication,
   exact-head CI и review findings. Не merge автоматически и не объявлять SDLC ready.

## Нормативная граница

Поддерживаемый local target — выделенный disposable PostgreSQL server и один
Compose application, under trusted owner coordinator/daemon. Все app writes
идут через отдельную DML role; privileged administrator остаётся trust boundary.
Unknown writer topology/session, stale guard, unexpected role privileges или
данные блокируют effect. Контракт producer для production enforcement отдельно:
[MUTABLE_POSTGRES_DELIVERY](../docs/contracts/MUTABLE_POSTGRES_DELIVERY.md).

Использовать snapshot copy-on-write: менять только новую БД; source остаётся
fenced. RPO=0 относительно acknowledged writes до подтверждённого drain и
snapshot boundary. До verified promotion/release новые app writes запрещены.
После release прежний snapshot не data-safe rollback: требуется новый drain,
backup и compatible forward packet/incident admission. Automatic down SQL нет.

Pinned Base `875cac2` utilities потребляются без изменения Base. Full platform
backup/restore не запускать; использовать только точечные DB helpers для своих
Compose services и immutable backup hashing. Utilities не являются producer grant.

## Verification ledger

Ниже перечислены предшествующие failed/interrupted runs. Compilation diagnostic
`3ac341601fbe`: fmt/check/strict Clippy/new bins/example PASS, 0 PG tests — только
сборка, не real verification. Failed runs `0407589cdd06` (Unix-socket readiness
во время PG init), `fa88fbc0b0a7` (20s QA image build deadline), `a6b400db1860`
(default internal `pg_*` memberships ошибочно отклонялись). Все finally cleanup
завершены; 258 source inputs неизменны, собственные temporary volumes удалены.

Pre-final real gate `6571f181ddd3` использовал tools image
`sha256:9249c2f617fcd202e6f41936b01675a176a27f05e3e88959efd84be127153560`
и PG17 image `sha256:b0f9560a2de083e2cc7382e75f808c7381a32852a7ec49117deedb300e552b24`.
A verified/replay и настоящие POST rows3/4; H health503 с отдельным acceptance200;
corrupt dump/unknown role/admin session/candidate drift/reader write privilege
restore rejects; fresh restore exact A image и rows1..4, next serial ID5 PASS.
H/C/M failures и restores, valid B schema2/rows1..4/new write5 и post-release
restore rejection PASS. После known-outcome test запуск остановлен для дополнительной
проверки PUBLIC database admission/object ownership; whole suite не PASS. Finally
очистил exact child/parent/own volumes, 258 source hashes unchanged. Final-source
gate `7cc6a717087f` подтвердил A/H и шесть restore negatives, затем был остановлен
для server-side statement/lock deadlines в Base adapter. Finally exact cleanup,
258 source hashes unchanged; whole suite не PASS. Gate `abdf791aa3ab`
подтвердил H/C и restores, но следующий QA candidate builder превысил20s
Compose deadline; suite остановлена для расширения только QA-builder wait60s,
runner completion300s. Runtime command deadline20s не меняется. Finally exact
cleanup/own volumes и258 source hashes unchanged. Owner SQL/SQLx deadlines8s/3s;
pg_dump/pg_restore могут сбросить server settings,20s process timeout оставляет
возможный remote effect Unknown с запретом replay.
Run `3a5d3592053d`: startup readiness failed до protocol test; следующие factory
panics показали reused disposable volume после incomplete setup. Добавлены
RAII exact-project cleanup до создания fixture и readiness final TCP+actual seed
query с bounded startup grace. Finally own resources очищены,258 inputs unchanged.
Journal directory creation теперь fsync parent entry перед critical effects;
power-loss recovery отдельно не заявляется. Полная final suite повторяется.
Final-source gate `ec6da7adab7c`:3/3 PASS, H/C/M/B и шесть restore negatives,
пять SIGKILL checkpoints/no dangerous replay, post-release writes/container identity,
history/schema/PUBLIC admission/stale lease. Exact child/parent/own volumes cleanup
завершён;258 source inputs unchanged. OCI regression `6817d8d05dda`:1/1 PASS,
exact cleanup/258 inputs unchanged. Общий source manifest SHA256:
`2c18d685e07a0f4a549dab436603d14fabdd2ec36ff3da1ebd43b41a8b1b1c50`.
Live root audit с actual application и final PG audit:
все3 endpoints checked, violations0. Предыдущий audit во время чужого Fleet QA
показывал3 foreign missing-manifest violations, own violations0; чужие ресурсы
не изменялись. Python scripts regressions16/SBOM drift/docs verifier и
tracked-export secret scan492 text files PASS. Frontend Node22.20.0/pnpm10.28.1:
frozen/offline install, OpenAPI check/compat(main), tsc/lint,201 tests, build,
audit0 advisories PASS; generated contract/lockfile unchanged. Log:
`.local/frontend-postgres-20261008.log`. UI не меняется, screenshots/a11y не запускались.

Normal gate `e79657111588`: workspace215 PASS, затем PostgreSQL SIGKILL137;
kernel log03:54:32Z подтверждает CONSTRAINT_MEMCG/postgres OOM. Own exact cleanup
и258 hashes unchanged. Disposable control PostgreSQL memory limit поднят512MiB→1GiB,
tmpfs512MiB сохранён. Повторяется полный normal gate; protocol/backend/test/SDK
bytes после PG/OCI suites не меняются. Source manifest difference должен быть
только `deploy/qa/task-delivery.compose.yml`; это QA capacity, не runtime pin.

Final normal gate `9d4070b6c316`:297 tests (215+80+2), fmt/check/strict Clippy,
OpenAPI equality и release build PASS.258 inputs unchanged; manifest SHA256
`e9ab43b279772dad9fd6446b1ef84f3ca0339c65c5dd3bb1c7f946262a95b565`.
Сравнение с PG/OCI source manifests подтвердило только указанный QA Compose delta;
protocol/backend/test/SDK inputs совпадают. Actual PG memory560MiB/1GiB без OOM,
tmpfs512MiB. Exact finally cleanup завершён; final root audit после временного
Desktop endpoint timeout: all3 endpoints checked, violations0. Hosted exact-head
CI/reviews публикуются в PR88.

Evidence `.local/task-delivery-qa/<project>/`: exact specs/logs/source hashes и
safe `pg-evidence` JSON (private runtime connection files не экспортируются).
Fixtures не означают production writer admission.
