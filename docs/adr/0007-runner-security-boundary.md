# ADR-0007: Исполнение отделено от control plane

## Status

Accepted (target architecture; implementation pending)

## Context

Текущий embedded runner выполняется в процессе backend. Такой процесс совмещает public API, PostgreSQL credentials и запуск недоверенного user code; Docker execution требует daemon privilege. Реестр `runners` пока не участвует в dispatch/lease.

## Decision

Control-plane API никогда не исполняет пользовательские команды и не получает Docker socket. Исполнение переносится в отдельный runner process/service с registration token, scoped lease token, heartbeat, capabilities/tags и execution attempts. Runner получает только подписанное задание, минимальные credentials и временно дешифрованные project secrets; результат, logs и artifacts возвращает через runner protocol. Docker/Kubernetes/shell являются infra adapters за runner boundary.

## Consequences

- `runner` registry превращается в protocol participant, а не UI inventory.
- Требуются очередь, lease expiry, reconciliation, cancellation signal и sandbox policy.
- Локальный embedded executor остаётся development adapter до готовности отдельного runner service; production docs не должны рекламировать его как безопасный runner pool.

## Related

- `docs/RUNNER_ARCHITECTURE.md`
- `docs/FUNCTIONAL_ARCHITECTURE.md`
- `docs/adr/0003-manual-job-transitions.md`
