# Операции Forge CI/CD

Этот документ описывает безопасную эксплуатацию Forge CI/CD и явно отделяет текущий локальный MVP от целевой production-архитектуры.

## Статус и граница доверия

- **Current verified:** локальный Docker Compose запускает PostgreSQL, backend и Dashboard; versioned SQLx migrations применяются при старте; embedded runner выполняет jobs на том же узле; health, `/metrics`, structured logs, conditional auth, schedules, outgoing webhook worker и `in_app`/`sse` notification delivery доступны.
- **Configuration only:** email/Slack notification adapters и inbound provider webhooks можно описать как target/config, но sender/handlers не исполняют внешнюю доставку.
- **Target approved:** TLS-termination, tenant isolation/scoped policy, отдельные runner-ы с leases, production scheduler/outbox guarantees, alerting, off-site/PITR backup platform и production DR.

> **Критическое ограничение MVP: только локальная или доверенная сеть.** Если `CICD_AUTH_SECRET` не задан или пустой, API и Dashboard работают open/trusted-network; при непустом секрете включаются JWT/scoped PAT, session-bound access invalidation, refresh rotate/logout/revoke, route roles и project memberships для project-owned ресурсов, но tenant isolation, service-account tokens, scoped Git credentials и production cookie/CSRF/session-family policy ещё не завершены. CORS по умолчанию permissive только для isolated dev; shared deployment обязан задать `CICD_CORS_ALLOWED_ORIGINS`. TLS отсутствует. PostgreSQL в `docker-compose.yml` привязан к `127.0.0.1`, но API/Dashboard host ports нельзя публиковать в недоверенную сеть и нельзя считать этот Compose production-развёртыванием.

Доступ к Docker daemon, хостовой файловой системе, `.env`, bare Git-томам и backup-файлам считается привилегированным. Не передавайте реальные секреты через командную строку, Git, логи или скриншоты.

## Локальное развёртывание MVP

### Предварительные условия

Нужны Docker Engine с Docker Compose v2, Git и `curl`. Репозиторий и локальный файл конфигурации:

```bash
git clone git@github.com:FerrPOINT/CI-CD.git /opt/dev/CI-CD
cd /opt/dev/CI-CD
cp .env.example .env
```

В `.env` до любого совместно используемого развёртывания задайте как минимум уникальные `CICD_DATABASE_PASSWORD` и `CICD_GIT_INTERNAL_TOKEN`; при использовании project secrets задайте уникальный `CICD_SECRETS_KEY`. Пустой `CICD_GIT_INTERNAL_TOKEN` допустим только для isolated local development, а legacy `forge-internal-dev-token` отклоняется при старте backend. Файл `.env` не коммитится. Порты по умолчанию: API `22801`, Dashboard `22802`, PostgreSQL `22543`.

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
curl -fsS http://127.0.0.1:22801/api/v1/readiness
curl -fsS http://127.0.0.1:22802/ >/dev/null
```

Ожидаемый liveness-ответ API: `{"status":"ok","service":"cicd"}`. Readiness возвращает `200` только когда PostgreSQL отвечает и все committed SQLx migrations применены без checksum mismatch; иначе endpoint возвращает `503` с `status:"not_ready"`.

PostgreSQL можно дополнительно проверить контейнерным healthcheck:

```bash
docker compose exec -T postgres \
  pg_isready -U "$CICD_DATABASE_USER" -d "$CICD_DATABASE_NAME"
```

`/api/v1/health` является liveness-проверкой и не должен использоваться как единственный сигнал готовности. После запуска проверяйте также `docker compose ps`, `/api/v1/readiness` и, при необходимости, безвредный read API, например список проектов:

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

- enforced auth/RBAC, безопасные сессии или API-токены, audit и `CICD_CORS_ALLOWED_ORIGINS`;
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

Перед обновлением с данными создайте verified backup через `scripts/backup.sh` и `scripts/verify-backup.sh` из раздела «Backup и restore». Схема применяется versioned migrations (`backend/migrations/`, ADR-0008): при старте backend и отдельно через `cicd-migrate` (advisory lock, идемпотентно). Rollback не поддерживается — восстановление через backup + forward-only migrations (MIGRATION_CONTRACT).

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

Версионируемые SQLx migration-файлы находятся в `backend/migrations/`, а binary `cicd-migrate` входит в workspace. Backend применяет pending migrations при старте через runtime `sqlx::migrate::Migrator`, загружая каталог из `CICD_MIGRATIONS_DIR` или дефолта `backend/migrations`; это удобно для локального MVP, но ещё не равно production rollout с отдельной owner/runtime ролью и pre-deploy verification gate.

Для проверки текущей базы используйте только read-only диагностику:

```bash
docker compose exec -T postgres \
  psql -U "$CICD_DATABASE_USER" -d "$CICD_DATABASE_NAME" -c '\dt'
