# Операции Forge CI/CD

Этот документ описывает безопасную эксплуатацию Forge CI/CD и явно отделяет текущий локальный MVP от целевой production-архитектуры.

## Статус и граница доверия

- **Current verified:** локальный Docker Compose запускает PostgreSQL, backend и Dashboard; embedded runner выполняет jobs на том же узле. Health API, Docker healthcheck и структурированные логи доступны.
- **Configuration only:** schedules, webhooks и notifications можно настроить, но их выполнение и гарантированная доставка отсутствуют.
- **Target approved:** auth/RBAC, TLS-termination, versioned migrations, отдельные runner-ы с leases, transactional outbox, metrics/alerting, backup scripts и production DR.

> **Критическое ограничение MVP: только локальная или доверенная сеть.** API и Dashboard не защищены auth/RBAC, CORS permissive, TLS отсутствует, а PostgreSQL в `docker-compose.yml` опубликован на все интерфейсы хоста. Не публикуйте порты `22801`, `22802` или `22543` в недоверенную сеть и не используйте этот Compose как production-развёртывание. API-токены и Login UI сейчас не обеспечивают контроль доступа.

Доступ к Docker daemon, хостовой файловой системе, `.env`, bare Git-томам и backup-файлам считается привилегированным. Не передавайте реальные секреты через командную строку, Git, логи или скриншоты.

## Локальное развёртывание MVP

### Предварительные условия

Нужны Docker Engine с Docker Compose v2, Git и `curl`. Репозиторий и локальный файл конфигурации:

```bash
git clone git@github.com:FerrPOINT/CI-CD.git /opt/dev/CI-CD
cd /opt/dev/CI-CD
cp .env.example .env
```

В `.env` до любого совместно используемого развёртывания замените как минимум `CICD_DATABASE_PASSWORD` и `CICD_GIT_INTERNAL_TOKEN`; при использовании project secrets задайте уникальный `CICD_SECRETS_KEY`. Файл `.env` не коммитится. Порты по умолчанию: API `22801`, Dashboard `22802`, PostgreSQL `22543`.

Примеры ниже с `"$CICD_DATABASE_USER"`/`"$CICD_DATABASE_NAME"` предполагают, что значения загружены из `.env` в текущую shell:

```bash
set -a; . ./.env; set +a
```

### Запуск и приёмка

```bash
cd /opt/dev/CI-CD
docker compose config -q
docker compose up -d --build
docker compose ps
curl -fsS http://127.0.0.1:22801/api/v1/health
curl -fsS http://127.0.0.1:22802/ >/dev/null
```

Ожидаемый ответ API: `{"status":"ok","service":"cicd"}`. PostgreSQL проверяется отдельным healthcheck:

```bash
docker compose exec -T postgres \
  pg_isready -U "$CICD_DATABASE_USER" -d "$CICD_DATABASE_NAME"
```

`/api/v1/health` является liveness-проверкой: в Current verified он не подтверждает доступность PostgreSQL и не заменяет прикладной smoke. После запуска проверяйте также `docker compose ps` и, при необходимости, безвредный read API, например список проектов:

```bash
curl -fsS http://127.0.0.1:22801/api/v1/projects
```

Для просмотра логов используйте:

```bash
docker compose logs --tail=200 backend
docker compose logs -f backend
docker compose logs --tail=200 postgres
```

`RUST_LOG=info` является нормальным уровнем. Для ограниченной диагностики можно временно установить `RUST_LOG=debug`, но затем вернуть обычный уровень: debug-логи могут быть избыточны. Изменение `.env` применяется только пересозданием контейнеров.

### Остановка

```bash
# Остановить и удалить контейнеры, сохранив named volumes.
docker compose down

# Удалить также PostgreSQL, Git и artifact volumes: только для disposable local dev.
docker compose down -v
```

Перед `down -v`, `docker volume prune` или `docker system prune --volumes` создайте и проверьте backup. Эти команды могут необратимо удалить `cicd_postgres_data`, `cicd_git_repos` и `cicd_artifacts`.

## Production prerequisites (Target approved)

Production-развёртывание не должно быть вариантом текущего файла Compose с открытыми портами. До его допуска требуются все следующие свойства:

