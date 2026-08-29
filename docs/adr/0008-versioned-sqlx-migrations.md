# ADR-0008: Versioned SQLx migrations

> **Уточнено ADR-0009:** канонический каталог SQL-файлов — `backend/migrations/*.sql`; crate инструмента — `backend/migration` (`cicd-migrate`). Production target — pre-runtime migration job вместо startup DDL.

## Status

Accepted (partially implemented: committed migrations + `cicd-migrate` + current CI real-DB; production owner/runtime rollout pending)

## Context

Исторический `store::migrate()` выполнял `CREATE TABLE IF NOT EXISTS` при старте. Такой bootstrap не мог безопасно изменить уже созданную таблицу, проверить историю схемы, дать repeatable deploy или clean real-PostgreSQL test database. Current MVP уже использует SQLx migration files, но применяет их из runtime-процесса при старте.

## Decision

Схема Forge CI/CD становится последовательностью immutable SQLx migrations в `backend/migrations/` с таблицей `_sqlx_migrations`. Новый instance получает baseline migration, существующие dev instances мигрируют через один документированный upgrade path. CI поднимает PostgreSQL service, применяет migrations и запускает integration tests. Изменение schema без migration запрещено.

## Consequences

- `store::migrate()` не расширяется для новой схемы; current startup migrator и `cicd-migrate` используют committed migration files.
- DDL, data backfill и destructive changes получают отдельные reviewable migration files.
- Backup/restore и version compatibility могут быть верифицированы.

## Related

- `docs/STORAGE_ARCHITECTURE.md`
- `docs/MIGRATIONS.md`
- `docs/DATA_MODEL.md`
- `docs/adr/0005-workspace-layered-architecture.md`
