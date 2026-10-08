# Owner-local mutable PostgreSQL deployment protocol v1

Статус: Proposed (source implementation); локальная PostgreSQL suite verified. Продолжает
[ADR-0021](../adr/0021-owner-local-oci-readonly-data.md); ограничения read-only
OCI path сохраняются. Нормативные owners: [MIGRATION_CONTRACT](MIGRATION_CONTRACT.md),
[DATA_LIFECYCLE](DATA_LIFECYCLE.md). Этот protocol не выдаёт SDLC admission.

## Manifest и target

Sealed retained candidate `forge/postgres-candidate/v1` связывает exact Git commit,
pipeline config/plan/ACK/artifact SHA и immutable local image ID. Image revision
проверяется фактически; label не native/signed attestation. Source и target schema
имеют version, deterministic catalog fingerprint и immutable migration bytes/hash.
Application compatibility перечисляет readable schema versions. Неизвестная
schema/history/checksum или unsupported migration закрывают rollout до effect.

Supported pending SQL v1: одна `ALTER TABLE public.<identifier> ADD COLUMN
<identifier> <text|integer|bigint|boolean> NULL` без arbitrary SQL/backfill/down.
Исторические bytes сравниваются с SHA384 SQLx history, artifact catalog — с SHA256.
Dataset ограничен16MiB/16 public tables/512 rows на table, routines/triggers закрыты.

Owner policy отдельно связывает выделенный temporary Compose project, actual daemon
и PostgreSQL system identifier, source DB, allowed DML/read roles, immutable previous
image, trusted tools/root, bounded deadlines и отдельные health/acceptance checks.
Caller не передаёт DB credentials, SQL command, Compose, target paths или готовый
backup/verified/drained boolean. PostgreSQL credentials — только protected env.
Target topology/roles/sessions проверяются реально; arbitrary shared DB запрещена.

## Drain, lease и fencing

Owner machine write ACL проверяется свежо; Forge authorization locks удерживаются
на время owner command. Отдельный target PostgreSQL advisory guard и process/OS
lock допускают одного coordinator. Durable monotonic operation generation и
bounded lease deadline проверяются перед каждой опасной фазой. Expiry не освобождает
unknown writer и не разрешает повтор command.

Draining останавливает application, закрывает login/CONNECT writer role и проверяет
прекращение прежних sessions и отсутствие иных writers/privileges. Все acknowledged
pre-drain writes должны присутствовать в снимке. Unknown privilege/session/topology
блокирует backup/promotion/restore. Superuser/owner/daemon — privileged trust boundary;
production внешних writers нельзя объявлять drained только по local observations.

PUBLIC database CONNECT/CREATE/TEMP должен быть закрыт, иначе повторное LOGIN
writer может открыть старую source DB. Application objects принадлежат PostgreSQL
administrator; reader не получает DML/CREATE, writer — CREATE/TEMP или SQLx history
write. Database/role/object grants проверяются вместе с fingerprints, а не только
session inventory. Probes запрещают redirects/proxy, connect/SQL/process deadlines
ограничены; application Compose creation90s входит в общий lease5..300s.

Owner SQL и SQLx migrations имеют server statement8s/lock3s deadlines. Для
pg_dump/pg_restore действует20s local process deadline: эти utilities могут сбросить
server timeout settings, поэтому PGOPTIONS не доказывает прекращение remote effect.
После timeout/SIGKILL возможный продолжающийся session/effect остаётся Unknown;
fresh isolated DB/source fence и запрет replay сохраняются до доказанного outcome.

## Backup и isolated migration

Backup выполняется до target change после подтверждённого drain. Manifest содержит
source database/system identity, schema/data/sequence fingerprints, exact previous
image/artifact manifest, dump SHA, lease/generation и snapshot boundary. Dump SHA
недостаточен: backup восстанавливается в fresh isolated rehearsal DB, schema/history,
rows и sequence values сверяются с source snapshot. Только такой backup verified.