- enforced auth/RBAC, безопасные сессии или API-токены, audit и ограниченный CORS;
- TLS на reverse proxy, ограничение ingress по сети; PostgreSQL не имеет опубликованного host port и доступен только runtime/maintenance-сетям;
- отдельные `forge_owner` и `forge_runtime` credentials; owner используется только мигратором и backup/restore, runtime не имеет DDL-разрешений;
- образы привязаны к immutable version tag и digest, конфигурация и секреты приходят из secret manager, а не из `.env` в рабочем каталоге;
- versioned migrations, pre-deploy backup, readiness с проверкой БД, resource limits, централизованные логи, metrics и alerting;
- private versioned object storage для artifacts, защищённое Git storage, off-site encrypted backup и регулярный isolated restore drill.

Процедуры восстановления и дриллы — [DISASTER_RECOVERY](DISASTER_RECOVERY.md); реакция на инциденты и severity — [INCIDENT_RESPONSE](INCIDENT_RESPONSE.md). Целевые SLO и требования DR определены в [контракте жизненного цикла данных](contracts/DATA_LIFECYCLE.md): PostgreSQL RPO не более 15 минут, Git/artifacts RPO не более 24 часов и single-region RTO не более 4 часов. Эти показатели **не достигнуты Current verified MVP**.

## Обновление

### Current verified: локальный MVP

Сначала сохраните доказательства текущего состояния и проверьте diff/конфигурацию. Для изменения исходного кода, Dockerfile, образа или `.env` всегда пересоздавайте сервисы:

```bash
cd /opt/dev/CI-CD
git fetch origin
git status --short
git pull --ff-only origin main
docker compose config -q
docker compose up -d --build
docker compose ps
curl -fsS http://127.0.0.1:22801/api/v1/health
```

Если image уже собран и изменились только Compose-параметры, допускается:

```bash
docker compose up -d
```

> **Не используйте `docker compose restart` для нового образа, Dockerfile, исходного кода или env/config.** `restart` запускает старый контейнер с прежней конфигурацией; для применения изменения нужен `docker compose up -d` или `docker compose up -d --build`.

Перед обновлением с данными создайте manual backup из этого документа. Схема применяется versioned migrations (`backend/migrations/`, ADR-0008): при старте backend и отдельно через `cicd-migrate` (advisory lock, идемпотентно). Rollback не поддерживается — восстановление через backup + forward-only migrations (MIGRATION_CONTRACT).

### Target approved: controlled rollout

Целевой rollout выполняется по immutable release tag/digest, не через `git pull main` на production-хосте:

1. Проверить release evidence, compatibility и approved forward/restore procedure.
2. Создать verified pre-deploy backup и зафиксировать его ID.
3. Остановить writers или включить maintenance mode.
4. Запустить `cicd-migrate up` под `forge_owner`; затем `cicd-migrate verify`.
5. Пересоздать runtime только после успешной migration/verify и выполнить readiness/application smoke.
6. Наблюдать error rate, queue age, runner health и deliveries в согласованное окно наблюдения.

Автоматический down migration и ручное удаление строк из `forge._sqlx_migrations` запрещены. Ошибка migration означает остановку rollout; восстановление допускается только следующей безопасной forward migration либо restore до известной compatibility boundary.

## Миграции PostgreSQL

### Current verified

Версионируемых migration-файлов и бинарника `cicd-migrate` в репозитории сейчас нет. Backend создаёт/дополняет legacy schema при старте через bootstrap DDL. Поэтому оператор не должен выдавать bootstrap за проверяемую migration-систему и не должен вручную редактировать таблицы ради обхода ошибки запуска.

Для проверки текущей базы используйте только read-only диагностику:

```bash
docker compose exec -T postgres \
  psql -U "$CICD_DATABASE_USER" -d "$CICD_DATABASE_NAME" -c '\dt'
docker compose logs --tail=200 backend
```

### Target approved: `cicd-migrate`

После реализации единственный путь schema change -- immutable SQL-файлы в `backend/migrations/` и binary `cicd-migrate`. Server не применяет DDL: при pending migration или checksum mismatch он не становится ready.

Целевая последовательность для owner credential:

