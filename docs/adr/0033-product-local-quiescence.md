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
