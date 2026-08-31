# Целевая архитектура автоматизации и интеграций Forge CI/CD

> **Статус:** объяснительный narrative. Нормативные контракты — `contracts/EVENT_CONTRACT.md`; при конфликте прав контракт (ADR-0009). Текущее состояние — `docs/CURRENT_STATE.md`.

## 1. Назначение и границы

Документ определяет целевую архитектуру надёжной автоматизации Forge CI/CD:

- запусков по расписанию;
- приёма событий Git push;
- исходящих webhook-ов;
- уведомлений;
- фоновой обработки, повторов и reconciliation;
- аудита, наблюдаемости и безопасного поэтапного внедрения.

PostgreSQL остаётся единственным источником истины для состояния автоматизации. Внешние HTTP-вызовы, Git hooks, SMTP и API мессенджеров не считаются транзакционными ресурсами: система проектируется с доставкой *at least once*, идемпотентными обработчиками и наблюдаемой историей попыток.

Вне первой реализации остаются: полноценный GitHub/GitLab App, Kafka/RabbitMQ, произвольные пользовательские скрипты автоматизации и гарантия exactly-once доставки во внешние системы.

---

## 2. Текущее состояние и целевое состояние

| Область | Сейчас | Целевое состояние |
|---|---|---|
| Расписания | Таблица `schedules`, CRUD и формальная проверка наличия пяти cron-полей. Исполнителя нет. | Durable scheduler с IANA timezone, расчётом `next_fire_at`, учётом DST, политикой пропущенных запусков, дедупликацией и аудитом. |
| Git push | `post-receive` вызывает `/api/v1/internal/git-push`; передаются repository/ref/old_rev/new_rev. Hook best-effort, ошибка не влияет на push; повтор same `repository/ref/new_rev` дедуплицируется через `pipeline_triggers`. | Надёжный ingress с immutable delivery id, типом ref-операции, идемпотентным сохранением входящего события и периодической сверкой refs. |
| Связь проекта с репозиторием | Поиск первого `projects.repository_url ILIKE '%<repo>.git'`. | Явная связь `project_repositories`, уникальная в пределах назначения и независимая от URL-шаблонов. |
| Запуск pipeline | Pipeline создаётся сразу в HTTP-обработчике; нет `source_event_id`, SHA и dedupe key. | Создание pipeline в транзакции с причиной запуска, неизменяемым commit SHA и уникальностью по событию-триггеру. |
| Webhooks | Сохраняются `url`, `events`, `enabled`; секретов, доставки, истории и retry нет. | Подписки с HMAC, версиями секретов, delivery history, ограниченными повторными попытками, replay и dead-letter состояниями. |
| Уведомления | Сохраняются channel/target/enabled; `PUT` удаляет и создаёт конфигурацию заново. Доставки нет. | Правила, получатели, шаблоны, предпочтения, адаптеры каналов, агрегация и история доставок. |
| Фоновая обработка | Есть embedded runner: polling queued jobs раз в две секунды. Нет общего worker-контура. | Независимые типы workers с lease в PostgreSQL, `FOR UPDATE SKIP LOCKED`, `LISTEN/NOTIFY` как ускорением и безопасным shutdown. |
| Outbox | Отсутствует. | Бизнес-событие и outbox-запись создаются в одной БД-транзакции; dispatcher формирует deliveries. |
| Audit | Частичные записи в `audit_log`, без correlation id и без полного покрытия. | Неподменяемые автоматизационные события, технический аудит, correlation/causation ids и связность с доставками. |
| Наблюдаемость | `tracing`, HTTP TraceLayer и `/health`. | Readiness, Prometheus/OpenTelemetry, метрики очередей, retry, delivery и scheduler lag, алерты и runbook. |
| Миграции | `CREATE TABLE IF NOT EXISTS` при старте. | Версионированные миграции, обратносуместимые изменения и отдельная проверка миграций перед rollout. |

Фактические текущие границы подтверждаются `backend/src/platform.rs`, `backend/src/store.rs`, `backend/src/git_host.rs`, `backend/src/main.rs`, `docs/API.md` и `docs/GIT_HOSTING.md`.

---

## 3. Архитектурные принципы

1. **PostgreSQL-first.** Все решения о запуске, очереди, повторах, lease, дедупликации и финальных статусах сохраняются в PostgreSQL до внешнего вызова.
2. **At least once наружу, идемпотентно внутри.** Внешний HTTP-вызов может быть выполнен повторно после timeout или restart. Получатель должен дедуплицировать по `X-Forge-Delivery-Id` или `event.id`.
3. **Нет внешнего вызова внутри БД-транзакции.** Транзакция только изменяет локальное состояние и создаёт outbox-записи.
4. **Событие неизменяемо.** Payload события после записи не редактируется; изменения состояния доставки хранятся отдельно.
5. **Явные причины запуска.** Каждый pipeline имеет `trigger_type`, `trigger_event_id`, `requested_ref`, `resolved_sha`, actor и correlation id.
6. **Безопасность по умолчанию.** Секреты не включаются в события, логи и историю доставок; webhook URL проходит защиту от SSRF.
7. **Повторяются только временные операции.** Retry допустим для сетевых ошибок, timeout, HTTP `408`, `429` и `5xx`; не выполняется для валидации, HMAC-конфликта и большинства `4xx`.
8. **Наблюдаемость является частью контракта.** Любая автоматизация имеет идентификатор, статус, количество попыток, следующую попытку и диагностируемую конечную причину.

---

## 4. Целевая схема компонентов

```text
                         ┌──────────────────────────────┐
                         │ API / Git Smart HTTP / UI     │
                         │ команды, CRUD, внутренние API │
                         └──────────────┬───────────────┘
                                        │ транзакции
                         ┌──────────────▼───────────────┐
                         │ PostgreSQL                    │
                         │ domain_events                 │
                         │ outbox_messages               │
                         │ inbound_deliveries            │
                         │ schedules / schedule_fires    │
                         │ webhook_deliveries/attempts   │
                         │ notification_*                │
                         └───────┬─────────────┬─────────┘
                                 │             │
                    LISTEN/NOTIFY│             │ lease + SKIP LOCKED
                                 │             │
             ┌───────────────────▼───┐   ┌─────▼────────────────────┐
             │ Scheduler worker      │   │ Outbox dispatcher         │
             │ расчёт и fire cron    │   │ fan-out в deliveries      │
             └─────────────┬─────────┘   └──────┬───────────┬────────┘
                           │                    │           │
                    schedule.triggered          │           │
                           │             ┌──────▼─────┐ ┌──▼────────────┐
                           └────────────►│ Pipeline   │ │ Notification  │
                                         │ worker     │ │ worker        │
                                         └────────────┘ └──┬────────────┘
                                                            │
                                                ┌───────────▼───────────┐
                                                │ Webhook delivery worker │
                                                │ SMTP/Slack/etc adapters │
                                                └─────────────────────────┘

Git push -> post-receive -> Git ingress API -> inbound_delivery
                                              -> domain_event/outbox
```