```bash
# Выполняется до запуска новой версии runtime и только после verified backup.
cicd-migrate up --database-url "$CICD_OWNER_DATABASE_URL"
cicd-migrate verify --database-url "$CICD_OWNER_DATABASE_URL"
```

Для исторической bootstrap-базы применяется только процедура `inspect-legacy` -> byte-for-byte сверка fingerprint -> `adopt-legacy --backup-id ...` -> `verify`; не пытайтесь подделать SQLx history вручную. Детали ролей, advisory lock, baseline и recovery приведены в [контракте миграций](contracts/MIGRATION_CONTRACT.md).

## Backup и восстановление

### Current verified: ручной backup

В MVP нет backup/restore/verify scripts. Упоминания `scripts/backup.sh`, `scripts/restore.sh` или `scripts/verify-backup.sh` в старых материалах не являются существующими исполняемыми средствами. До появления автоматизации оператор вручную сохраняет **согласованный набор**: PostgreSQL custom dump, bare Git directory и artifacts directory.

Локальные named volumes содержат:

- PostgreSQL metadata и job logs: `cicd_postgres_data`;
- bare Git repositories: `cicd_git_repos`, смонтирован в backend как `/var/lib/forge/git`;
- artifact bytes: `cicd_artifacts`, смонтирован как `/var/lib/forge/artifacts`.

Для наиболее согласованного manual backup дождитесь terminal jobs, приостановите новые push/API mutations и остановите backend. Current embedded runner может быть прерван остановкой backend, поэтому это maintenance-операция, а не online snapshot.

```bash
cd /opt/dev/CI-CD
set -eu

STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
BACKUP_DIR="${BACKUP_DIR:-$PWD/backups/$STAMP}"
mkdir -p "$BACKUP_DIR"

# Запомнить контейнер до остановки: docker cp работает и с остановленным контейнером.
BACKEND_CID="$(docker compose ps -q backend)"
test -n "$BACKEND_CID"

# Остановить writers и embedded executor на время согласованного снимка.
docker compose stop backend

# Custom format удобен для pg_restore и проверки структуры архива.
docker compose exec -T postgres \
  pg_dump -U "$CICD_DATABASE_USER" -d "$CICD_DATABASE_NAME" \
  --format=custom --no-owner > "$BACKUP_DIR/postgres.dump"

# Скопировать contents volumes, а не только metadata PostgreSQL.
docker cp "$BACKEND_CID:/var/lib/forge/git/." "$BACKUP_DIR/git"
docker cp "$BACKEND_CID:/var/lib/forge/artifacts/." "$BACKUP_DIR/artifacts"

# Минимальная проверка читаемости дампа и manifest без секретов.
pg_restore --list "$BACKUP_DIR/postgres.dump" >/dev/null
(
  cd "$BACKUP_DIR"
  find git artifacts -type f -print 2>/dev/null | LC_ALL=C sort > files.txt
  sha256sum postgres.dump > SHA256SUMS
  sha256sum $(find git artifacts -type f -print 2>/dev/null | LC_ALL=C sort) >> SHA256SUMS
)

docker compose up -d backend frontend
curl -fsS http://127.0.0.1:22801/api/v1/health
```

Если один из Git/artifact каталогов пуст, `find` не выдаст файлов; это допустимо. При наличии файлов команда `SHA256SUMS` создаёт manifest для последующей сверки. Храните каталог backup вне Docker volumes и вне единственного хоста; шифруйте его внешним средством и ограничивайте доступ. Не помещайте в backup manifest значения из `.env`, plaintext secret или database password.

`pg_dump` и копирование томов не дают crash-consistent distributed snapshot. В частности, копия Git/artifacts без остановки backend не должна считаться восстанавливаемым production backup. После backup сохраняйте дату, commit/release, оператора, checksum manifest и результат restore drill.

### Current verified: ручное восстановление

Сначала выполните восстановление в изолированном disposable окружении. Восстановление поверх действующего экземпляра допустимо только как осознанная maintenance-операция с подтверждённым backup, остановленными writers и возможной потерей данных после момента снимка.

Ниже `<backup-dir>` -- каталог, созданный предыдущей процедурой. Команды заменяют содержимое базы, Git и artifacts на содержимое backup.

