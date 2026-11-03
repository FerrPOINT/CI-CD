# ADR-0046: повтор и отмена исходного deployment request

Статус: принято для реализации; установка и live-приёмка подтверждаются отдельно.

## Контекст

Fleet передаёт request key, а Forge уже имеет его колонку и unique index.
HTTP DTO прежде игнорировал ключ; повтор после потери ответа мог создать новую
выкладку. Отмена вызывала отсутствующий endpoint, а локальный terminal status
не подтверждал окончание удалённого исполнения.

## Решение

Сохраняем владельца выкладки в Forge, существующие deployment rows, pipeline
trigger idempotency и runner reconciliation. UUID ключа связан fingerprint с
видом операции, target, параметрами и проверенным автором. Одно окружение и
ключ определяют одну запись. Несовпадение или неизвестная legacy provenance
возвращают конфликт. GET исходного request проверяет его подтверждённого автора
по атомарному creation audit; чтение по ID по-прежнему проверяет project access.

Отмена добавляет durable timestamp к той же записи. Блокировка deployment
сериализует её с публикацией pipeline; прежний pipeline/execution механизм
останавливает уже принятое выполнение. 202 означает ожидание подтверждения,
200 — фактически известный terminal outcome. Поздняя отмена не меняет успех на
ошибку и не запускает rollback.

Fleet сохраняет environment/key перед POST и исходный ID после readback.
При неизвестном создании и принятом cancel отсутствие результата не разрешает
создать новую выкладку. Ошибка транспорта сохраняет неопределённость. Каждый
короткий control request ограничен deadline; общий SSE client не ограничивается
коротким timeout.

## Альтернативы и границы

Отдельный оркестратор или operation registry дублировал бы существующего
владельца. Немедленный локальный Cancelled либо повторный POST без ключа были
бы короче, но нарушали бы подтверждённый outcome и создавали дубли. Существующая
PostgreSQL unique constraint, transaction и owner trigger дают нужные гарантии
без новой подсистемы.

Nullable поля вводятся аддитивно; старые checksums и DTO чтения сохраняются.
Forge устанавливается раньше Fleet. Старый writer, не соблюдающий принятую
отмену, не возвращается при открытых cancel requests. Неизвестный исход требует
сверки исходной операции; его нельзя объявить завершённым по таймеру. Audit
исходной незавершённой операции сохраняется как часть её readback provenance.

## Проверка

Ручной native gate выполняется на изолированной `forge_space_test`, заданной
`FORGE_SHARED_SPACE_TEST_DATABASE_URL`:

```bash
cargo test -p cicd-server --test rust_audit_regressions -- --ignored --test-threads=1
```

Проверяются concurrent replay, иной
payload/автор, rollback, audit fault, cancel до/во время pipeline и повтор
после потери ответа. Отдельные Compose/Kubernetes, Fleet и dev-проверки
обязательны; native fixture их не заменяет.