Компоненты могут сначала исполняться в одном серверном процессе Tokio, но должны иметь отдельные циклы, лимиты конкурентности, типы задач и конфигурацию. В production их следует запускать отдельными deployment-ами с общей БД.

---

## 5. Единый контракт событий

### 5.1 Envelope

Каждое внутреннее и исходящее событие имеет версионированный envelope:

```json
{
  "id": "018f8fd4-8d34-7e8c-a5f0-3f19f0e73a11",
  "type": "forge.pipeline.finished.v1",
  "schema_version": 1,
  "occurred_at": "2026-08-26T12:34:56.123Z",
  "project_id": "b26f8b51-8adb-41dc-bd8c-b830fae11cc8",
  "aggregate": {
    "type": "pipeline",
    "id": "7a9d4d22-9f06-4e60-bd9f-66b5d2d8c1ec"
  },
  "correlation_id": "018f8fd4-8c91-7f6d-9f42-4c6fd2032203",
  "causation_id": "018f8fd4-8bce-7bf7-8f80-af0830ba9b28",
  "actor": {
    "type": "system",
    "id": "scheduler"
  },
  "data": {
    "status": "success",
    "git_ref": "main",
    "commit_sha": "a1b2c3d4..."
  }
}
```

Правила:

- `id` генерируется один раз и остаётся неизменным.
- `type` содержит домен, действие и версию: `forge.<domain>.<action>.vN`.
- `occurred_at` — время факта в UTC, а не время обработки worker-ом.
- `correlation_id` связывает весь сценарий, например Git push → pipeline → job → webhook.
- `causation_id` указывает непосредственное предыдущее событие.
- `data` не содержит секретов, полных токенов, неотфильтрованных HTTP headers и необработанных персональных данных.
- JSON schema события фиксируется в коде и OpenAPI/AsyncAPI-артефакте; изменение контракта требует нового `.v2`.

### 5.2 Транзакционный outbox

Изменение доменной сущности и создание события выполняются в одной транзакции:

```text
BEGIN;
  INSERT/UPDATE pipeline, job, schedule_fire или delivery;
  INSERT INTO domain_events (... immutable envelope ...);
  INSERT INTO outbox_messages (event_id, topic, available_at, status)
    VALUES (..., 'automation.dispatch', now(), 'pending');
COMMIT;
```

Dispatcher читает `outbox_messages`, формирует подписанные webhook deliveries и notification deliveries, после чего фиксирует результат локально.

Нельзя:

- создать pipeline, а событие записать отдельным запросом;
- отправить HTTP webhook до commit;
- считать `NOTIFY` источником истины;
- удалять успешное событие сразу после публикации.

`pg_notify('forge_outbox', message_id)` используется только для уменьшения latency. После restart worker обязан периодически опрашивать БД.

---

## 6. Расписания

### 6.1 Контракт расписания

Целевая сущность расписания:

```json
{
  "id": "uuid",
  "project_id": "uuid",
  "cron": "0 4 * * 1",
  "timezone": "Europe/Moscow",
  "git_ref": "main",
  "enabled": true,
  "misfire_policy": "fire_once",
  "misfire_grace_seconds": 900,
  "concurrency_policy": "forbid",
  "next_fire_at": "2026-08-31T01:00:00Z",
  "last_fire_at": "2026-08-24T01:00:00Z",
  "created_at": "2026-08-20T10:00:00Z",
  "updated_at": "2026-08-26T10:00:00Z"
}
```

Поддерживается только фиксированный cron-диалект из пяти полей:

```text
minute hour day-of-month month day-of-week
```

Поддержка шести полей, секунд, псевдонимов разных cron-реализаций или provider-specific синтаксиса не вводится без отдельного ADR.

`timezone` обязателен и соответствует IANA TZ database, например `UTC`, `Europe/Moscow`, `America/New_York`. Внутреннее хранение и сравнение времени всегда выполняется в UTC.

### 6.2 DST и время

Политика должна быть детерминированной:

- при повторяющемся локальном времени осенью запуск выполняется один раз — в первое соответствующее UTC-время;
- несуществующее локальное время весной пропускается;
- факт пропуска фиксируется как `forge.schedule.fire_skipped_dst.v1`;
- UI показывает timezone, следующее локальное время и UTC-время;
- изменение timezone пересчитывает `next_fire_at` атомарно.

### 6.3 Idempotency

Для каждого планового слота создаётся запись `schedule_fires`:

```text
UNIQUE(schedule_id, scheduled_for)
```

`scheduled_for` — нормализованное UTC-время конкретного cron-слота, а не время фактического захвата worker-ом. Если два scheduler-инстанса одновременно обнаружат один слот, только один `INSERT` пройдёт успешно.

Pipeline дополнительно получает уникальную связь с trigger event:

```text
UNIQUE(trigger_event_id)
```

Это исключает повторный pipeline при повторной обработке schedule fire.

### 6.4 Пропущенные срабатывания и конкуренция

`misfire_policy`:

| Значение | Поведение |
|---|---|
| `skip` | Просроченные интервалы не выполняются; создаётся аудит пропуска. |
| `fire_once` | Если задержка не превышает `misfire_grace_seconds`, запускается один актуальный fire; весь накопившийся backlog не воспроизводится. |
| `catch_up` | Не включается в первом релизе. Требует явного лимита количества запусков и одобрения ADR. |

`concurrency_policy`:

| Значение | Поведение |
|---|---|
| `allow` | Каждый допустимый fire создаёт отдельный pipeline. |
| `forbid` | Новый fire фиксируется как `skipped_concurrency`, если у расписания есть активный pipeline. |
| `replace` | Новый pipeline создаётся, текущий scheduled pipeline получает запрос отмены. Не применяется к ручным и Git-triggered pipeline. |

Для первоначального rollout значения по умолчанию: `fire_once`, grace 15 минут, `forbid`.

### 6.5 Алгоритм scheduler worker

1. Получить leader lease через PostgreSQL advisory lock либо строку `worker_leases`.
2. Выбрать `enabled` schedules с `next_fire_at <= now()` батчем и `FOR UPDATE SKIP LOCKED`.
3. Для каждой записи в короткой транзакции:
   - рассчитать due slots;
   - вставить `schedule_fires` с `ON CONFLICT DO NOTHING`;
   - пересчитать и записать `next_fire_at`;
   - создать `forge.schedule.triggered.v1` и outbox только для успешно вставленного fire.
4. После commit разбудить dispatcher.
5. Не удерживать lock на расписании во время получения Git SHA или создания внешних доставок.
6. При превышении допустимой задержки применять `misfire_policy`.

