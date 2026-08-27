# Выбор технологий и библиотек — Forge CI/CD

> **Статус:** справочник. Привязка крейтов к фазам целевой архитектуры и ADR. Обновлять при смене решения — с новой записью в `docs/ADR.md`.

## Текущий стек (зафиксирован)

| Область | Выбор | Обоснование |
|---|---|---|
| Web framework | axum 0.8 + tower-http | Стандарт экосистемы, tower-слои |
| DB | PostgreSQL 17 + sqlx 0.8 (compile-checked) | ADR-0004 PostgreSQL-only |
| Сериализация | serde / serde_json / serde_yaml | — |
| Git | git2 0.20 (libgit2) + CLI git для merge-tree | bare-репозитории, Smart HTTP |
| Криптография | aes-gcm 0.10, sha2 0.10, base64 | Секреты AES-256-GCM at-rest |
| Логи | tracing + tracing-subscriber (env-filter) | — |
| Frontend | React 19 + Vite 6 + Tailwind 4 + shadcn/ui + TanStack Query | FSD-слои |
| CLI | clap 4 + reqwest (rustls) | Отдельный workspace-пакет |

## Крейты для целевых фаз

| Фаза | Крейт | Роль | ADR / док |
|---|---|---|---|
| OpenAPI-first | **utoipa 5 + utoipa-axum** | Генерация OpenAPI 3.1 из кода, Swagger UI/Redoc | DELIVERY_ARCHITECTURE |
| Auth | **argon2 0.5** | Argon2id password hashing | AUTHORIZATION |
| Auth | **jsonwebtoken 9 + axum-extra (cookie)** | JWT access/refresh, httpOnly cookie | AUTHORIZATION |
| Rate limiting | **tower-governor** | Per-IP/bearer лимиты, `secure()` пресет для login | SECURITY |
| Cron-scheduler | **tokio-cron-scheduler** (без postgres_storage) | Cron поверх tokio; идемпотентность держим в своей таблице `schedules` (dedup по schedule_id+fire_at) | AUTOMATION_ARCHITECTURE, ADR-0006 |
| Webhook HMAC | **hmac 0.12 + hex** | Constant-time верификация подписей доставки | AUTOMATION_ARCHITECTURE |
| Metrics | **axum-prometheus** | Стандартные HTTP-метрики, `/metrics` | DELIVERY_ARCHITECTURE |
| Object storage | **object-store** (Arrow) | S3/MinIO/GCS/local единый API — подключать при появлении второго бэкенда | STORAGE_ARCHITECTURE |
| Email | **lettre** | SMTP для email-канала уведомлений | AUTOMATION_ARCHITECTURE |

## Отклонённые варианты

| Вариант | Причина отказа |
|---|---|
| authx-core / authx-axum (full auth framework) | Свои RBAC-модель и контроль; встраиваем точечно argon2/jsonwebtoken |
| Redis (очереди/pub-sub) | ADR: PostgreSQL-only до реальной потребности в горизонтальном масштабе |
| Kafka/RabbitMQ | См. AUTOMATION_ARCHITECTURE — outbox в PostgreSQL достаточен |
| bollard (Docker API) | Embedded runner использует CLI `docker run`; переход на API — вместе с внешним runner-протоколом |
| prometheus / prometheus-client напрямую | axum-prometheus закрывает HTTP-метрики слоем; кастомные метрики добавим при появлении |

## Правила обновления

1. Новая зависимость уровня infra → сначала запись сюда, затем PR.
2. Смена фундаментального решения (БД, auth, executor) → новый ADR + правка этой таблицы.
3. Крейт с несовместимой лицензией или заброшенный (>1 года без релизов, открытые CVE) → поиск замены здесь же.