docker compose logs --tail=200 backend
```

### Target approved: controlled `cicd-migrate`

Production-путь schema change -- immutable SQL-файлы в `backend/migrations/` и pre-runtime запуск `cicd-migrate`. Целевой server не применяет DDL: при pending migration или checksum mismatch он не становится ready.

Целевая последовательность для owner credential:

```bash
# Выполняется до запуска новой версии runtime и только после verified backup.
cicd-migrate up --database-url "$CICD_OWNER_DATABASE_URL"
cicd-migrate verify --database-url "$CICD_OWNER_DATABASE_URL"
```

Для исторической bootstrap-базы применяется только процедура `inspect-legacy` -> byte-for-byte сверка fingerprint -> `adopt-legacy --backup-id ...` -> `verify`; не пытайтесь подделать SQLx history вручную. Детали ролей, advisory lock, baseline и recovery приведены в [контракте миграций](contracts/MIGRATION_CONTRACT.md).

## Backup и восстановление

### Current verified MVP: scripted local backup

В MVP есть локальный scripted helper: `scripts/forge_backup.py` и wrappers `scripts/backup.sh`, `scripts/restore.sh`, `scripts/verify-backup.sh`. Он сохраняет **согласованный набор** для Docker Compose: PostgreSQL custom dump, bare Git directory, artifacts directory, `files.txt`, `SHA256SUMS` и `manifest.json` без `.env`/секретов. Скрипт не заменяет production backup platform: off-site copy, encryption, PITR, immutable storage и scheduled backup остаются обязанностью оператора/target.

Локальные named volumes содержат:

- PostgreSQL metadata и job logs: `cicd_postgres_data`;
- bare Git repositories: `cicd_git_repos`, смонтирован в backend как `/var/lib/forge/git`;
- artifact bytes: `cicd_artifacts`, смонтирован как `/var/lib/forge/artifacts`.

Для наиболее согласованного backup дождитесь terminal jobs, приостановите новые push/API mutations и позвольте helper-у остановить backend/frontend на время снимка. Current embedded runner может быть прерван остановкой backend, поэтому это maintenance-операция, а не online snapshot.

```bash
cd /opt/dev/CI-CD
scripts/backup.sh --backup-dir "$PWD/backups/$(date -u +%Y%m%dT%H%M%SZ)"
scripts/verify-backup.sh "$PWD/backups/<backup-id>"
```

Helper берёт `CICD_DATABASE_USER`/`CICD_DATABASE_NAME` из `.env` или окружения, сохраняет ID backend-контейнера до остановки, делает `pg_dump --format=custom --no-owner`, копирует contents mounted Git/artifact volumes, выполняет `git fsck` для bare repositories через backend image и создаёт checksum manifest. Если один из Git/artifact каталогов пуст, это допустимо. Храните каталог backup вне Docker volumes и вне единственного хоста; шифруйте его внешним средством и ограничивайте доступ. Не помещайте в backup manifest значения из `.env`, plaintext secret или database password.

`pg_dump` и копирование томов не дают crash-consistent distributed snapshot. В частности, копия Git/artifacts без остановки backend не должна считаться восстанавливаемым production backup. После backup сохраняйте дату, commit/release, оператора, checksum manifest и результат restore drill.

### Current verified MVP: scripted restore

Сначала выполните восстановление в изолированном disposable окружении. Восстановление поверх действующего экземпляра допустимо только как осознанная maintenance-операция с подтверждённым backup, остановленными writers и возможной потерей данных после момента снимка.

Ниже `<backup-dir>` -- каталог, созданный предыдущей процедурой. Команды заменяют содержимое базы, Git и artifacts на содержимое backup.

```bash
cd /opt/dev/CI-CD
scripts/verify-backup.sh "/absolute/path/to/<backup-dir>"
scripts/restore.sh "/absolute/path/to/<backup-dir>" --confirm-restore
curl -fsS http://127.0.0.1:22801/api/v1/health
curl -fsS http://127.0.0.1:22801/api/v1/projects
```

Restore script требует `--confirm-restore`, перед записью проверяет `SHA256SUMS`/`files.txt`/`manifest.json`, останавливает frontend/backend, запускает `pg_restore --clean --if-exists --no-owner`, заменяет Git/artifact volumes через backend image и выполняет `git fsck`, если не задан `--skip-git-fsck`. После восстановления проверьте чтение нескольких ожидаемых pipeline/job/artifact записей через API. Не запускайте новые pipelines до завершения этой проверки. Если restore не проходит, сохраняйте исходные логи и эскалируйте; не лечите ошибку удалением data volume.

### Target approved: production backup platform и DR

Локальные scripts являются MVP helper-ом. Production target должен создавать encrypted off-site backup с immutable manifest, PITR/WAL, schedule/retention, monitored backup age и публиковать успех только после verification. Скрипты не заменяют production backup platform.

Target backup включает physical PostgreSQL base backup и continuous WAL для PITR, daily portable logical dump, проверенные `git bundle --all`/snapshot после `git fsck`, versioned artifact objects и manifest с LSN, applied migration version/checksum, object versions, SHA-256 и key IDs. Restore выполняется только изолированно: роли/schema, PostgreSQL до manifest boundary, migration verify без DDL, Git/artifact integrity, доступность key versions и read-only smoke. Restore drill обязателен не реже раза в месяц и после изменения migration, storage или key policy.

## Мониторинг и health

### Current verified

Наблюдаемость MVP включает Docker status/healthchecks, `/api/v1/health`, `/api/v1/readiness`, `/metrics` Prometheus text и backend `tracing` logs. Alert routing, queue-age/dead-letter dashboards и production log pipeline отсутствуют.

| Проверка | Команда | Значение |
|---|---|---|
| Compose services | `docker compose ps` | PostgreSQL должен быть `healthy`; backend должен быть running/healthy. |
| API liveness | `curl -fsS http://127.0.0.1:22801/api/v1/health` | Процесс backend отвечает; это не DB readiness. |
| API readiness | `curl -fsS http://127.0.0.1:22801/api/v1/readiness` | PostgreSQL отвечает, `_sqlx_migrations` совпадает с configured SQLx migrations. |
| Metrics | `curl -fsS http://127.0.0.1:22801/metrics` | Prometheus text endpoint; в MVP не защищён отдельно от общей сетевой/auth границы. |
| PostgreSQL | `docker compose exec -T postgres pg_isready -U "$CICD_DATABASE_USER" -d "$CICD_DATABASE_NAME"` | База принимает подключения. |
| Dashboard | `curl -fsS http://127.0.0.1:22802/ >/dev/null` | nginx отвечает статическим приложением. |
| Логи | `docker compose logs --since 30m backend` | Ошибки запуска, execution и API requests. |
| Диск | `df -h` и `docker system df` | Риск заполнения volumes/logs/images. |