---

## 7. Приём Git push событий

### 7.1 Ограничения текущей реализации

Текущий hook отправляет repository/ref/old_rev/new_rev, использует статический header-токен в сгенерированном shell-файле, игнорирует ошибку через `|| true` и создаёт pipeline непосредственно из HTTP-handler. Повтор same `repository/ref/new_rev` уже дедуплицируется через `pipeline_triggers`, но это ещё не полноценный durable ingress. Остаются риски:

- потери события при недоступности API;
- повторного pipeline при старом hook без `new_rev` или при другом source/key;
- отсутствия immutable ingress event и audited replay;
- best-effort commit SHA вместо immutable event snapshot;
- неверного выбора проекта при нескольких похожих URL.

### 7.2 Новый hook payload

`post-receive` читает стандартные строки `oldrev newrev refname` и создаёт delivery для каждой ref-операции:

```json
{
  "delivery_id": "uuid",
  "repository_id": "uuid",
  "repository_name": "platform-core",
  "old_sha": "0000000000000000000000000000000000000000",
  "new_sha": "a1b2c3d4e5f6...",
  "ref": "refs/heads/main",
  "received_at": "2026-08-26T12:34:56.123Z",
  "hook_version": 1
}
```

Правила:

- `new_sha = 000...000` означает удаление ref; pipeline по умолчанию не создаётся.
- Ветки и теги обрабатываются отдельно через project trigger policy.
- SHA проверяется как 40- или 64-символьный hex согласно настроенной версии Git object format.
- `delivery_id` генерируется hook-ом один раз на строку `post-receive`.
- В internal API передаётся timestamp и HMAC-подпись, а не долгоживущий токен, встроенный в текст hook-а.
- Секрет hook-а передаётся через защищённый файл/переменную окружения Git service, недоступную пользователям репозитория.

### 7.3 Ingress API

Внутренний endpoint:

```text
POST /api/v1/internal/git-events/push
```

Требования:

- доступен только из внутренней сети либо через mTLS/reverse-proxy allowlist;
- проверяет HMAC, timestamp freshness и payload schema;
- отвечает `202 Accepted` после надёжного commit в PostgreSQL;
- не клонирует репозиторий и не запускает pipeline синхронно;
- не раскрывает информацию о проектах при ошибке авторизации.

В транзакции endpoint:

1. записывает `inbound_deliveries` с уникальностью `(source, delivery_id)`;
2. связывает repository с проектами через `project_repositories`;
3. создаёт `forge.git.push.received.v1`;
4. добавляет outbox message;
5. возвращает существующий результат для повторного `delivery_id`.

Если проект не подписан на ref, входящее событие всё равно сохраняется со статусом `ignored`; это необходимо для диагностики и reconciliation.

### 7.4 Pipeline trigger policy

Для каждого `project_repository` задаются:

- разрешённые refs: exact match или ограниченный glob;
- включение branch/tag triggers;
- действие для удаления ref: `ignore`;
- debounce window для серии push на один branch;
- `auto_cancel_superseded`;
- включение/отключение автоматизации.

Pipeline worker:

1. читает `forge.git.push.received.v1`;
2. проверяет policy;
3. определяет `resolved_sha = new_sha`;
4. читает `.forge-ci.yml` именно из `resolved_sha`;
5. создаёт pipeline с `trigger_type = git_push`;
6. записывает событие `forge.pipeline.queued.v1`.

Запуск по SHA исключает гонку: pipeline не начинает работу на более новом коммите, появившемся после push.

### 7.5 Reconciliation Git ingress

Best-effort hook не может быть единственной гарантией. Reconciliation worker:

- периодически получает refs локального bare repository;
- сравнивает текущий SHA с `repository_refs`;
- для изменившегося ref без соответствующего inbound delivery создаёт `forge.git.push.reconciled.v1`;
- применяет ту же pipeline policy;
- никогда не создаёт дубликат при уже обработанном `(repository_id, ref, new_sha)`;
- сохраняет отчёт reconciliation и количество найденных расхождений.

В первом релизе reconciliation выполняется только для локальных Forge bare repositories. Внешние Git providers требуют отдельного адаптера и явного API-контракта.

---

## 8. Исходящие webhook-ы

### 8.1 Подписки

Целевая подписка:

```json
{
  "id": "uuid",
  "project_id": "uuid",
  "url": "https://receiver.example/hooks/forge",
  "event_types": [
    "forge.pipeline.finished.v1",
    "forge.deployment.finished.v1"
  ],
  "enabled": true,
  "secret_id": "uuid",
  "secret_version": 2,
  "timeout_seconds": 10,
  "max_attempts": 8,
  "created_at": "2026-08-26T12:00:00Z",
  "updated_at": "2026-08-26T12:00:00Z"
}
```

`event_types = []` означает не «все события», а ошибку валидации. Wildcard-подписка допускается только как явный `forge.*` и только для роли administrator.

### 8.2 HTTP-контракт

Webhook отправляется как `POST` с точным сериализованным JSON-body. Подписывается именно набор байтов фактического body.

Обязательные заголовки:

```text
Content-Type: application/json
User-Agent: Forge-CI-CD-Webhooks/1
X-Forge-Event: forge.pipeline.finished.v1
X-Forge-Event-Id: <event UUID>
X-Forge-Delivery-Id: <delivery UUID>
X-Forge-Timestamp: 2026-08-26T12:34:56Z
X-Forge-Signature-256: sha256=<hex HMAC-SHA-256>
X-Forge-Secret-Version: 2
```

Строка для подписи:

```text
v1.<timestamp>.<raw-body>
```

Получатель обязан:

1. проверять freshness timestamp;
2. вычислять HMAC-SHA-256 от исходного body;
3. использовать constant-time comparison;
4. дедуплицировать по `X-Forge-Delivery-Id`, а при необходимости и по `X-Forge-Event-Id`;
5. вернуть любой `2xx` только после устойчивого принятия события.

### 8.3 Retry policy

| Результат | Действие |
|---|---|
| `2xx` | Delivery завершён успешно. |
| `408`, `429`, `5xx`, DNS/connect/read timeout | Retry до лимита. Для `429` учитывается разумный `Retry-After`. |
| `3xx` | Не следовать redirect; terminal failed с причиной `unexpected_redirect`. |
| Большинство `4xx` | Terminal failed без retry. |
| Ошибка сериализации/конфигурации | Terminal failed, требуется действие оператора. |

Backoff: экспоненциальный с полным jitter, например базовая задержка 15 секунд, максимум 1 час, максимум 8 попыток. Конкретные лимиты задаются `CICD_WEBHOOK_*` и фиксируются в delivery snapshot при создании, чтобы изменение настройки не меняло уже существующие попытки.

### 8.4 История доставок и replay

Delivery имеет статусы:

