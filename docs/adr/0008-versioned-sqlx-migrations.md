# ADR-0008: Versioned SQLx migrations

> **Уточнено ADR-0009:** канонический каталог SQL-файлов — `backend/migrations/*.sql`; crate инструмента — `backend/migration` (`cicd-migrate`). вместо startup schema bootstrap

## Status

Accepted (target architecture; implementation pending)

## Context

Текущий `store::migrate()` выполняет `CREATE TABLE IF NOT EXISTS` при старте. Такой bootstrap не может безопасно изменить уже созданную таблицу, проверить историю схемы, дать repeatable deploy или clean real-PostgreSQL test database.

## Decision

Схема Forge CI/CD становится последовательностью immutable SQLx migrations в `backend/migration/` с таблицей `_sqlx_migrations`. Новый instance получает baseline migration, существующие dev instances мигрируют через один документированный upgrade path. CI поднимает isolated Postgres, применяет migrations и запускает integration tests. Изменение schema без migration запрещено.

## Consequences

- `store::migrate()` уходит после проверки baseline и upgrade path.
- DDL, data backfill и destructive changes получают отдельные reviewable migration files.
- Backup/restore и version compatibility могут быть верифицированы.

## Related

- `docs/STORAGE_ARCHITECTURE.md`
- `docs/MIGRATIONS.md`
- `docs/DATA_MODEL.md`
- `docs/adr/0005-workspace-layered-architecture.md`