```bash
cd /opt/dev/CI-CD
set -eu
BACKUP_DIR="/absolute/path/to/<backup-dir>"
test -f "$BACKUP_DIR/postgres.dump"
sha256sum -c "$BACKUP_DIR/SHA256SUMS"

# Сохранить ID контейнера, пока volumes ещё подключены.
BACKEND_CID="$(docker compose ps -q backend)"
test -n "$BACKEND_CID"
docker compose stop frontend backend

# Восстановить metadata. Не удаляйте database или volume без отдельного approved плана.
docker compose exec -T postgres \
  pg_restore -U "$CICD_DATABASE_USER" -d "$CICD_DATABASE_NAME" \
  --clean --if-exists --no-owner < "$BACKUP_DIR/postgres.dump"

# Заменить файловые данные через volumes backend-контейнера.
docker run --rm --user root --volumes-from "$BACKEND_CID" \
  -v "$BACKUP_DIR:/backup:ro" alpine:3.21 sh -ceu '
    rm -rf /var/lib/forge/git/* /var/lib/forge/artifacts/*
    mkdir -p /var/lib/forge/git /var/lib/forge/artifacts
    tar -C /backup/git -cf - . | tar -C /var/lib/forge/git -xf -
    tar -C /backup/artifacts -cf - . | tar -C /var/lib/forge/artifacts -xf -
  '

docker compose up -d backend frontend
curl -fsS http://127.0.0.1:22801/api/v1/health
curl -fsS http://127.0.0.1:22801/api/v1/projects
```

После восстановления сравните `files.txt` и checksum manifest, выполните `git fsck` для каждого recovered bare repository и проверьте чтение нескольких ожидаемых pipeline/job/artifact записей через API. Не запускайте новые pipelines до завершения этой проверки. Если restore не проходит, сохраняйте исходные логи и эскалируйте; не лечите ошибку удалением data volume.

### Target approved: автоматизация и DR

Целевые скрипты `scripts/backup.sh`, `scripts/restore.sh` и `scripts/verify-backup.sh` **утверждены как направление, но ещё не существуют**. Они должны создавать encrypted off-site backup с immutable manifest, проверять checksum и публиковать успех только после verification. Скрипты не заменят production backup platform.

Target backup включает physical PostgreSQL base backup и continuous WAL для PITR, daily portable logical dump, проверенные `git bundle --all`/snapshot после `git fsck`, versioned artifact objects и manifest с LSN, applied migration version/checksum, object versions, SHA-256 и key IDs. Restore выполняется только изолированно: роли/schema, PostgreSQL до manifest boundary, migration verify без DDL, Git/artifact integrity, доступность key versions и read-only smoke. Restore drill обязателен не реже раза в месяц и после изменения migration, storage или key policy.

## Мониторинг и health

### Current verified

Наблюдаемость MVP ограничена Docker status/healthchecks, `/api/v1/health` и backend `tracing` logs. Prometheus endpoint, readiness endpoint, business metrics, queue-age и alert routing отсутствуют.

| Проверка | Команда | Значение |
|---|---|---|
| Compose services | `docker compose ps` | PostgreSQL должен быть `healthy`; backend должен быть running/healthy. |
| API liveness | `curl -fsS http://127.0.0.1:22801/api/v1/health` | Процесс backend отвечает; это не DB readiness. |
| PostgreSQL | `docker compose exec -T postgres pg_isready -U "$CICD_DATABASE_USER" -d "$CICD_DATABASE_NAME"` | База принимает подключения. |
| Dashboard | `curl -fsS http://127.0.0.1:22802/ >/dev/null` | nginx отвечает статическим приложением. |
| Логи | `docker compose logs --since 30m backend` | Ошибки запуска, execution и API requests. |
| Диск | `df -h` и `docker system df` | Риск заполнения volumes/logs/images. |

Health endpoint не должен быть единственным сигналом решения об инциденте: backend может отвечать, когда прикладные запросы не работают из-за PostgreSQL. Проверяйте PostgreSQL и read API отдельно.

### Target approved

