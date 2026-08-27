# Контракт миграций PostgreSQL

Статус: Accepted target contract. Основание: [ADR-0009](../adr/0009-canonical-registry.md).

Этот контракт обязателен для изменения схемы. Runtime bootstrap DDL не является заменой versioned migration и не должен оставаться fallback после adoption.

## 1. Toolchain и source of truth

- Единственный каталог SQL migration -- `backend/migrations/*.sql`. Migration после merge не редактируется, не переименовывается и не удаляется; исправление всегда является следующей migration.
- Инструмент -- crate `backend/migration` с binary `cicd-migrate`. Он использует `sqlx::migrate!`, PostgreSQL и advisory lock `forge_migration_lock` с timeout 60 секунд.
- Версия `sqlx-cli` совпадает с workspace major/minor: `0.8.x`. CI запускает toolchain в `rust:1.86-bookworm`; host installation не требуется.
- SQLx history/checksum хранится в `forge._sqlx_migrations`. Pending migration, checksum mismatch или недоступный lock делают migration/verify failure.
- `cicd-server` не применяет DDL. Его startup verify только проверяет отсутствие pending migrations и завершает запуск ошибкой при несоответствии.
- Каждая migration имеет UTC numeric version и краткое `snake_case` имя. Первые committed baseline migrations имеют фиксированные имена: `0001_bootstrap_v1.sql`, `0002_runtime_role.sql`, `0003_auth_foundation.sql`.

## 2. Database roles и permissions

| Роль | Разрешения | Запрещено |
|---|---|---|
| `forge_owner` | Владелец database/schema `forge`, tables, sequences и extensions; выполняет `cicd-migrate`, backup/restore setup. | Runtime API/worker deployment. |
| `forge_runtime` | `CONNECT`, `USAGE` schema, явные `SELECT/INSERT/UPDATE/DELETE`, sequence usage; runtime tests. | `CREATE`, `ALTER`, `DROP`, `TRUNCATE`, `CREATE EXTENSION`, migration execution. |

Migration `0002_runtime_role.sql` выдаёт DML и default privileges для объектов `forge_owner` в schema `forge`. Нельзя использовать superuser, `forge_owner` или owner credentials в API/worker container. Каждая production connection имеет `search_path = forge, public`; domain tables в `public` запрещены.

## 3. Типы изменений и совместимость

| Фаза | Допустимое изменение | Правило rollout |
|---|---|---|
| Expand | Новая table, nullable column, additive index/API | Сначала schema, затем backward-compatible application. |
| Backfill | Идемпотентное обновление малыми batch | Progress хранится в `migration_progress`; restart продолжает работу. |
| Contract | `NOT NULL`, FK, CHECK, switch read/write path | Только после verified backfill и совместимого периода. |
| Drop | Удаление legacy column/table/index | Только после usage evidence, approved backup и compatibility deadline. |

Большой `CREATE INDEX CONCURRENTLY` располагается в отдельном SQL file с `-- no-transaction`; он выполняется в maintenance window, имеет resumable progress и сопровождается runbook. Migration не выполняет network I/O, не зависит от application memory и не содержит secret.

## 4. Baseline для legacy installation

Для пустой database `0001_bootstrap_v1.sql` создаёт точную поддерживаемую baseline. Для database, созданной историческим startup bootstrap, применяется только следующий путь:

1. Создать и верифицировать pre-migration backup; передать его идентификатор как `--backup-id`.
2. Остановить writers или включить maintenance mode.
3. Выполнить `cicd-migrate inspect-legacy --database-url ... --json > legacy-fingerprint.json`.
4. Сравнить fingerprint byte-for-byte с committed `backend/migration/fixtures/bootstrap-v1-fingerprint.json`.
5. Выполнить `cicd-migrate adopt-legacy --database-url ... --backup-id ...` только при пустой SQLx history и отсутствии pending migration.
6. Инструмент регистрирует baseline, применяет последующие migrations, затем выполняет `verify` и smoke.

Fingerprint включает tables, columns/types/null/defaults, PK/unique/FK/check constraints и indexes в deterministic lexical order. `adopt-legacy` не угадывает произвольную схему и при mismatch не меняет `forge._sqlx_migrations`; mismatch требует отдельного reviewed migration plan.

## 5. Изолированная test DB

`backend/docker-compose.test.yml` -- единственная compose fixture integration DB. Она использует `postgres:17-alpine`, database `forge_test_cicd`, tmpfs data volume, no host port, healthcheck `pg_isready` и `backend/tests/sql/init-roles.sql` для создания `forge_runtime`.

| URL | Назначение |
|---|---|
| `CICD_TEST_DATABASE_URL` | `forge_owner` URL для `cicd-migrate` и test setup. |
| `CICD_TEST_RUNTIME_DATABASE_URL` | `forge_runtime` URL для проверки runtime access и DDL denial. |

Harness отказывается от destructive setup, если database name URL не начинается с `forge_test_`. Parallel tests создают `forge_test_<uuid>` database/schema из migrated template и не делят mutable fixtures. CI и local cleanup всегда уничтожают test compose volume после работы.

## 6. Обязательные команды CI

Migration job выполняет следующую последовательность; `down -v` запускается в cleanup независимо от исхода предыдущего шага:

```bash
docker compose -f backend/docker-compose.test.yml up -d --wait
cicd-migrate up --database-url "$CICD_TEST_DATABASE_URL"
cicd-migrate verify --database-url "$CICD_TEST_DATABASE_URL"
cargo test --workspace
cicd-migrate verify --database-url "$CICD_TEST_DATABASE_URL"
docker compose -f backend/docker-compose.test.yml down -v
```

Локальные wrappers обязаны сохранять ту же семантику:

```bash
just db-test-up
action=up just migrate
just migrate-verify
just db-test-down
```

Required CI job `migration-test` поднимает test DB, применяет/verify migrations, выполняет real PostgreSQL tests и cleanup. Backend gate дополнительно запускает `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --workspace` и `cargo build --release`. CI должен использовать SQLx 0.8.x в pinned Rust container и подтверждать чистый `git diff` после generated migration artifacts, если они введены.

Минимальная migration suite содержит: empty bootstrap, upgrade от предыдущей released schema, legacy mismatch без history mutation, runtime DDL denial, checksum mismatch failure и concurrent migrator/advisory-lock behavior.

## 7. Deploy, failure и recovery policy

- Migration job запускается до новых application pods; rollout не продолжается при pending/failed/unknown migration state.
- Production migration сначала проходит на clean DB и representative prior schema в isolated environment, затем получает verified pre-migration backup и reviewed forward recovery procedure.
- Автоматический down/rollback SQL в production запрещён. Ошибочная migration не маскируется ручным `DELETE` из SQLx history.
- При сбое deployment останавливается. Восстановление допускается только через безопасную forward migration либо restore проверенного backup до известной compatibility boundary.
- Destructive или contract migration требует expand/backfill/dual-read/dual-write периода, migration progress, compatibility deadline и явного rollback/restore runbook.
- После restore обязательно выполняются migration verify, application smoke и audit записи о решении/результате recovery.

## 8. Проверяемые требования

- Каждое schema change имеет reviewed immutable SQL file, tests на empty/prior schema и документированную forward/restore policy.
- Integration tests доказывают, что `forge_runtime` выполняет нужный DML, но не DDL, а `forge_owner` требуется для migration.
- CI сохраняет evidence: applied version/checksum, test DB logs, migration verify result, backup ID для production change и release decision.
- Runtime readiness не становится healthy при pending migration или schema checksum mismatch.
