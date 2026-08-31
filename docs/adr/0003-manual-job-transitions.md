# ADR-0003: Ручные переходы статусов job в MVP

## Status

Accepted as historical MVP baseline; partially superseded by current embedded runner execution, execution attempts, and ADR-0007 target runner boundary.

## Context

Полноценное выполнение CI-задач требует runner-ов, их регистрации и аутентификации, очереди, lease/heartbeat, sandboxing, работы с Git и secrets, потоковых логов, артефактов, retries и восстановления после отказов. На момент принятия ADR эта инфраструктура была существенно больше, чем базовая ценность раннего MVP: проверить модель pipeline/stage/job, правила переходов и Dashboard.

## Current Status

Current baseline уже не manual-only: embedded runner в `cicd-server` выполняет jobs в Docker или host shell, пишет stdout/stderr в attempt-owned `job_logs`, поддерживает cancel/retry через `execution_attempts` и inject/mask project secrets. Ручный status API остаётся диагностическим и legacy-compatible механизмом, но не является основным исполнителем job. Target-граница external runner с lease/fencing описана в ADR-0007 и `docs/RUNNER_ARCHITECTURE.md`.

## Alternatives Considered

| Option | Pros | Cons |
|---|---|---|
| Сразу реализовать локальный Docker runner | Демонстрирует реальное выполнение | Высокий риск безопасности, большой объём инфраструктуры и слабая проверяемость control plane |
| Использовать GitHub Actions как executor | Быстрое выполнение в облаке | Не является self-hosted execution, связывает модель с внешним провайдером и не проверяет runner protocol |
| Ручные переходы через API/UI/CLI | Малый объём, явная проверка state machine и агрегирования статусов | Не выполняет команды, не даёт real-time output и не проверяет dispatch |

## Decision

В раннем MVP job переводились вручную через API, CLI и Dashboard. Разрешённые переходы определяет `JobStatus::transition_to()` в domain-слое: terminal job нельзя перезапустить, а недопустимый переход отклоняется. После изменения job backend агрегирует состояния вверх от job к stage и pipeline. Логи job сохраняются append-only через публичный API.

## Consequences

- Можно проверить API contract, модель данных, UX и правила жизненного цикла без запуска недоверенного кода.
- Система честно позиционируется как control plane, а не как готовая execution platform; UI и документация не должны создавать обратное впечатление.
- Исторически не было автоматического checkout, выполнения shell-команд, retries, runner health, artifacts или интеграции secrets; current baseline это частично закрыл embedded runner-ом, но без production-safe external runner boundary.
- Все клиенты используют одинаковую доменную валидацию через API, что упрощает дальнейшую замену ручного механизма на dispatcher.

## Migration Path

1. Добавить модели runner, registration token, capabilities, heartbeat и lease без изменения существующей state machine.
2. Current baseline ввёл `execution_attempts` для job; idempotency key и устойчивая очередь в PostgreSQL остаются target.
3. Реализовать защищённый runner protocol, sandboxing и потоковую загрузку append-only логов.
4. Добавить timeout, retry, circuit breaker и reconciliation согласно `docs/RESILIENCE.md`.
5. Подключить S3-совместимое artifact storage, secrets/RBAC и наблюдаемость.
6. Переводить job в terminal state только по подтверждённому результату execution attempt, сохранив существующую агрегацию stage/pipeline.

## Related

- `docs/ARCHITECTURE.md`
- `docs/ROADMAP.md`
- `docs/RESILIENCE.md`
- `docs/adr/0004-postgresql-only.md`