```text
pending -> leased -> delivering -> delivered
                             └-> retry_scheduled
                             └-> failed
                             └-> canceled
```

Для каждой попытки сохраняются:

- номер попытки;
- start/finish time;
- HTTP status;
- классификация ошибки;
- network error code;
- response headers allowlist;
- усечённый response body;
- duration;
- request id/correlation id.

Не сохраняются:

- HMAC secret;
- `Authorization`, cookie и произвольные request headers;
- полный body ответа без ограничения;
- секретные значения event payload.

`Replay` создаёт новую delivery с новым `delivery_id`, но с тем же `event_id`; это сохраняет идемпотентность у получателя и не переписывает исходную историю.

### 8.5 Защита от SSRF

При создании и перед каждой доставкой:

- разрешены только `https://` в production; `http://` — только явно включённый local-development режим;
- запрещены URL с userinfo, непредсказуемыми схемами и redirect;
- DNS-resolved IP проверяется против loopback, link-local, private, multicast и metadata ranges, если destination не находится в явном allowlist;
- hostname повторно резолвится непосредственно перед соединением;
- лимитируются DNS lookup, connect, TLS handshake, body size и общий request timeout;
- egress выполняется через выделенный сетевой policy/прокси в production.

---

## 9. Уведомления

### 9.1 Модель

Уведомление — это не webhook с другой строкой URL. Оно строится из четырёх сущностей:

1. **Notification rule** — какие события интересуют проект.
2. **Destination** — куда отправлять: email, Slack, Telegram, generic webhook или будущий provider.
3. **Preference** — персональная или командная настройка важности, тишины и группировки.
4. **Template** — версионированный шаблон для канала и locale.

Поддерживаемые начальные каналы:

- `email` через SMTP;
- `slack_webhook`;
- `generic_webhook`, использующий общий delivery subsystem.

Telegram, Microsoft Teams, PagerDuty и другие добавляются как adapters без изменения outbox-протокола.

### 9.2 Правила и предпочтения

Пример rule:

```json
{
  "id": "uuid",
  "project_id": "uuid",
  "event_types": [
    "forge.pipeline.finished.v1",
    "forge.deployment.finished.v1"
  ],
  "minimum_severity": "warning",
  "only_terminal_statuses": true,
  "destination_ids": ["uuid"],
  "template_key": "pipeline-terminal",
  "enabled": true
}
```

Пример preference:

```json
{
  "destination_id": "uuid",
  "locale": "ru",
  "quiet_hours": {
    "timezone": "Europe/Moscow",
    "from": "22:00",
    "to": "08:00"
  },
  "digest_mode": "immediate",
  "mute_success": true
}
```

`mute_success` не влияет на критические события безопасности, на failed deployment в production и на явные manual notifications.

### 9.3 Шаблоны

Шаблоны хранятся как версионированный кодовый каталог в первом релизе, а не как произвольный пользовательский HTML. Это уменьшает риск XSS, SSRF-ссылок, template injection и несовместимости после обновления.

Контекст шаблона содержит только allowlist:

```text
project.name
pipeline.id
pipeline.status
pipeline.url
pipeline.git_ref
pipeline.commit_sha
deployment.environment
event.occurred_at
```

Уведомления в первом релизе используют `ru` и `en`, с fallback `ru -> en`. Каждая delivery хранит `template_key`, `template_version`, locale и уже отрендеренный безопасный payload для воспроизводимой истории.

### 9.4 Дедупликация и агрегация

Чтобы серия job events не превращалась в спам:

- пользователю по умолчанию отправляется один terminal pipeline notification;
- ключ дедупликации: `(destination_id, event_type family, pipeline_id, terminal status)`;
- окно агрегации для non-terminal событий — configurable, по умолчанию 60 секунд;
- retry одной delivery не создаёт новую notification delivery;
- события `pipeline.failed` и `deployment.failed` не должны быть потеряны из-за mute success/digest.

---

## 10. Асинхронная модель workers

### 10.1 Типы workers

| Worker | Ответственность |
|---|---|
| `scheduler` | Вычисляет due cron slots, создаёт schedule fire events. |
| `outbox-dispatcher` | Забирает outbox, создаёт fan-out deliveries и задачи pipeline/notification. |
| `pipeline-trigger` | Обрабатывает schedule/Git/manual trigger events, фиксирует SHA и создаёт pipeline. |
| `webhook-delivery` | Выполняет HTTP webhook requests и управляет retry. |
| `notification-delivery` | Рендерит и передаёт уведомления адаптерам каналов. |
| `git-reconciler` | Сверяет refs и восстанавливает пропущенные Git события. |
| `automation-reconciler` | Находит истёкшие lease, зависшие deliveries, orphan records и stale next-run. |
| `retention` | Очищает данные по политике хранения без удаления нужного аудита. |

### 10.2 Получение задач

Базовый паттерн для любой очереди:

```sql
WITH candidates AS (
  SELECT id
  FROM outbox_messages
  WHERE status IN ('pending', 'retry_scheduled')
    AND available_at <= now()
    AND (locked_until IS NULL OR locked_until < now())
  ORDER BY available_at, created_at
  FOR UPDATE SKIP LOCKED
  LIMIT $1
)
UPDATE outbox_messages m
SET status = 'leased',
    locked_by = $2,
    locked_until = now() + $3::interval,
    attempts = m.attempts + 1
FROM candidates
WHERE m.id = candidates.id
RETURNING m.*;
```

Правила:

- lease короткий и продлевается только во время реально долгой операции;
- сетевой вызов выполняется после commit lease-транзакции;
- финальная фиксация использует compare-and-set по `id`, `locked_by`, `status`;
- worker не удерживает открытое SQL-соединение во время внешнего I/O;
- `SKIP LOCKED` позволяет горизонтально масштабировать consumers;
- истёкший lease считается временной ошибкой, а не успехом.

### 10.3 Конкурентность и shutdown

Каждый worker имеет:

- глобальный `max_concurrency`;
- per-project и per-destination limit;
- deadline на одну операцию;
- bounded batch size;
- graceful shutdown: прекратить claim новых задач, дождаться текущих в пределах deadline, не удалять lease искусственно.

Worker instances должны иметь уникальный `worker_id`, включающий роль, hostname/pod и UUID запуска.

---

## 11. Целевой каталог событий