Pinned Base utilities используются точечно для pg_dump/pg_restore и hashes; business
state machine и authorization принадлежат Forge. Нельзя запускать полный platform
restore и затрагивать другие databases/volumes. Fresh DB ownership проверяется до
CREATE/restore; destructive restore поверх source/live DB запрещён.

Migration runner применяет retained immutable forward SQL в новой shadow DB с
statement/lock deadlines и transaction, version/checksum history. После migration
проверяет exact target catalog/history. Source DB не изменяется. Runtime DML role
не получает DDL, owner credential не передаётся application.

## Promotion, failure и restore

Application запускается с exact candidate image и shadow DB. Served manifest,
actual image/container/config/network и schema проверяются до/после health и
отдельного application acceptance. До успешных checks app writer остаётся закрыт.
Verified pair получает immutable evidence; release writer — отдельная journal phase.

Migration failure/health503/acceptance422 не объявляют новую пару confirmed. Explicit
rollback до release берёт verified pre-change snapshot и exact last-confirmed image,
восстанавливает его в ещё одну fresh isolated DB, сверяет rows/sequences/schema,
запускает прежний image и повторяет все checks. Source/failed candidate drift,
corrupt/missing/unverified backup, stale lease или неподтверждённый writer блокируют
restore/promotion. Down migrations и automatic repair/delete history запрещены.

После release write capability прежний snapshot больше не допустим для automatic
restore, даже если новый health стал failed: user writes могли быть приняты. Для
возврата нужен новый drain/verified snapshot и compatible forward packet либо
authoritative incident/data-loss decision, которой этот local protocol не выдаёт.

## Unknown и recovery

Immutable intent фиксируется до каждого опасного command. PID/engine/PG sessions
отличают живой прежний writer от прекратившегося; Linux parent-death завершает local
child coordinator. Docker exec/PG effect может пережить client: restart не повторяет
migration/restore/release. Readback показывает history; reconcile только наблюдает
фактические catalog/data/container/guard outcomes. Не доказанный или partial effect
остаётся Unknown и удерживает target. Новый key не обходит hold.

## RPO и required producer

Для supported isolated drain window RPO=0 относительно acknowledged pre-drain
writes: после snapshot новые app writes не принимаются до verified release; restore
сохраняет snapshot rows/sequences. Это не PITR/off-site SLO из DATA_LIFECYCLE и не
production DR. Записи после release требуют нового snapshot; отбрасывать их молча
запрещено. Backup encryption/retention/legal hold production owner отдельно.

Production producer должен предоставить проверяемый DB resource/system identity,
writer inventory и enforced write admission/fence, monotonic lease/readback с
freshness, drain ACK всех writers и доказанную snapshot/LSN boundary, migration
compatibility approval и разрешение traffic switch/restore. Tracker/Fleet/Workflow
grant и incident commander decision отсутствуют — production effect blocked.
Local tests не становятся такими producer receipts. SDLC flags остаются false.

## Фактическая verification

Обязательная suite: real A с пользовательскими rows→B valid migration и checks;
migration failure/health503/acceptance422; SIGKILL критических checkpoints и restart
без повторения опасного command; corrupt backup/unknown schema/stale lease/unquiesced
writer/data drift rejection; restore exact предыдущего image и согласованных rows.
Только disposable Compose projects с owner/purpose, exact finally cleanup и live/final
Docker group audit. Gate `sdlc-qa-forge-delivery-ec6da7adab7c`:3/3 PASS,
H/C/M/B, пять SIGKILL checkpoints, schema/history/PUBLIC admission/stale lease и
шесть restore negatives. Restore сохраняет rows1..4/next ID5; completed-release
reconcile сохраняет post-release writes/container identity.258 source inputs
неизменны, exact own cleanup завершён; live/final all3-endpoint audit violations0.
Полный quality/publication packet: [verification](../TASK_DELIVERY_VERIFICATION.md).
