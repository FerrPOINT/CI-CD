# ADR-0007: Исполнение отделено от control plane

## Status

Accepted (target architecture; external runner protocol + forge-runner shell MVP implemented; production runner boundary pending)

## Context

Текущий embedded runner выполняется в процессе backend. Такой процесс совмещает public API, PostgreSQL credentials и запуск недоверенного user code; Docker execution требует daemon privilege. Embedded execution уже пишет `job_leases` как локальный owner/expiry ledger. External runner protocol MVP подключил `runners` к register/heartbeat/poll/ack/renew/complete и lease fencing; `forge-runner` уже может выполнять shell jobs отдельным процессом, но production sandbox boundary ещё не закрыт.

## Decision

Control-plane API никогда не исполняет пользовательские команды и не получает Docker socket. Исполнение переносится в отдельный runner process/service с registration token, scoped lease token, heartbeat, capabilities/tags и execution attempts. Runner получает только подписанное задание, минимальные credentials и временно дешифрованные project secrets; результат, logs и artifacts возвращает через runner protocol. Docker/Kubernetes/shell являются infra adapters за runner boundary.

## Consequences

- `runner` registry уже участвует в protocol MVP, но legacy inventory endpoints сохраняются для оператора.
- Текущий protocol MVP закрывает внешний lease token/ack/renew/complete, `workspace.checkoutUrl`, fencing generation и отдельный shell-runner process; ещё требуются durable queue, cancellation signal/control endpoint, credential rotation/revocation, log/secret/artifact protocol и sandbox policy.
- Локальный embedded executor остаётся development adapter до готовности отдельного runner service; production docs не должны рекламировать его как безопасный runner pool.

## Related

- `docs/RUNNER_ARCHITECTURE.md`
- `docs/FUNCTIONAL_ARCHITECTURE.md`
- `docs/adr/0003-manual-job-transitions.md`
