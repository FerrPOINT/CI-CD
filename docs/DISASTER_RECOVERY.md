# Аварийное восстановление Forge CI/CD

> **Назначение:** этот runbook описывает восстановление persistent data после потери хоста, тома, повреждения данных или неудачного изменения. Он не заменяет процедуру incident response: incident commander принимает решение о maintenance, восстановлении и возврате трафика.
>
> **Статус:** текущая manual-процедура ниже — **Current verified** для локального Docker Compose; автоматизированная production-схема — **Target approved**. SLO не должны считаться достигнутыми до появления проверяемых backup/restore evidence.
>
> **Нормативный источник retention, RPO/RTO, состава manifest и требований к target backup:** [контракт жизненного цикла данных](contracts/DATA_LIFECYCLE.md). Этот документ не переопределяет его.

## 1. Scope и роли

### Что защищаем

| Набор | Текущее размещение | Последствие потери | Уровень |
|---|---|---|---|
| PostgreSQL | named volume `cicd_postgres_data` | metadata control plane, состояния jobs, логи, artifact metadata, encrypted project secrets, audit | Tier 1 |
| Bare Git repositories | named volume `cicd_git_repos`, `/var/lib/forge/git` | Git objects, refs и hooks | Tier 2 |
| Artifact bytes | named volume `cicd_artifacts`, `/var/lib/forge/artifacts` | загруженные файлы артефактов | Tier 3 |
| Конфигурация и env | versioned Compose/release configuration и защищённый `.env`/secret manager | connection settings, paths, токены и ключи для расшифровки secrets | prerequisite всех tiers |

`docker compose down -v`, `docker volume prune` и `docker system prune --volumes` являются destructive-операциями для первых трёх наборов. До них нужен verified backup и явное решение оператора.

Не включайте в manifest или логи значения `.env`, database password, токены, plaintext secrets, `CICD_SECRETS_KEY` либо ciphertext secret. Для восстановления encrypted project secrets должна быть доступна ровно та версия ключа, которой они зашифрованы; отсутствие ключа фиксируется как блокер, а не обходится заменой данных.

### Роли и границы

- **Incident commander:** объявляет инцидент, выбирает backup/manifest boundary, разрешает destructive restore и возврат трафика.
- **Recovery operator:** выполняет команды, ведёт UTC-журнал и не запускает новые pipelines до успешного smoke.
- **Service owner:** подтверждает функциональный smoke и согласует допустимую потерю данных с выбранным RPO.
- Доступ к Docker daemon, volumes, backup storage, `.env` и secret manager — привилегированный. На время согласованного снимка и restore остановите writers, Git push и embedded runner.

## 2. Recovery tiers и RTO/RPO

| Tier | Граница восстановления | Целевое RTO | Целевое RPO | Статус |
|---|---|---:|---:|---|
| Tier 1 | control plane: API + PostgreSQL | single-region не более 4 часов | PostgreSQL metadata не более 15 минут | Target approved |
| Tier 2 | Git storage: bare repositories | single-region не более 4 часов | не более 24 часов | Target approved |
| Tier 3 | artifacts: metadata из Tier 1 + artifact bytes | single-region не более 4 часов | не более 24 часов | Target approved |