| Событие | Источник | Потребители | Назначение |
|---|---|---|---|
| `forge.git.push.received.v1` | Git ingress | pipeline-trigger, notification/audit | Принят push с конкретными old/new SHA. |
| `forge.git.push.reconciled.v1` | git-reconciler | pipeline-trigger, audit | Восстановлен пропущенный Git event. |
| `forge.schedule.triggered.v1` | scheduler | pipeline-trigger, audit | Наступил уникальный schedule slot. |
| `forge.schedule.skipped.v1` | scheduler | audit, notification при критичности | Fire пропущен из-за policy/concurrency. |
| `forge.schedule.fire_skipped_dst.v1` | scheduler | audit | Локальное время не существовало из-за DST. |
| `forge.pipeline.queued.v1` | pipeline service | runner dispatcher, webhook, notification | Pipeline устойчиво создан. |
| `forge.pipeline.started.v1` | runner/pipeline service | webhook, notification | Pipeline начал выполнение. |
| `forge.pipeline.finished.v1` | runner/pipeline service | webhook, notification, reports | Pipeline достиг terminal state. |
| `forge.job.started.v1` | runner | webhook при подписке | Job начал выполнение. |
| `forge.job.finished.v1` | runner | webhook при подписке | Job достиг terminal state. |
| `forge.deployment.finished.v1` | deployment service | webhook, notification | Deployment завершён. |
| `forge.webhook.delivered.v1` | webhook worker | audit/metrics | Webhook успешно доставлен. |
| `forge.webhook.failed.v1` | webhook worker | notification/audit | Попытки исчерпаны или конфигурация ошибочна. |
| `forge.notification.delivered.v1` | notification worker | audit/metrics | Уведомление принято каналом. |
| `forge.notification.failed.v1` | notification worker | audit | Уведомление не доставлено. |
| `forge.automation.reconciliation_found.v1` | reconciler | audit/alerting | Найдена и исправляется несогласованность. |

Внешне доступны только документированные domain events. Технические события delivery не должны по умолчанию вновь запускать webhook delivery, чтобы избежать циклов.

---

## 12. Целевая дата-модель

### 12.1 События и очередь

#### `domain_events`

| Поле | Тип | Назначение |
|---|---|---|
| `id` | UUID PK | Идентификатор события. |
| `type` | TEXT | Версионированный тип события. |
| `schema_version` | SMALLINT | Версия payload schema. |
| `project_id` | UUID NULL FK | Проект, если применимо. |
| `aggregate_type` | TEXT | Тип агрегата. |
| `aggregate_id` | UUID NULL | Идентификатор агрегата. |
| `correlation_id` | UUID | Сквозной сценарий. |
| `causation_id` | UUID NULL | Непосредственная причина. |
| `actor_type`, `actor_id` | TEXT | Инициатор. |
| `payload` | JSONB | Валидный безопасный event envelope/data. |
| `occurred_at` | TIMESTAMPTZ | Время факта. |
| `created_at` | TIMESTAMPTZ | Время записи. |

Индексы: `(project_id, occurred_at DESC)`, `(type, occurred_at DESC)`, `(correlation_id)`.

#### `outbox_messages`

| Поле | Тип | Назначение |
|---|---|---|
| `id` | UUID PK | Идентификатор сообщения очереди. |
| `event_id` | UUID FK UNIQUE | Исходное событие. |
| `topic` | TEXT | Например, `automation.dispatch`. |
| `status` | TEXT | `pending`, `leased`, `published`, `retry_scheduled`, `failed`. |
| `available_at` | TIMESTAMPTZ | Не раньше этого времени брать задачу. |
| `attempts` | INTEGER | Количество claim/publish попыток. |
| `locked_by`, `locked_until` | TEXT/TIMESTAMPTZ | Lease. |
| `last_error_code`, `last_error_message` | TEXT | Диагностика, без секретов. |
| `created_at`, `published_at` | TIMESTAMPTZ | Lifecycle. |

### 12.2 Расписания

` schedules` расширяется полями:

```text
timezone TEXT NOT NULL DEFAULT 'UTC'
misfire_policy TEXT NOT NULL DEFAULT 'fire_once'
misfire_grace_seconds INTEGER NOT NULL DEFAULT 900
concurrency_policy TEXT NOT NULL DEFAULT 'forbid'
next_fire_at TIMESTAMPTZ
last_fire_at TIMESTAMPTZ
updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
version INTEGER NOT NULL DEFAULT 1
```

`schedule_fires`:

```text
id UUID PRIMARY KEY
schedule_id UUID NOT NULL REFERENCES schedules(id) ON DELETE CASCADE
scheduled_for TIMESTAMPTZ NOT NULL
fired_at TIMESTAMPTZ
status TEXT NOT NULL
pipeline_id UUID NULL REFERENCES pipelines(id) ON DELETE SET NULL
event_id UUID UNIQUE REFERENCES domain_events(id)
reason TEXT NULL
created_at TIMESTAMPTZ NOT NULL DEFAULT now()
UNIQUE(schedule_id, scheduled_for)
```

### 12.3 Git ingress

`project_repositories`:

```text
id UUID PRIMARY KEY
project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE
repository_id UUID NOT NULL REFERENCES repositories(id) ON DELETE CASCADE
trigger_policy JSONB NOT NULL
enabled BOOLEAN NOT NULL DEFAULT TRUE
created_at TIMESTAMPTZ NOT NULL DEFAULT now()
updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
UNIQUE(project_id, repository_id)
```

`inbound_deliveries`:

```text
id UUID PRIMARY KEY
source TEXT NOT NULL
external_delivery_id TEXT NOT NULL
event_type TEXT NOT NULL
repository_id UUID NULL REFERENCES repositories(id) ON DELETE SET NULL
payload JSONB NOT NULL
payload_sha256 TEXT NOT NULL
signature_version TEXT
received_at TIMESTAMPTZ NOT NULL
status TEXT NOT NULL
event_id UUID NULL UNIQUE REFERENCES domain_events(id)
error_code TEXT
created_at TIMESTAMPTZ NOT NULL DEFAULT now()
UNIQUE(source, external_delivery_id)
```

`repository_refs` хранит последний подтверждённый SHA для reconciliation:

```text
repository_id UUID NOT NULL
ref_name TEXT NOT NULL
object_sha TEXT NOT NULL
observed_at TIMESTAMPTZ NOT NULL
PRIMARY KEY(repository_id, ref_name)
```

### 12.4 Pipeline provenance

`pipelines` получает:

```text
trigger_type TEXT NOT NULL DEFAULT 'manual'
trigger_event_id UUID NULL UNIQUE REFERENCES domain_events(id)
requested_ref TEXT
resolved_sha TEXT
repository_id UUID NULL REFERENCES repositories(id) ON DELETE SET NULL
correlation_id UUID
actor_type TEXT
actor_id TEXT
```

`git_ref` сохраняется для обратной совместимости UI, но в новых сценариях отражает `requested_ref`; фактическим источником выполнения является `resolved_sha`.

### 12.5 Webhook и notification delivery

`webhook_subscriptions` заменяет существующую минимальную таблицу `webhooks` либо мигрирует её данные:

