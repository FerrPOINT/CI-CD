# Architecture Decision Records — Forge CI/CD

## 1. Overview

ADR фиксируют ключевые архитектурные решения: контекст, альтернативы, выбор, последствия. Хранятся в `docs/adr/`. Каждый ADR — одно значимое решение, принятое в ходе разработки Forge CI/CD.

---

## 2. Format

Каждый ADR — файл `NNNN-title.md` в `docs/adr/`:

```markdown
# ADR-NNNN: Title

## Status

Proposed / Accepted / Deprecated / Superseded by ADR-NNNN

## Context

Какую проблему решаем? Какие ограничения и требования влияют на решение?

## Alternatives Considered

Таблица или список альтернативных вариантов с pros/cons.

## Decision

Что решили? Какой вариант выбран и почему?

## Consequences

Положительные и отрицательные последствия выбора.

## Related

Ссылки на связанные документы и ADR.
```

---

## 3. Active ADRs

| ID | Title | Status | Дата |
|----|-------|--------|------|
| ADR-0001 | Rust + Axum + SQLx для backend | Accepted | 2026-08-26 |
| ADR-0002 | React + Vite + Tailwind + shadcn/ui для Dashboard | Accepted | 2026-08-26 |
| ADR-0003 | Ручные переходы статусов job в MVP | Accepted | 2026-08-26 |
| ADR-0004 | Только PostgreSQL для постоянных данных | Accepted | 2026-08-26 |
| ADR-0005 | Cargo workspace и слоистая архитектура | Accepted | 2026-08-26 |
| ADR-0006 | PostgreSQL outbox для асинхронных эффектов | Accepted | 2026-08-26 |
| ADR-0007 | Исполнение отделено от control plane | Accepted | 2026-08-26 |
| ADR-0008 | Версионные SQLx migrations | Accepted | 2026-08-26 |
| ADR-0009 | Канонический реестр имён и приоритет источников | Accepted | 2026-08-27 |

---

## 4. Creating New ADRs

1. Взять следующий свободный номер (сейчас: 0010, затем 0011, ...); следующий ADR о auth/tenancy должен занять зарезервированный 0010.
2. Создать `docs/adr/NNNN-title.md` (шаблон имени) по формату из раздела 2.
3. Обновить индекс в этом файле (раздел 3) — добавить строку с номером, названием, статусом и датой.
4. Открыть PR с описанием решения и ссылкой на связанную issue/task.
5. После review и merge — статус `Accepted`.

### 4.1. Когда создавать ADR

- Выбор framework, библиотеки, СУБД, протокола.
- Архитектурное решение, влияющее на несколько компонентов.
- Решение, которое трудно изменить позже (мigrate cost high).
- Отказ от очевидной альтернативы с обоснованием.
- Изменение предыдущего архитектурного решения.

### 4.2. Когда НЕ создавать ADR

- Реализация конкретной фичи (фиксируется в коде и `docs/API.md`).
- Bug fix, refactor, обновление зависимости (minor/patch).
- Изменение текста в UI.

---

## 5. Superseding

Если решение меняется:

1. Создать новый ADR со статусом `Accepted`.
2. Старый ADR меняет статус на `Superseded by ADR-NNNN`.
3. Обновить индекс в этом файле: старый — `Superseded`, новый — `Accepted`.
4. В новом ADR указать ссылку на superseded в разделе `Related`.

---

## 6. Principles

- One significant decision — one ADR.
- Keep ADRs concise (1–2 страницы).
- Link to related docs (`docs/ARCHITECTURE.md`, `docs/DATA_MODEL.md`, другие ADR).
- Язык ADR — русский (в соответствии с языком документации проекта).
- Код и комментарии — на английском согласно `docs/CODE_STYLE.md`.
- ADR не удаляется; устаревшие решения помечаются `Deprecated` или `Superseded`.

---

## 7. Planned ADRs

Следующие ADR будут созданы по мере реализации фаз roadmap:

| Фаза | Тема | Статус |
|------|------|--------|
| Phase 1 | Auth: JWT + Argon2id | Planned |
| Phase 3 | YAML pipeline config format | Planned |
| Phase 5 | Runner protocol and job queue | Planned |
| Phase 6 | Webhook delivery and retry strategy | Planned |
| Phase 7 | Secret encryption (AES-256-GCM) | Planned |
| Phase 8 | Artifact storage backend (FS / S3) | Planned |
| Future | RBAC model | Planned |
| Future | Redis for cache and pub/sub | Planned |

---

## 8. References

- `docs/ARCHITECTURE.md` — общая архитектура.
- `docs/adr/0001-rust-axum-sqlx.md` — Rust + Axum + SQLx.
- `docs/adr/0002-react-vite-tailwind.md` — React + Vite + Tailwind.
- `docs/adr/0003-manual-job-transitions.md` — Ручные переходы статусов.
- `docs/adr/0004-postgresql-only.md` — Только PostgreSQL.
- `docs/adr/0005-workspace-layered-architecture.md` — Cargo workspace и слоистая архитектура.
- `docs/adr/0006-postgresql-outbox.md` — надёжные асинхронные эффекты.
- `docs/adr/0007-runner-security-boundary.md` — граница control plane/execution.
- `docs/adr/0008-versioned-sqlx-migrations.md` — versioned migrations.
- `docs/adr/0009-canonical-registry.md` — канонический реестр имён и authority matrix.
- `docs/ROADMAP.md` — план разработки.
- `docs/CODE_STYLE.md` — конвенции кода.