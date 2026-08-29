# ADR-0004: Только PostgreSQL для постоянных данных

## Status

Accepted

## Context

Control plane требует согласованно хранить проекты, pipeline graph, job state transitions, append-only логи и в будущем очереди, leases, runner heartbeats, artifact metadata и аудит. На старте важнее одна надёжная операционная зависимость и транзакционная целостность, чем поддержка нескольких СУБД.

## Alternatives Considered

| Option | Pros | Cons |
|---|---|---|
| SQLite | Очень простой локальный запуск, один файл | Ограниченная модель конкурентной записи, сложнее multi-instance deployment, отличается от production-нагрузки |
| MySQL | Распространённая self-hosted СУБД | Дополнительная поддержка диалекта и поведения; меньше соответствие выбранным SQLx/PostgreSQL возможностям |
| PostgreSQL + Redis | Быстрая очередь и cache | Дополнительная критичная инфраструктура до подтверждения необходимости |
| Только PostgreSQL 17 | Транзакции, конкурентный доступ, SQLx поддержка, одна backup-процедура | Требует управления PostgreSQL и контроля роста БД |

## Decision

Использовать PostgreSQL 17 как единственное постоянное хранилище Forge CI/CD. Backend использует SQLx 0.8 и `PgPool`; схема задаётся committed SQLx migrations в `backend/migrations/*.sql` и в current MVP применяется при старте backend. SQLite, MySQL, Redis и файловое хранение состояния не поддерживаются.

PostgreSQL хранит исходное состояние и координационные данные. Когда появится S3-совместимое artifact storage, оно будет хранить только бинарные объекты, а метаданные и ссылки останутся в PostgreSQL.

## Consequences

- Одна транзакционная точка истины упрощает domain transitions, агрегирование статусов, recovery и backup.
- Docker Compose и production deployment должны предоставлять PostgreSQL 17, volume и health-check; API не запускается без доступной БД.
- Не нужно поддерживать абстракции и тестовую матрицу для разных SQL-диалектов.
- Эксплуатация обязана включать мониторинг подключений, размера БД, vacuum/maintenance и проверенное резервное копирование/восстановление.
- При будущей высокой нагрузке сначала масштабируются индексы, запросы, pooling и PostgreSQL; отдельные queue/cache хранилища добавляются только с отдельным ADR и измеримым обоснованием.

## Related

- `docs/DATA_MODEL.md`
- `docs/STORAGE.md`
- `docs/RUNTIME.md`
- `docs/adr/0001-rust-axum-sqlx.md`
