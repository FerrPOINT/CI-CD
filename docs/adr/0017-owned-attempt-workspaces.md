# ADR-0017: Fresh lease-owned runner checkout и cleanup после ACK

## Status

Proposed; source implemented 2026-10-03. Merge/install/live acceptance отдельно.

## Context

Embedded runner использовал mutable branch и удалял предыдущую job-папку.
External runner имел fresh path, но не проверял full SHA/clean readback и очищал
файлы до server completion. Потеря ACK уничтожала контекст reconciliation.
Нельзя добавлять второй scheduler или объявлять filesystem marker trusted SDLC receipt.

## Alternatives Considered

- Reuse job path + `git clean`: сохраняет риск чужих файлов и destructive retry.
- Отдельный workspace scheduler: дублирует existing job queue/lease ownership.
- Delete до completion: экономит storage, но теряет evidence при unknown effect.

## Decision

Оба runners используют общий физический IO helper с fresh attempt/lease/generation
directory и строго проверяемым marker. Pinned checkout detached и clean; SHA
проверяется до команд без mutable fallback. Legacy unpinned jobs остаются совместимы,
но не являются SDLC admission. Cleanup выполняется в blocking IO worker только после
owner terminal ACK/readback и повторного path/marker guard. Docker-job получает только
свой attempt bind, не общий workspace root. Неизвестный результат сохраняет папку.

## Consequences

Есть scoped Git/HTTP/runner tests, без нового API, таблицы, зависимости или очереди.
Retention после crash требует disk monitoring и явной owner reconciliation; нельзя
объявлять cleanup успешным по отсутствию процесса или EOF. Shared cache, shell
process и embedded Docker/control-plane границы ещё не production sandbox.
Task/root/assignment permissions, formal quarantine/lookup и trusted versioned receipts
остаются отдельным B-SDLC-04. Private role/skill content не требуется этому срезу.

## Related

- [SDLC_DELIVERY_V1](../SDLC_DELIVERY_V1.md)
- [Operations](../OPERATIONS.md)
- [ADR-0007](0007-runner-security-boundary.md)
