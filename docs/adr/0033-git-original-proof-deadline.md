# ADR-0033: исходный срок подтверждения в native Git

Статус: source candidate; isolated native HTTP/PG qualification выполнена.
Установка, Space admission и activation полного cohort остаются отдельно.

## Причина

Проверка scopes и срока перед/после ACL SQL не ограничивала последующий native
advertisement/receive/upload. Lock wait, custom hook и body handoff могли
пережить исходное подтверждение. Простой timeout с удалением future мог раньше
времени освободить namespace lease либо оставить hook process.

## Решение

Private GitAccess сохраняет original VerifiedCentralToken deadlines, без bearer,
новой capability или новой таблицы. После locks и до native spawn срок проверяется
снова. Detached owner сохраняет прежние lifecycle lease и MutationGuard до
фактического native exit и собственного cleanup. Advertisement использует тот
же supervisor. Срок охватывает ожидание inherited stdout/stderr pipe writers.

Receive добавляет private reference-transaction wrapper. Custom prepared hook
выполняется первым с прежними stdin/argv; затем проверяется fixed boot-clock
и wall deadline до native ref preparation. CLOCK_BOOTTIME включает suspend.
Перевод использует только оставшееся Instant-окно с округлением вниз, никогда
новый TTL. Последняя неполная секунда и ошибка получения clock закрывают запуск.
Вне Linux эта ветка fail-closed, без небезопасного wall-only fallback.

По deadline owner завершает только созданную им process group и ждёт native Git.
Это повторяет существующую Unix-модель sync Git, без нового subprocess service
или credential protocol. Private hook files удаляются после native completion;
исходные repo hooks/config не меняются. Protected response получает private,
no-store и проверяет original proof при передаче body frames.

## Границы и проверка

Admission не заменяет роли, scopes, project restriction или task child grants.
Public/legacy/task permissions этим срезом не меняются и пока не квалифицированы
для общего пространства. Старые SQL migrations/receipts, refs и API schema сохранены.
Прежние buffered IO и output-size semantics не получают нового произвольного
лимита. Это проверка авторизации перед native ref preparation, без обещания
жёсткого реального времени для диска и отзыва уже отправленных байтов.

Actual HTTP clone/push проверяет разрешённую запись, expired slow prepared hook,
сохранение соседней ref, остановку позднего hook effect, custom rejection и
успешный следующий push. Slow post-receive доказывает unknown publication:
клиент видит отказ, но exact ref readback находит уже записанную ref.
Timeout не доказывает rollback и не разрешает автоматический retry.

[Матрица оставшихся gates](../UNIFIED_SPACE_SURFACES.md) отделяет source/native
результат от полного serving cohort, миграции, dev и браузерной приёмки.
