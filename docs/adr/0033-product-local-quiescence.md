# ADR-0033: Product-local Forge quiescence

Статус: принято для source; установка/native backup qualification отдельно.

Forge должен закрыть новые submissions и autonomous producers, дать ранее
выданным executions завершить renew/control/log/artifact/complete и physical
cleanup, затем удерживать подтверждённый writer barrier в течение backup.

Рассмотрены публичный admin API с per-process counters и private shared
filesystem control. Выбран второй: existing Unix flock, private directory,
persistent operation marker, durable effect custody и foreground coordinator.
Это исключает новый public API/schema/credential и даёт один product-local
kernel protocol для server, external runner, delivery и migration CLI.

Generic HTTP middleware не является единственным owner: native/detached
workers и background producers владеют своими leases до actual completion.
HTTP cancellation оставляет actual handler task выполняться с его lease.
Coordinator требует explicit pinned complete participant manifest, live kernel
registration locks, отсутствие effect records и source-owned DB/physical
custody closure. Unknown всегда HOLD. Истечение lease/process exit не является
доказательством stop. Kernel fence не заменён глобальными SQL write locks.

При выключенном opt-in прежние Compose/Docker/shell contracts сохраняются.
Все source/SDK/UI pins сохранены; dependency edges migration binary используют
уже закреплённые workspace libraries, новые версии не добавляются.

Первое подключение legacy runtime требует отдельной qualification точного
старого image и original-state preservation. Новый source не доказывает старый
CID. Контроль и legacy fixture protocol: [FORGE_QUIESCENCE](../FORGE_QUIESCENCE.md).

После полного held ACK тот же private coordinator может запросить normal
idle exit exact pinned boots. Новый nonce/ACK и живой exclusive fence защищают
от stale intent; kernel owner disappearance и replacement проверяются без
PID reuse assumptions. Все четыре Forge process kinds имеют этот путь.
Authenticated upload-pack остаётся tracked continuation до fence; receive-pack
не открывается. Old-image TERM fixture FAIL не скрывается новым source.

Private directory использует existing workspace PVC вне job subPaths.
Restore/generation retirement требует явного release-owner mapping source
exited receipt → current operation → destination UIDs до первого нового writer;
автоматического удаления/игнорирования unknown custody нет. Native handoff
остаётся открытым gate.