Health endpoint не должен быть единственным сигналом решения об инциденте: backend может отвечать, когда прикладные запросы не работают из-за PostgreSQL. Проверяйте readiness, PostgreSQL и read API отдельно.

### Target approved

Production monitoring добавляет защищённый metrics endpoint, централизованные structured logs, внешнюю uptime-проверку и alert routing поверх current readiness. Минимальные alert-и: API/DB unavailable, 5xx/error budget, disk pressure, backup age/failure, migration verify failure, runner offline, lease expiry/reconciliation, queue age, outbox retry lag/dead letters, Git/artifact integrity error и KMS/key availability. Labels не содержат tenant/project ID, URL, tokens, event/delivery IDs или secret data.

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

В Current MVP есть durable `job_queue`, embedded `job_leases` TTL/reconciliation, external runner protocol ack/renew/control/logs/complete, ack-timeout requeue, configurable queue-timeout diagnostic для dispatch-eligible job без compatible runner-а, basic tag/current executor compatibility и `forge-runner` shell process, но нет full lost-runner auto-dispatch для уже running execution, sandboxed runner или гарантированного auto-retry. Зависшая job требует проверки queue/lease/attempt и ручного решения; не объявляйте её завершённой без API action/evidence.

**Target approved: процедура**

Сверить `job_leases`, `execution_attempts`, heartbeat и срок lease. После expiry прежний owner fencing-ится; reconciler останавливает stale execution, фиксирует причину и создаёт новый attempt только в соответствии с retry policy. Timeout job отменяет процесс, сохраняет diagnostic и переводит job в `failed` либо `canceled`. Manual completion/status rewrite запрещены.

