# ADR-0022: Owner-local PostgreSQL shadow delivery

Статус: Proposed (source implementation); локальная PostgreSQL suite verified. Дата: 2026-10-08.

## Контекст

[ADR-0021](0021-owner-local-oci-readonly-data.md) подтверждает только read-only
snapshot. PostgreSQL deployment требует сохранить пользовательские строки,
sequence values и immutable SQLx history. Возврат старого image после изменения
schema сам по себе не восстанавливает согласованную application/database пару.

## Решение

Отдельный privileged `forge-delivery --postgres` принимает retained typed artifact,
а не произвольные SQL/credentials от HTTP caller. Supported target — выделенный
temporary Compose PostgreSQL, bounded dataset, два непривилегированных application
roles и owner-controlled host/daemon. Production HTTP dispatch остаётся закрыт.

Полный forward SQLx catalog содержит exact historical bytes и не более одной
pending additive nullable scalar column migration. Перед effects сравниваются
actual catalog/history и schema compatibility. Coordinator удерживает свежую Forge
authorization, target advisory lock, filesystem lock и bounded generation/lease.
HBA допускает PostgreSQL administrator только с адреса QA coordinator; application
не получает administrator credential. Drain закрывает writer login/CONNECT,
останавливает application и завершает writer sessions. Неизвестные writers блокируют
дальнейшие effects; privileged owner остаётся явной trust boundary.

Dump получает hash и database/system/schema/rows/sequences identity. Реальный
restore drill в fresh DB должен воспроизвести snapshot. Candidate мигрирует новую
shadow DB; source не изменяется. Exact image/database pair проверяется через
manifest, actual container и отдельные health/acceptance. Только затем journal
разрешает writer release. Все dangerous intents записываются до command.

Failed migration/health/acceptance допускает explicit restore verified snapshot
в ещё одну fresh DB с exact last-confirmed image. После writer release старый
snapshot restore запрещён: требуется новый snapshot/compatible forward packet или
отдельное authoritative incident decision. Automatic down SQL нет.

Restart/readback не повторяет migration/restore/release. Частичные эффекты остаются
Unknown и удерживают target. Observation-only reconcile может подтвердить только
уже завершённый release при совпадающей фактической паре и всех checks.

## Последствия

RPO=0 относится только к acknowledged writes до подтверждённого drain/snapshot;
это не production PITR/off-site DR. Application acceptance fixture проверяет rows
и schema, но не заменяет business acceptance или upstream authorization. Полный
SDLC blocked до producer contracts writer inventory/fence/drain ACK/LSN, traffic
switch/incident authorization и Tracker/Fleet/Workflow admission.

Pinned Base `875cac2` используется без изменения: точечные private helpers
`_write_database_dump`, `_restore_database`, `database_evidence`. Consumer адаптирует
их transport к bounded subprocess и fixed daemon. Это coupling к внутреннему API;
обновление Base требует отдельной проверки byte identity и real restore suite.
Полный platform backup/restore из этого пути не вызывается.

## Verification

[Contract](../contracts/MUTABLE_POSTGRES_DELIVERY.md),
[task packet/ledger](../../plans/2026-10-08-mutable-postgres-delivery.md).
Real PostgreSQL gate `ec6da7adab7c`:3/3 PASS. Known failures и verified restore,
пять SIGKILL checkpoints/no dangerous replay, completed-release recovery с
post-release writes, checksum/schema/PUBLIC/lease и restore negatives проверены.
258 source hashes unchanged, exact own cleanup и live/final Docker audit PASS.
Quality/publication evidence — [task verification](../TASK_DELIVERY_VERIFICATION.md).
Production admission/full business acceptance остаются открытыми.