```text
id UUID PRIMARY KEY
project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE
url TEXT NOT NULL
event_types TEXT[] NOT NULL
enabled BOOLEAN NOT NULL DEFAULT TRUE
secret_id UUID NULL REFERENCES integration_secrets(id)
secret_version INTEGER
timeout_seconds INTEGER NOT NULL DEFAULT 10
max_attempts INTEGER NOT NULL DEFAULT 8
created_at TIMESTAMPTZ NOT NULL DEFAULT now()
updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
```

`integration_secrets` хранит только зашифрованные значения через существующий контур AES-256-GCM; секрет не возвращается API после создания.

`outbound_deliveries` — общая очередь отправки:

```text
id UUID PRIMARY KEY
kind TEXT NOT NULL -- webhook | notification
event_id UUID NOT NULL REFERENCES domain_events(id)
project_id UUID NULL REFERENCES projects(id) ON DELETE CASCADE
subscription_id UUID NULL
destination_id UUID NULL
status TEXT NOT NULL
dedupe_key TEXT NULL
payload JSONB NOT NULL
payload_sha256 TEXT NOT NULL
next_attempt_at TIMESTAMPTZ NOT NULL
attempt_count INTEGER NOT NULL DEFAULT 0
max_attempts INTEGER NOT NULL
locked_by TEXT
locked_until TIMESTAMPTZ
last_error_code TEXT
last_error_message TEXT
delivered_at TIMESTAMPTZ
created_at TIMESTAMPTZ NOT NULL DEFAULT now()
UNIQUE(subscription_id, event_id)
```

`outbound_delivery_attempts`:

```text
id BIGSERIAL PRIMARY KEY
delivery_id UUID NOT NULL REFERENCES outbound_deliveries(id) ON DELETE CASCADE
attempt_number INTEGER NOT NULL
started_at TIMESTAMPTZ NOT NULL
finished_at TIMESTAMPTZ
outcome TEXT NOT NULL
http_status INTEGER
response_headers JSONB
response_body_preview TEXT
error_code TEXT
error_message TEXT
duration_ms INTEGER
UNIQUE(delivery_id, attempt_number)
```

### 12.6 Reconciliation и retention

`reconciliation_runs` фиксирует тип проверки, границы сканирования, состояние, найденные/исправленные записи и ошибку. Это необходимо, чтобы «автоматически исправлено» не становилось невидимым побочным эффектом.

Сроки хранения:

- `domain_events`, `schedule_fires`, terminal deliveries и audit — минимум 180 дней, configurable;
- preview HTTP-body — существенно короче, например 30 дней;
- payload с повышенной чувствительностью не сохраняется либо хранится отдельно зашифрованным;
- удаление выполняется worker-ом батчами и фиксируется в audit.

---

## 13. Целевые API

Все новые API требуют authentication/RBAC. До включения автоматизации в shared environment нельзя считать unauthenticated текущие endpoints достаточными.

### 13.1 Расписания

| Метод | Путь | Назначение |
|---|---|---|
| `GET` | `/projects/{project_id}/schedules` | Список с `next_fire_at`, timezone и состоянием. |
| `POST` | `/projects/{project_id}/schedules` | Создать расписание. |
| `GET` | `/schedules/{schedule_id}` | Детали и последние fires. |
| `PATCH` | `/schedules/{schedule_id}` | Частичное обновление с optimistic version. |
| `POST` | `/schedules/{schedule_id}/pause` | Отключить без удаления истории. |
| `POST` | `/schedules/{schedule_id}/run` | Ручной запуск с отдельной причиной `manual_schedule`. |
| `GET` | `/schedules/{schedule_id}/fires` | Paginated история срабатываний. |

`POST/PATCH` валидирует cron, IANA timezone, ограничение `misfire_policy` и `concurrency_policy`. Ответ содержит вычисленный ближайший запуск.

### 13.2 Git integrations

| Метод | Путь | Назначение |
|---|---|---|
| `POST` | `/internal/git-events/push` | Внутренний подписанный Git ingress. |
| `GET` | `/projects/{project_id}/repositories` | Связанные репозитории и trigger policy. |
| `POST` | `/projects/{project_id}/repositories` | Привязать repository к проекту. |
| `PATCH` | `/project-repositories/{id}` | Изменить ref policy/debounce/enabled. |
| `POST` | `/repositories/{id}/reconcile` | Запустить ограниченную ручную сверку; только administrator. |
| `GET` | `/repositories/{id}/inbound-deliveries` | История Git ingress без секретов. |

### 13.3 Webhooks

| Метод | Путь | Назначение |
|---|---|---|
| `GET` | `/projects/{project_id}/webhooks` | Подписки без секретов. |
| `POST` | `/projects/{project_id}/webhooks` | Создать подписку и один раз показать generated secret. |
| `PATCH` | `/webhooks/{webhook_id}` | Изменить безопасные поля и enabled. |
| `POST` | `/webhooks/{webhook_id}/rotate-secret` | Ротация с новой версией секрета. |
| `DELETE` | `/webhooks/{webhook_id}` | Мягкое отключение/удаление по retention policy. |
| `GET` | `/webhooks/{webhook_id}/deliveries` | Paginated delivery history. |
| `GET` | `/webhook-deliveries/{delivery_id}` | Детали и attempts с redaction. |
| `POST` | `/webhook-deliveries/{delivery_id}/replay` | Создать новую delivery для прежнего event. |
| `POST` | `/webhooks/{webhook_id}/test` | Тестовый `forge.webhook.test.v1`, не имитирующий production event. |

### 13.4 Notifications

| Метод | Путь | Назначение |
|---|---|---|
| `GET/POST` | `/projects/{project_id}/notification-rules` | Список/создание rules. |
| `PATCH/DELETE` | `/notification-rules/{id}` | Изменение/удаление rule. |
| `GET/POST` | `/projects/{project_id}/notification-destinations` | Получатели и каналы. |
| `PATCH/DELETE` | `/notification-destinations/{id}` | Управление destination. |
| `GET/PATCH` | `/notification-destinations/{id}/preferences` | Locale, quiet hours, digest. |
| `GET` | `/projects/{project_id}/notifications/deliveries` | История отправок. |
| `POST` | `/notification-destinations/{id}/test` | Тестовое уведомление. |

### 13.5 Operational APIs

| Метод | Путь | Назначение |
|---|---|---|
| `GET` | `/automation/health` | Readiness workers, backlog и DB connectivity. |
| `GET` | `/automation/queue` | Aggregate pending/retry/failed по типам; admin only. |
| `GET` | `/automation/dead-letters` | Terminal automation failures. |
| `POST` | `/automation/dead-letters/{id}/requeue` | Осознанный повтор оператором. |
| `GET` | `/events` | Фильтруемый event/audit stream с pagination. |

Списки обязательно имеют cursor pagination, фиксированный sort order и ограничение размера страницы.

---

## 14. Сбои, восстановление и reconciliation