Таблица является краткой картой tiers. Канонические цели, retention и условия измерения приведены в [DATA_LIFECYCLE.md](contracts/DATA_LIFECYCLE.md#5-backup-restore-и-dr); Current verified MVP не обеспечивает эти SLO автоматически.

Восстанавливайте Tier 1 до Tier 2 и Tier 3: PostgreSQL определяет metadata и связи, по которым проверяются Git и artifacts. Если восстановлена только часть storage, не удаляйте несопоставимые данные автоматически: зафиксируйте finding, ограничьте доступ к повреждённому ресурсу и оставьте recovery window не менее 24 часов, как требует контракт.

## 3. Backup

### Current verified: ручной согласованный набор

В текущем MVP отсутствуют scripts backup/restore/verify и off-site automation. Manual backup должен содержать один каталог с `postgres.dump`, копиями Git/artifacts, `files.txt` и `SHA256SUMS`. Выполняйте его в maintenance window: дождитесь terminal jobs, остановите новые mutations и backend. Это прерывает embedded runner и не является online snapshot.

```bash
cd /opt/dev/CI-CD
set -eu
set -a; . ./.env; set +a

STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
BACKUP_DIR="${BACKUP_DIR:-$PWD/backups/$STAMP}"
mkdir -p "$BACKUP_DIR"

# Сохранить ID до остановки backend: он остаётся пригоден для docker cp.
BACKEND_CID="$(docker compose ps -q backend)"
test -n "$BACKEND_CID"

docker compose stop backend

docker compose exec -T postgres \
  pg_dump -U "$CICD_DATABASE_USER" -d "$CICD_DATABASE_NAME" \
  --format=custom --no-owner > "$BACKUP_DIR/postgres.dump"

# Копируются contents mounted volumes, а не metadata Docker volume.
docker cp "$BACKEND_CID:/var/lib/forge/git/." "$BACKUP_DIR/git"
docker cp "$BACKEND_CID:/var/lib/forge/artifacts/." "$BACKUP_DIR/artifacts"

pg_restore --list "$BACKUP_DIR/postgres.dump" >/dev/null
(
  cd "$BACKUP_DIR"
  find git artifacts -type f -print 2>/dev/null | LC_ALL=C sort > files.txt
  sha256sum postgres.dump > SHA256SUMS
  find git artifacts -type f -print0 2>/dev/null \
    | LC_ALL=C sort -z \
    | xargs -0r sha256sum >> SHA256SUMS
)

docker compose up -d backend frontend
curl -fsS http://127.0.0.1:22801/api/v1/health
```

После backup зафиксируйте UTC timestamp, operator, release/commit, результат `pg_restore --list`, checksum manifest, путь к off-site copy и результат последнего drill. Локальный backup внутри Docker volume или на том же единственном хосте не является аварийной копией.

### Target approved: автоматизированный backup

Целевая автоматизация следует [MIGRATION_CONTRACT.md](contracts/MIGRATION_CONTRACT.md) и [DATA_LIFECYCLE.md](contracts/DATA_LIFECYCLE.md#5-backup-restore-и-dr):

1. Создаёт PostgreSQL physical base backup и continuous WAL archive для PITR, а также daily portable logical dump.
2. Создаёт crash-consistent Git snapshot либо `git bundle --all` только после `git fsck`; проверяет checksum каждого snapshot/bundle.
3. Хранит artifacts в private versioned object storage и реплицирует конкретные object versions off-site.
4. Публикует encrypted immutable manifest лишь после verification: PostgreSQL backup ID, WAL/LSN boundary, migration version/checksum, Git snapshot/bundle IDs and SHA-256, artifact object versions/checksums, KMS/key IDs, application/schema version и digest manifest.
5. Отмечает backup в catalog как `verified` только после проверки checksums, manifest и доступности ключей; failure/превышение backup age alert-ится.

Целевые `scripts/backup.sh`, `scripts/restore.sh` и `scripts/verify-backup.sh` утверждены как направление, но пока не существуют. Не выдавайте описанную target automation за действующую Current verified capability.

## 4. Restore

### Общие требования до restore

1. Назначить incident commander, открыть UTC-журнал инцидента и выбрать backup/manifest, соответствующий требуемому RPO.
2. Проверить checksum и полноту выбранного набора; при target restore дополнительно сверить immutable manifest, LSN, migration checksum и key references.
3. Сначала развернуть isolated disposable environment. Restore поверх работающего экземпляра допустим только как одобренная maintenance-операция.
4. Остановить frontend/backend и writers; сохранить логи и состояние до destructive действий. Не удаляйте database или volumes без отдельного approved плана.
5. Восстановить configuration/release через versioned source; `.env` и secrets получить из защищённого хранилища, не из backup manifest. Сверить `CICD_DATABASE_*`, `CICD_GIT_ROOT`, `CICD_ARTIFACTS_DIR` и доступность версии ключа secrets.

### Tier 1: PostgreSQL и control plane

Для Current verified restore используйте custom dump. Команды ниже заменяют database contents, поэтому выполняются только после общих требований.

```bash
cd /opt/dev/CI-CD
set -eu
set -a; . ./.env; set +a
BACKUP_DIR="/absolute/path/to/<backup-dir>"
test -f "$BACKUP_DIR/postgres.dump"
sha256sum -c "$BACKUP_DIR/SHA256SUMS"

docker compose stop frontend backend

docker compose exec -T postgres \
  pg_restore -U "$CICD_DATABASE_USER" -d "$CICD_DATABASE_NAME" \
  --clean --if-exists --no-owner < "$BACKUP_DIR/postgres.dump"

docker compose up -d backend frontend
```

**Target approved:** создать roles/schema под owner credential, восстановить PostgreSQL до manifest boundary через PITR или approved logical restore, затем выполнить `cicd-migrate verify --database-url "$CICD_OWNER_DATABASE_URL"` без DDL. Pending migration, checksum mismatch или недоступный key блокируют возврат трафика. Автоматический down migration и ручное изменение `forge._sqlx_migrations` запрещены.

### Tier 2: bare Git repositories

Current verified restore файлов Git запускайте после database restore, пока backend остановлен. Сохраняйте до начала ID backend container: его volumes должны быть подключены до остановки.

```bash
cd /opt/dev/CI-CD
set -eu
BACKUP_DIR="/absolute/path/to/<backup-dir>"
BACKEND_CID="$(docker compose ps -q backend)"
test -n "$BACKEND_CID"
test -d "$BACKUP_DIR/git"

docker compose stop frontend backend

docker run --rm --user root --volumes-from "$BACKEND_CID" \
  -v "$BACKUP_DIR:/backup:ro" alpine:3.21 sh -ceu '
    rm -rf /var/lib/forge/git/*
    mkdir -p /var/lib/forge/git
    tar -C /backup/git -cf - . | tar -C /var/lib/forge/git -xf -
  '

# Проверить каждый bare repository до запуска writers.
docker run --rm --user root --volumes-from "$BACKEND_CID" alpine:3.21 sh -ceu '
  find /var/lib/forge/git -type d -name "*.git" -print0 |
    xargs -0r -n1 -I{} git --git-dir={} fsck --no-dangling
'
```

Если Git repository повреждён либо отсутствует, не открывайте clone/push для него. В target восстановите snapshot/bundle, подтвердите SHA-256 manifest и `git fsck`, затем зафиксируйте repository-level finding для reconciler.

### Tier 3: artifacts

Artifact metadata уже восстановлена из PostgreSQL; затем замените bytes и сверьте checksum manifest.

```bash
cd /opt/dev/CI-CD
set -eu
BACKUP_DIR="/absolute/path/to/<backup-dir>"
BACKEND_CID="$(docker compose ps -q backend)"
test -n "$BACKEND_CID"
test -d "$BACKUP_DIR/artifacts"

docker compose stop frontend backend

docker run --rm --user root --volumes-from "$BACKEND_CID" \
  -v "$BACKUP_DIR:/backup:ro" alpine:3.21 sh -ceu '
    rm -rf /var/lib/forge/artifacts/*
    mkdir -p /var/lib/forge/artifacts
    tar -C /backup/artifacts -cf - . | tar -C /var/lib/forge/artifacts -xf -
  '

(
  cd "$BACKUP_DIR"
  sha256sum -c SHA256SUMS
)
```

В target восстановите exact versioned object versions из immutable manifest и проверьте object checksum. Несовпадение metadata/bytes, checksum error или missing object блокирует download этого ресурса, создаёт incident/audit evidence и не исправляется удалением DB rows.

### Общий smoke после restore

После восстановления всех требуемых tiers запустите services и выполняйте read-only smoke до снятия maintenance. Не создавайте pipelines, не загружайте artifacts и не меняйте статусы jobs в ходе проверки.

```bash
cd /opt/dev/CI-CD
set -eu
set -a; . ./.env; set +a

docker compose up -d backend frontend
docker compose ps

docker compose exec -T postgres \
  pg_isready -U "$CICD_DATABASE_USER" -d "$CICD_DATABASE_NAME"
curl -fsS http://127.0.0.1:22801/api/v1/health
curl -fsS http://127.0.0.1:22801/api/v1/projects
curl -fsS http://127.0.0.1:22802/ >/dev/null
```

Подтвердите также: (1) expected project/pipeline/job records читаются через API; (2) для всех recovered bare repositories прошёл `git fsck`; (3) выбранный known artifact доступен только в isolated environment и его digest совпадает с manifest; (4) backend logs не содержат restore/schema/key errors; (5) target-only `cicd-migrate verify` прошёл без DDL и доступна required KMS key version. Incident commander разрешает traffic и новые pipelines только после этих проверок.

## 5. Правило 3-2-1-1 и размещение копий

Каждый verified backup соблюдает правило **3-2-1-1**:

- **3 copies:** production dataset и минимум две резервные копии.
- **2 different media/storage systems:** например encrypted backup repository и versioned object storage; локальный Docker volume не считается независимой копией.
- **1 off-site:** копия в независимом failure domain, не на единственном production host/region.
- **1 offline or immutable:** WORM/immutable retention lock либо физически/логически изолированная offline copy, защищённая от удаления и ransomware.

Размещение: primary остаётся в production PostgreSQL/Git/object storage; operational copy — в ограниченном backup storage другого storage system; off-site immutable copy — в отдельном account/project и независимом failure domain. Все копии шифруются in transit and at rest, access least-privilege, retention не короче [DATA_LIFECYCLE.md](contracts/DATA_LIFECYCLE.md#3-retention-и-удаление): daily не менее 35 дней, monthly не менее 12 месяцев. Ключи шифрования и их backup должны быть доступны по процедуре break-glass, но не храниться в том же manifest или обычном backup bucket.

## 6. DR drills

### Cadence

- **Current verified operating control:** quarterly tabletop + isolated manual restore drill, пока automation отсутствует.
- **Target approved requirement:** isolated restore drill не реже monthly и после изменения migration, storage или key policy, как определено в [DATA_LIFECYCLE.md](contracts/DATA_LIFECYCLE.md#5-backup-restore-и-dr).
- После real incident провести post-incident drill/review до закрытия corrective actions.

### Checklist drill

1. Выбрать scenario: PostgreSQL corruption, потеря Git volume, потеря artifact objects, потеря host или key-unavailable; задать target RTO/RPO.
2. Назначить incident commander/operator, зафиксировать UTC start и взять последний eligible verified backup/manifest.
3. Проверить checksum, manifest completeness, access к off-site copy и required key versions без раскрытия secrets.
4. Поднять isolated environment без production traffic; восстановить Tier 1, затем Tier 2 и Tier 3 по этому runbook.
5. Выполнить database/API/Git/artifact smoke и `cicd-migrate verify` там, где target migration tool доступен.
6. Измерить actual RTO и data point/LSN времени для actual RPO; сравнить с целями DATA_LIFECYCLE.
7. Уничтожить isolated drill environment или оставить его закрытым по approved evidence policy; не переносить drill data в production.
8. Провести review и назначить owners/dates для gaps до следующего drill.

### Что фиксировать

Drill report содержит scenario, scope/tier, UTC start/end, participants, backup ID/manifest digest и age, release/schema/migration version, RTO/RPO target vs actual, verification commands/results, Git/artifact checksum results, availability key versions, exceptions, evidence locations, residual risks и corrective actions с owner/due date. Audit должны покрывать restore, backup failure, key access/rotation и решение о возврате трафика.

## 7. RTO/RPO: соответствие DATA_LIFECYCLE

| DR tier | Набор данных | Канонический RPO | Канонический RTO | Authority |
|---|---|---:|---:|---|
| Tier 1 | PostgreSQL metadata / control plane | не более 15 минут | single-region не более 4 часов | [DATA_LIFECYCLE.md](contracts/DATA_LIFECYCLE.md#5-backup-restore-и-dr) |
| Tier 2 | Bare Git storage | не более 24 часов | single-region не более 4 часов | [DATA_LIFECYCLE.md](contracts/DATA_LIFECYCLE.md#5-backup-restore-и-dr) |
| Tier 3 | Artifact bytes / metadata dependency | не более 24 часов | single-region не более 4 часов | [DATA_LIFECYCLE.md](contracts/DATA_LIFECYCLE.md#5-backup-restore-и-dr) |

Эта таблица намеренно не дублирует lifecycle policy, retention, manifest schema или SLO definition. При расхождении действует [DATA_LIFECYCLE.md](contracts/DATA_LIFECYCLE.md); actual RTO/RPO подтверждаются только drill report, а не этим документом.

## Связанные документы

- [Операции](OPERATIONS.md) — текущие локальные backup/restore команды и incident runbooks.
- [Контракт жизненного цикла данных](contracts/DATA_LIFECYCLE.md) — нормативные retention, backup, restore и DR требования.
- [Контракт миграций](contracts/MIGRATION_CONTRACT.md) — roles, migration verify и policy recovery.
- [CURRENT_STATE.md](CURRENT_STATE.md) — фактически реализованные capability и ограничения MVP.
- [SLO.md](SLO.md) — target service objectives.
