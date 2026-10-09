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

Есть scoped Git/HTTP/runner tests, без новой таблицы, зависимости или очереди.
Source follow-up: create-new/sync completion intent + acknowledgement journal,
runner-owned GET terminal readback и explicit offline inspect/reconcile CLI.
External startup не получает новую работу при unresolved папках; readback после
unknown completion не повторяет POST. Local records не подписывают runtime truth;
cleanup при restart требует fresh exact server receipt. Expiry/mismatch не ACK.
Pending migration 0039 добавляет `completion_received_at` без backfill:
cancel-on-expiry не является принятым completion. При неподтверждённом child wait
outer handler не отправляет failed completion, не объявляет свободный слот и
выходит из polling; marker остаётся для owner/process reconciliation.
No SDLC task/assignment identity или новый scheduler этим не вводятся.

Embedded runner также требует успешный `child.wait()` до terminal write и cleanup.
Ошибка ожидания, включая timeout/kill path, оставляет process identity, lease и
каталог для reconciliation; она не преобразуется в доказанный failed outcome.
Fallible timeout logging идёт после подтверждённой остановки. Это не process-tree
sandbox и не автоматическое доказательство отсутствия дочерних процессов.

Retention после crash требует disk monitoring и явной owner reconciliation; нельзя
объявлять cleanup успешным по отсутствию процесса или EOF. Shared cache, shell
process и embedded Docker/control-plane границы ещё не production sandbox.
Task/root/assignment permissions, SDLC quarantine/lookup и trusted versioned receipts
остаются отдельным B-SDLC-04. Private role/skill content не требуется этому срезу.

## Related

- [SDLC_DELIVERY_V1](../SDLC_DELIVERY_V1.md)
- [Operations](../OPERATIONS.md)
- [ADR-0007](0007-runner-security-boundary.md)