| Сценарий | Поведение |
|---|---|
| API упал после commit inbound delivery | Повтор Git hook возвращает существующий delivery; dispatcher обработает pending outbox после restart. |
| API упал до commit | Hook получает ошибку; push не блокируется, reconciliation обнаруживает расхождение refs. |
| Scheduler запущен на двух репликах | Уникальность `(schedule_id, scheduled_for)` исключает двойной fire. |
| Worker упал после lease, до HTTP-вызова | Lease истекает; другой worker повторяет задачу. |
| Worker упал после HTTP-вызова, до commit результата | Delivery повторится; получатель дедуплицирует по delivery id. |
| Webhook endpoint постоянно отвечает `500` | Exponential retry, затем `failed`; alert и ручной replay. |
| Webhook возвращает `401/403/404` | Terminal failure без automatic retry; оператор исправляет secret/URL и запускает replay. |
| SMTP/provider недоступен | Retry по policy адаптера; terminal failure виден в notification history. |
| Изменён cron/timezone | Новое `next_fire_at` рассчитывается в транзакции; уже созданные fires не переписываются. |
| Удалён проект | Новые deliveries не создаются; активные deliveries переводятся в `canceled` с audit event. История хранится до retention. |
| Ref удалён | Событие сохраняется, pipeline не создаётся по default policy. |
| Изменена ветка после push | Pipeline использует сохранённый `resolved_sha`, а не текущий head. |
| БД недоступна | Readiness возвращает false; workers прекращают claim новых задач. Неуспешные попытки не маркируются success. |

Reconciliation jobs:

1. **Lease recovery** — периодически ищет `locked_until < now()` и возвращает задачи в retry.
2. **Outbox lag recovery** — обнаруживает domain events без published outbox или pending outbox старше SLO.
3. **Schedule repair** — пересчитывает `next_fire_at` для enabled schedules с NULL/stale значением.
4. **Git ref reconciliation** — сравнивает bare refs и обработанные SHA.
5. **Delivery consistency** — проверяет terminal deliveries без terminal attempt и attempts без родительской delivery.
6. **Pipeline provenance repair** — не исправляет SHA автоматически; только создаёт alert для pipeline без ожидаемого trigger metadata.

Ни один reconciliation worker не должен silently удалять данные или порождать pipeline без явного event/audit trail.

---

## 15. Безопасность и доступ

### 15.1 RBAC

Минимальные разрешения:

| Действие | Роль |
|---|---|
| Просмотр расписаний, deliveries и истории | `viewer` с доступом к проекту. |
| Создание/изменение schedules и notification rules | `developer` или выше. |
| Создание webhook destination и тестовая отправка | `maintainer` или выше. |
| Ротация webhook secret, replay terminal failure | `maintainer` или выше. |
| Просмотр payload preview, dead letters, запуск reconciliation | `admin`. |
| Изменение egress allowlist и retention policy | `admin`. |

### 15.2 Секреты

- webhook secrets и SMTP/provider credentials шифруются AES-256-GCM через выделенный secret abstraction;
- API возвращает сгенерированный webhook secret только один раз;
- ротация создаёт новую `secret_version`; старую можно принимать ограниченное transition window, если это явно включено;
- ни webhook secret, ни внутренний Git hook secret не записываются в audit, event payload, error message или response preview;
- internal Git hook не должен содержать постоянный секрет в файле, доступном репозиторным пользователям.

### 15.3 Payload и журналирование

- payload event проходит schema validation и size limit;
- text fields экранируются в UI;
- response preview ограничен по размеру и санитизируется;
- логируются id, тип, статус, попытка, duration и error class, но не request body с секретами;
- access к delivery history и inbound payload аудитируется.

---

## 16. Наблюдаемость

### 16.1 Health и readiness

Разделить endpoints:

- `/api/v1/health` — liveness процесса;
- `/api/v1/readiness` — доступность PostgreSQL, миграции, способность worker-ов получать lease;
- `/api/v1/automation/health` — статус scheduler/outbox/workers, максимальный queue lag и age oldest pending message.

Readiness не должна быть `200`, если миграции не применены или outbox worker критически отстал сверх установленного порога.

### 16.2 Метрики

Минимальный Prometheus/OpenTelemetry набор:

```text
forge_automation_queue_depth{kind,status}
forge_automation_oldest_pending_seconds{kind}
forge_automation_worker_claim_total{worker,result}
forge_automation_worker_lease_expired_total{worker}
forge_schedule_due_total{result}
forge_schedule_lag_seconds
forge_git_inbound_total{result,ref_kind}
forge_git_reconciliation_total{result}
forge_outbox_dispatch_total{result,topic}
forge_webhook_delivery_total{result,event_type}
forge_webhook_attempt_total{result,http_status}
forge_webhook_duration_seconds
forge_notification_delivery_total{channel,result}
forge_notification_duration_seconds{channel}
forge_dead_letter_total{kind}
forge_pipeline_trigger_total{trigger_type,result}
```

Cardinality ограничивается: нельзя добавлять в labels project ID, URL, user ID, SHA или delivery UUID.

### 16.3 Трассировка и логи

Каждый request и worker span содержит:

```text
trace_id
correlation_id
causation_id
event_id
delivery_id
attempt_number
project_id
pipeline_id
schedule_id
repository_id
worker_id
```

HTTP webhook span фиксирует только hostname и status class, не полный URL с query parameters.

### 16.4 Алерты и SLO

Первичные алерты:

- oldest pending outbox message старше 5 минут;
- scheduler lag старше двух poll intervals;
- доля terminal webhook failures выше базового порога за 15 минут;
- dead letters появились;
- Git reconciliation обнаружил необработанные ref изменения;
- delivery retry rate резко вырос;
- PostgreSQL lease recovery стабильно находит зависшие задачи;
- readiness false.

Рекомендуемые начальные SLO:

- 99% due schedule fires создают event не позднее 60 секунд после due time;
- 99% успешно доступных webhook endpoints получают первую попытку не позднее 60 секунд после domain event;
- zero unobserved terminal delivery failures;
- Git ingress reconciliation устраняет пропущенное локальное событие не позднее 10 минут.

---

## 17. Тестовая стратегия

### 17.1 Unit tests

- cron parsing, timezone validation и `next_fire_at`;
- DST ambiguous/nonexistent time;
- misfire и concurrency policies;
- deterministic idempotency keys;
- event envelope/schema validation;
- HMAC test vectors и constant-time comparison;
- классификация HTTP retry/non-retry;
- backoff + jitter в границах допустимого диапазона;
- secret redaction;
- шаблоны уведомлений и locale fallback;
- URL/SSRF validation.

### 17.2 Repository and transaction tests

На реальном PostgreSQL test database:

