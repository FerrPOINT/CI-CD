# ADR-0001: Rust + Axum + SQLx для backend

## Status

Accepted

## Context

Forge CI/CD — self-hosted control plane, который хранит состояние проектов, пайплайнов, стадий, задач и логов, а в следующих фазах будет координировать runner-ы. Нужен backend с предсказуемым потреблением ресурсов, безопасной конкурентностью, asynchronous I/O, явными доменными моделями и хорошей поддержкой PostgreSQL.

## Alternatives Considered

| Option | Pros | Cons |
|---|---|---|
| Go + chi + sqlc | Простая модель конкурентности, небольшие бинарники, быстрый onboarding Go-разработчиков | Меньше гарантий тип-системы для состояний и владения ресурсами; дополнительная генерация sqlc и отдельный слой интеграции |
| Go + chi + ручной SQL | Минимум зависимостей | Больше ручного mapping и риск расхождения SQL, моделей и ошибок |
| Rust + Actix Web + SQLx | Высокая производительность и зрелость | Более специфичная модель framework; Axum лучше согласован с Tower ecosystem |
| Rust + Axum + SQLx | Типобезопасность, async на Tokio, Tower middleware, явный SQL и PostgreSQL pool | Дольше компиляция и выше порог входа Rust |

## Decision

Использовать Rust edition 2024 с Axum 0.8, Tokio и SQLx 0.8 для HTTP API и доступа к PostgreSQL 17. Архитектура backend сохраняет разделение `api -> domain -> store`: HTTP слой не содержит правил переходов статусов, а store не определяет бизнес-логику.

SQLx выбран вместо code generation SQL-клиента: запросы остаются явными в Rust-коде, используют parameterized bind values и возвращают ошибки без скрытого ORM-поведения. `PgPool` предоставляет управляемый пул подключений, а async runtime Tokio совпадает с экосистемой Axum.

## Consequences

- Состояния pipeline/job и допустимые переходы выражены через enum и типы Rust, уменьшая вероятность некорректных переходов.
- API получает лёгкий async runtime, middleware экосистемы Tower и единый подход к graceful shutdown.
- SQL и схема PostgreSQL остаются обозримыми, без неявных запросов ORM.
- Разработчикам нужны знания ownership, lifetimes и async Rust; CI должен проверять `cargo fmt`, `cargo clippy`, тесты и release build.
- Время сборки и образ builder больше, чем у сопоставимого Go-сервиса; это принимаемая цена за гарантии и производительность.

## Related

- `docs/ARCHITECTURE.md`
- `docs/DATA_MODEL.md`
- `docs/CODE_STYLE.md`
- `docs/adr/0004-postgresql-only.md`