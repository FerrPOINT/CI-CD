# ADR-0007: Исполнение отделено от control plane

## Status

Accepted (target architecture; external runner implementation pending; embedded lease ledger partial)

## Context

Текущий embedded runner выполняется в процессе backend. Такой процесс совмещает public API, PostgreSQL credentials и запуск недоверенного user code; Docker execution требует daemon privilege. Embedded execution уже пишет `job_leases` как локальный owner/expiry ledger, но реестр `runners` пока не участвует во внешнем dispatch/lease protocol.

## Decision

Control-plane API никогда не исполняет пользовательские команды и не получает Docker socket. Исполнение переносится в отдельный runner process/service с registration token, scoped lease token, heartbeat, capabilities/tags и execution attempts. Runner получает только подписанное задание, минимальные credentials и временно дешифрованные project secrets; результат, logs и artifacts возвращает через runner protocol. Docker/Kubernetes/shell являются infra adapters за runner boundary.

## Consequences

- `runner` registry превращается в protocol participant, а не UI inventory.
- Требуются очередь, внешний lease token/ack/renew/fencing, cancellation signal и sandbox policy; current embedded `job_leases` закрывает только локальный owner/expiry/reconciliation MVP.
- Локальный embedded executor остаётся development adapter до готовности отдельного runner service; production docs не должны рекламировать его как безопасный runner pool.

## Related

- `docs/RUNNER_ARCHITECTURE.md`
- `docs/FUNCTIONAL_ARCHITECTURE.md`
- `docs/adr/0003-manual-job-transitions.md`
