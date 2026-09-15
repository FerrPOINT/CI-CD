# Runbook: эксплуатация Forge CI/CD (этап 5)

Статус: обязательный операционный документ (AUTOMATION_ARCHITECTURE §18 этап 5 п.4).
Область: requeue, replay, secret rotation, reconciliation, incident response.

## 1. Роли и масштабирование

| Переменная | Поведение |
|------------|-----------|
| `CICD_ROLES` не задана | Все роли в одном процессе (локальный compose) |
| `CICD_ROLES=api` | Только API + dashboard |
| `CICD_ROLES=worker` | Outbox delivery + scheduler + retention |
| `CICD_ROLES=runner` | Embedded runner (jobs) / maintenance |

Масштабирование: поднимай N `worker`-подов — lease-claim (`FOR UPDATE SKIP
LOCKED` + окно 300 c) гарантирует exactly-once доставку между инстансами
(см. тест `parallel_delivery_claims_message_exactly_once`).

## 2. Requeue застрявшей поставки

Симптом: сообщение висит `delivered_at IS NULL` дольше часа.

1. `SELECT id, attempts, last_error, next_attempt_at FROM outbox_messages WHERE delivered_at IS NULL AND failed_at IS NULL;`
2. Если `next_attempt_at` в будущем из-за сбоя воркера — сбросить lease:
   `UPDATE outbox_messages SET next_attempt_at = now() WHERE id = '<id>';`
3. Lease истекает сам через 300 c; ручной сброс нужен только при массовых сбоях.

## 3. Replay доставленного события

Повторная отправка терминального события:
`POST /api/v1/projects/{id}/notifications/{message_id}/replay`
(создаёт новую строку `replay_of_id → оригинал`, оригинал не мутирует).

## 4. Ротация секретов

- Webhook-секреты: `PUT /api/v1/projects/{id}/webhooks/{wid}` с новым `secret`;
  подписи считаются на момент отправки — старый секрет перестаёт действовать сразу.
- SAT-токены runners: `POST /api/v1/runners/{id}/tokens/rotate`; старый токен
  отзывается атомарно, runner получает 401 и перерегистрируется.
- SMTP-пароль: env `CICD_SMTP_*`, применяется при рестарте worker-роли.
- Egress-allowlist: env `CICD_WEBHOOK_ALLOWLIST` (csv host[:port]); пусто = без
  ограничений (только локальный режим).

## 5. Reconciliation

- Runner offline > ack-timeout: control-plane автоматически переочередит job
  (lease-переходы в `job_leases`), stale-runner помечается offline.
- Dead-letter доставок: `notification_destination_alerts` открывают алерт при
  dead-letter, закрывают при успехе; acknowledge — Developer+.
- При рассинхроне: `GET /api/v1/system/reconciliation` (сводка расхождений
  leases/queue) — раз в смену.

## 6. Incident response

| Сценарий | Действие |
|----------|----------|
| Воркер упал во время доставки | Lease истечёт (300 c), другой воркер заберёт. Действий не требуется |
| Шторм неудачных доставок | Проверить `outbox_delivery_attempts` (outcome/http_status); при сетевом инциденте — bulk `next_attempt_at = now()` после восстановления |
| Заблокированный egress-хост | Сообщение завершится failed с `egress allowlist` в last_error; добавить хост в allowlist и replay |
| Зависший schedule | `UPDATE schedules SET next_fire_at = NULL WHERE id=...` — следующий pass материализует слот |
| Миграция не применилась | Проверить `_migrations`; каждая миграция идемпотентна (IF NOT EXISTS), retry безопасен |

## 7. Retention / backup

- Outbox: delivered-сообщения старше 30 дней удаляет worker (раз в час),
  attempt-history каскадом. Недоставленные/queued не трогаются.
- Артефакты: `artifact_retention_loop` по `CICD_ARTIFACTS_RETENTION_DAYS`.
- Backup: pg_dump по расписанию ОС-уровня; restore-путь = чистая БД + полный
  chain миграций (каждый интеграционный тест проходит его на отдельной схеме).