Production monitoring добавляет liveness `/api/v1/health`, DB-aware readiness, защищённый `/api/v1/metrics`, централизованные structured logs и внешнюю uptime-проверку. Минимальные alert-и: API/DB unavailable, 5xx/error budget, disk pressure, backup age/failure, migration verify failure, runner offline, lease expiry/reconciliation, queue age, outbox retry lag/dead letters, Git/artifact integrity error и KMS/key availability. Labels не содержат tenant/project ID, URL, tokens, event/delivery IDs или secret data.

## Инцидент-ранбуки

Общее правило для каждого инцидента: зафиксируйте UTC-время, release/commit, affected IDs, `docker compose ps`, последние релевантные логи и принятое решение. Не изменяйте статусы jobs или записи базы SQL-командами; используйте API/UI либо target reconciliation. Перед destructive recovery создайте backup.

### Упавший pipeline

**Current verified: диагностика и действия**

1. Получить детали pipeline и определить failed job:
   ```bash
   curl -fsS http://127.0.0.1:22801/api/v1/pipelines/<pipeline-id>
   curl -fsS http://127.0.0.1:22801/api/v1/jobs/<job-id>/logs
   ```
2. Сопоставить timestamp с `docker compose logs --since 30m backend` и, для Docker execution, с контейнером `forge-job-<job-id>`:
   ```bash
   docker ps -a --filter "name=forge-job-<job-id>"
   docker logs forge-job-<job-id>
   ```
3. Устранить внешнюю причину (недоступный Git/image, ошибка команды, дефицит места), сохранить логи и только затем создать повтор через Dashboard или API:
   ```bash
   curl -fsS -X POST \
     http://127.0.0.1:22801/api/v1/pipelines/<pipeline-id>/retry
   ```
4. Если нужно прекратить нетерминальную работу, отменить pipeline через API, а не менять status в PostgreSQL:
   ```bash
   curl -fsS -X POST \
     http://127.0.0.1:22801/api/v1/pipelines/<pipeline-id>/cancel
   ```

**Target approved: процедура**

Проверить execution attempt, lease/runner, immutable plan, append-only logs и событие завершения. Retry создаёт новый attempt только после классификации причины и проверки идемпотентности command/deploy. Terminal failure порождает audit/event, а operator retry сохраняет correlation. Не повторяйте потенциально side-effecting deployment автоматически.

### Зависший job

**Current verified: диагностика и действия**

1. Получить pipeline/job/logs и проверить время последней строки лога.
2. Проверить backend, Docker execution container, host resources и PostgreSQL:
   ```bash
   docker compose ps
   docker compose logs --tail=200 backend
   docker ps -a --filter "name=forge-job-<job-id>"
   docker stats --no-stream
   df -h
   ```
3. Если задача не имеет прогресса и владелец подтверждает отмену, отправить pipeline cancel. Если это только одна terminal job с допустимой retry-семантикой, используйте `POST /api/v1/jobs/<job-id>/retry` после расследования.
4. Не используйте restart как способ применить новый образ/config; при необходимости пересоздания backend примените `docker compose up -d --build`, затем повторите health и pipeline inspection.

В Current MVP нет lease TTL, remote runner reconciliation или гарантированного auto-retry. Зависшая job требует ручного решения; не объявляйте её завершённой без API action/evidence.

**Target approved: процедура**

Сверить `job_leases`, `execution_attempts`, heartbeat и срок lease. После expiry прежний owner fencing-ится; reconciler останавливает stale execution, фиксирует причину и создаёт новый attempt только в соответствии с retry policy. Timeout job отменяет процесс, сохраняет diagnostic и переводит job в `failed` либо `canceled`. Manual completion/status rewrite запрещены.

### Потерянный runner

**Current verified: диагностика и действия**

Registry/heartbeat в MVP -- только inventory; remote runner registration, dispatch и leases ещё не реализованы. Проверить запись runner и backend logs:

```bash
curl -fsS http://127.0.0.1:22801/api/v1/runners
docker compose logs --tail=200 backend
```

Для реально исполняемой embedded job проверяйте доступность backend host и Docker execution container. Не удаляйте runner registry-запись для «восстановления» job: это не перезапускает execution. Отмените/повторите affected pipeline через API после диагностики.

**Target approved: процедура**