### Потерянный runner

**Current verified: диагностика и действия**

Registry/heartbeat legacy endpoints остаются inventory; external runner protocol MVP уже поддерживает `/api/v1/runner/register`, heartbeat, work poll и leases, а `forge-runner` запускает shell-MVP отдельным процессом; production sandbox/dispatch ещё не реализован. Проверить запись runner и backend logs:

```bash
curl -fsS http://127.0.0.1:22801/api/v1/runners
docker compose logs --tail=200 backend
```

Для реально исполняемой embedded job проверяйте доступность backend host и Docker execution container. Не удаляйте runner registry-запись для «восстановления» job: это не перезапускает execution. Отмените/повторите affected pipeline через API после диагностики.

**Target approved: процедура**

Runner считается unhealthy после отсутствия heartbeat более 45 секунд и offline после 120 секунд. Current queue timeout завершает только dispatch-eligible queued work, если нет compatible embedded/protocol runner path; он не создаёт retry новой attempt. Перевести runner в draining/disabled, запретить новые leases, сохранить evidence последнего heartbeat/capability и ждать lease expiry. Reconciler fencing-ит старого owner, очищает workspace/ресурсы по policy и передаёт безопасно повторяемую работу другому compatible runner. Credential rotate/revoke и повторная registration аудируются.

### Outbox backlog

**Current verified: диагностика и действия**

Transactional outbox MVP реализован через `domain_events`, `outbox_messages` и `outbox_delivery_attempts`: terminal pipeline event создаёт сообщения для enabled outgoing webhooks и local `in_app`/`sse` notifications; worker доставляет webhook-и с basic retry/backoff, помечает local notifications delivered, фиксирует attempts/outcome и останавливает exhausted delivery через `failed_at`. Failed delivery можно явно requeue через API; `outbox_deliveries` snapshots/leases, full dead-letter workflow, external notification adapters и точные crash-safe delivery guarantees ещё target.

Для диагностики текущего backlog используйте API history, read-only запросы к `outbox_messages`/`outbox_delivery_attempts` и backend logs; не редактируйте payload/attempts вручную:

```bash
curl -fsS "http://127.0.0.1:22801/api/v1/projects/$PROJECT_ID/outbox-deliveries?status=failed&limit=50"
docker compose exec -T postgres \
  psql -U "$CICD_DATABASE_USER" -d "$CICD_DATABASE_NAME" \
  -c "select channel, failed_at is not null as failed, count(*), min(next_attempt_at) from outbox_messages where delivered_at is null group by channel, failed"
docker compose logs --tail=200 backend
```

**Target approved: процедура**

1. Alert содержит queue age, pending/leased/failed counts, retry lag и worker version; зафиксировать их до изменений.
2. Проверить worker health, PostgreSQL capacity/locks, oldest `outbox_messages`, `next_attempt_at`/`failed_at` и safe failure classes в `outbox_delivery_attempts`; target-инциденты дополнительно проверяют lease expiry в `outbox_deliveries`.
3. Восстановить worker или зависимость, не меняя immutable payload и не удаляя pending rows вручную.
4. Просроченные lease должны быть reclaimed idempotently; delivery retry следует backoff policy. В current MVP terminal failed row повторяется только явным `POST /api/v1/outbox-deliveries/{delivery_id}/requeue`, который создаёт новую generation; target dead letter дополнительно требует alert и full audited operator workflow.
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
  -lc '/usr/local/cargo/bin/cargo fmt --all -- --check && /usr/local/cargo/bin/cargo clippy --workspace --all-targets -- -D warnings && /usr/local/cargo/bin/cargo test --workspace && /usr/local/cargo/bin/cargo build --release --workspace'

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