- одновременный schedule claim двумя worker-ами создаёт один `schedule_fire`;
- повтор inbound Git delivery возвращает тот же logical result;
- pipeline `trigger_event_id` не допускает дубликат;
- изменение pipeline/job и outbox event коммитятся либо откатываются вместе;
- lease невозможно захватить двумя consumer-ами;
- expired lease корректно восстанавливается;
- `ON CONFLICT` не теряет первоначальный event payload;
- project deletion корректно отменяет pending deliveries согласно policy.

### 17.3 Integration tests

С локальным HTTP receiver и SMTP fake server:

- webhook headers и HMAC совпадают с raw body;
- `500 -> retry -> 200` создаёт корректную историю attempts;
- `429 Retry-After` влияет на `next_attempt_at`;
- `404` становится terminal failure без retry;
- timeout после фактического принятия receiver-ом создаёт повтор с тем же delivery id;
- receiver dedupe доказывает безопасность повторной доставки;
- Slack/email adapter рендерит шаблон и не раскрывает secret;
- post-receive payload с branch/tag/delete ref обрабатывается по policy;
- reconciliation создаёт событие только для отсутствующего `(repo, ref, SHA)`.

### 17.4 End-to-end и UI tests

- создание schedule отображает timezone и ближайший запуск;
- pause/resume сохраняет состояние после refresh;
- webhook secret показывается один раз;
- delivery history показывает success/retry/failed и redacted diagnostics;
- replay создаёт новую delivery, не меняя исходную;
- notification preferences применяются к отправке;
- Git push создаёт pipeline с отображением branch и immutable SHA;
- все страницы проверяются на 375, 1920 и 2560 px.

### 17.5 Failure-injection tests

- restart worker между claim, HTTP request и финальным update;
- временная недоступность PostgreSQL;
- DNS timeout, TLS failure, slow receiver;
- перестановка/дублирование `LISTEN/NOTIFY`;
- два scheduler instances;
- clock skew между API и worker;
- миграция существующих schedules/webhooks/notifications без потери конфигурации.

---

## 18. План внедрения

### Этап 0. Подготовка фундамента

1. Ввести версионированные SQL migrations вместо расширения только через `CREATE TABLE IF NOT EXISTS`.
2. Добавить authentication/RBAC middleware для mutation и operational APIs.
3. Добавить `domain_events`, `outbox_messages`, correlation ids и базовый worker lease abstraction.
4. Включить метрики, readiness и структурированные logs.
5. Не включать внешние отправки.

Критерий готовности: изменение pipeline создаёт событие и outbox atomically; после restart dispatcher не теряет pending message.

### Этап 1. Расписания

1. Мигрировать текущие `schedules`: присвоить `timezone = UTC`, `fire_once`, `forbid`.
2. Ввести строгую cron/IANA validation и расчёт `next_fire_at`.
3. Реализовать `schedule_fires`, scheduler worker и audit/events.
4. Включить feature flag `CICD_AUTOMATION_SCHEDULES_ENABLED=false` по default.
5. Pilot для одного внутреннего проекта с read-only dry-run mode, затем real pipelines.

Критерий готовности: нет дубликатов при двух scheduler replicas; DST и misfire покрыты integration tests.

### Этап 2. Надёжный Git ingress

1. Создать `project_repositories`, `inbound_deliveries`, `repository_refs`.
2. Сохранить legacy endpoint на переходный период, но не использовать его для новых hooks.
3. Выпустить новую версию generated `post-receive` hook с delivery id, SHA и подписью.
4. Добавить pipeline provenance и запуск по `resolved_sha`.
5. Включить Git reconciliation сначала в report-only mode, затем в repair mode.

Критерий готовности: повтор hook request не создаёт второй pipeline; отключение API во время push восстанавливается reconciliation.

### Этап 3. Outbound webhooks

1. Мигрировать `webhooks` в `webhook_subscriptions`.
2. Добавить encrypted secrets, HMAC, URL validation, delivery/attempt tables.
3. Реализовать тестовую доставку и history UI.
4. Включить только allowlisted destinations для pilot.
5. Включить replay и dead-letter handling после подтверждения наблюдаемости.

Критерий готовности: endpoint receiver успешно валидирует подпись, retry и duplicate delivery.

### Этап 4. Notifications

1. Добавить destinations, rules, preferences и template catalog.
2. Включить email fake/test adapter, затем Slack webhook.
3. Ввести aggregation и quiet hours.
4. Запустить notifications для terminal pipeline events одного проекта.
5. Добавить production alerting на failed notification destinations.

Критерий готовности: muted success не скрывает failed deployment; template rendering воспроизводим по delivery history.

### Этап 5. Масштабирование и эксплуатация

1. Вынести workers в отдельные deployment-ы.
2. Настроить per-destination concurrency и egress controls.
3. Включить retention worker и backup/restore tests.
4. Утвердить runbook: requeue, replay, secret rotation, reconciliation и incident response.
5. Удалить legacy Git trigger endpoint и configuration-only API поведение после migration window.

---

## 19. Обратная совместимость и миграция данных

- Существующие schedule records получают `timezone = 'UTC'`; это явно отображается в UI, чтобы владелец мог изменить timezone.
- Существующие `webhooks.url/events/enabled` мигрируют в subscriptions без секрета и остаются disabled до явного создания/ротации секрета.
- Существующие `notification_configs` конвертируются в destinations с выключенными rules; нельзя автоматически начать отправку на старые targets.
- Существующие pipelines получают `trigger_type = 'legacy'`, `resolved_sha = NULL`; история не подделывается.
- Старый endpoint `/internal/git-push` остаётся доступен ограниченное время и записывает событие типа `forge.git.push.legacy_received.v1`, но не должен быть основным путём.
- Каждая миграция проходит backup/restore rehearsal на production-like snapshot.
- Rollback кода не должен требовать удаления новых таблиц или колонок; schema changes выполняются expand/contract-подходом.

---

## 20. Критерии принятия целевой архитектуры

Автоматизация считается реализованной не после появления CRUD-экранов, а только когда подтверждены следующие свойства:

1. Запланированный запуск не дублируется при двух scheduler instances и переживает restart.
2. Git push с одним delivery id создаёт не более одного pipeline.
3. Pipeline, вызванный Git push, содержит исходный ref и конкретный immutable commit SHA.
4. Изменение доменного состояния и outbox event атомарны.
5. Webhook имеет HMAC-подпись, уникальный delivery id, историю попыток и контролируемый retry.
6. Timeout после фактического приёма webhook receiver-ом не приводит к необнаружимому или опасному дубликату.
7. Notification delivery имеет template version, channel, preferences и redacted историю.
8. Любой terminal failure виден через API/UI, логи, метрики и alerting.
9. Reconciliation способен обнаружить и восстановить пропущенный Git ingress без создания дубликата.
10. Полный набор unit, PostgreSQL integration, failure-injection и E2E-тестов проходит в CI.