Runner считается unhealthy после отсутствия heartbeat более 45 секунд и offline после 120 секунд. Перевести runner в draining/disabled, запретить новые leases, сохранить evidence последнего heartbeat/capability и ждать lease expiry. Reconciler fencing-ит старого owner, очищает workspace/ресурсы по policy и передаёт безопасно повторяемую работу другому compatible runner. Credential rotate/revoke и повторная registration аудируются.

### Outbox backlog

**Current verified: диагностика и действия**

Transactional outbox, worker, `outbox_messages` и `outbox_deliveries` ещё не реализованы. Следовательно, в MVP нет очереди outbox, которую можно «разгрести». Configuration-only webhook/notification/schedule не имеет delivery backlog; диагностируйте фактический pipeline/Git flow и не создавайте фиктивные SQL-таблицы или ручные event-записи.

**Target approved: процедура**

1. Alert содержит queue age, pending/leased/failed counts, retry lag и worker version; зафиксировать их до изменений.
2. Проверить worker health, PostgreSQL capacity/locks, oldest `outbox_messages`, lease expiry и safe failure class в `outbox_deliveries`.
3. Восстановить worker или зависимость, не меняя immutable payload и не удаляя pending rows вручную.
4. Просроченные lease должны быть reclaimed idempotently; delivery retry следует backoff policy. Terminal dead letter требует alert и только явный audited replay/requeue с новой delivery generation.
5. После восстановления подтвердить уменьшение queue age, отсутствие duplicate side effects у consumer и отсутствие secret в logs/history.

Целевые гарантии и retry classification определены в [контракте событий и доставок](contracts/EVENT_CONTRACT.md).

## Релизный процесс

### Current verified: release candidate

Версия следует Semantic Versioning, тег имеет вид `vMAJOR.MINOR.PATCH`; изменения пользователя фиксируются в `CHANGELOG.md` до tag. Перед публикацией release candidate обязан пройти актуальные CI gates: backend format/clippy/tests/release build, frontend frozen install/tests/build и `docker compose build`.

Локальное воспроизведение:

```bash
cd /opt/dev/CI-CD

docker run --rm --entrypoint /bin/bash -v "$PWD/backend:/workspace" \
  -w /workspace rust:1.86-bookworm \
  -lc '/usr/local/cargo/bin/cargo fmt --check && /usr/local/cargo/bin/cargo clippy --all-targets -- -D warnings && /usr/local/cargo/bin/cargo test --workspace && /usr/local/cargo/bin/cargo build --release'

cd frontend
pnpm install --frozen-lockfile
pnpm test
pnpm build
cd ..

docker compose config -q
docker compose build
```

После review и успешного CI создайте неизменяемый annotated tag из release commit:

```bash
git tag -a v<version> -m "Release v<version>"
git push origin v<version>
```

Не перезаписывайте опубликованный tag или version-tagged image. При регрессии выпускайте patch release либо документируйте возврат на предыдущий known-good tag. Для локального развёртывания release применяйте раздел «Обновление», включая backup и `docker compose up -d --build`.

### Target approved: production release

Production release добавляет migration test job, clean/prior-schema upgrade evidence, pre-deploy backup ID, signed immutable image digests, approved deployment window, canary/controlled rollout, readiness and domain smoke, мониторинг после релиза и formal release decision. Любая destructive/contract migration требует expand/backfill/compatibility периода и tested forward/restore runbook. Release notes указывают breaking changes, migration requirement, rollback/restore boundary, image digest и known limitations.

## Связанные документы

- [CURRENT_STATE.md](CURRENT_STATE.md) -- единственный снимок фактически реализованных возможностей.
- [Контракт миграций](contracts/MIGRATION_CONTRACT.md) -- нормативные target rules для `cicd-migrate`.
- [Контракт жизненного цикла данных](contracts/DATA_LIFECYCLE.md) -- target backup, retention и DR.
- [Контракт runner protocol](contracts/RUNNER_PROTOCOL.md) -- target leases, heartbeats и fencing.
- [Контракт событий и доставок](contracts/EVENT_CONTRACT.md) -- target outbox, deliveries и replay.
- [GIT_HOSTING.md](GIT_HOSTING.md) -- storage и Smart HTTP Git.
- [DEVELOPMENT_GUIDE.md](DEVELOPMENT_GUIDE.md) -- текущие CI gates.
