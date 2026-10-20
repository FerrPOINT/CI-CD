# ADR-0035: допуск до count/page общего каталога репозиториев

Статус: opt-in source candidate, установленная политика не менялась.

## Причина и решение

REST middleware сохранял только claims, теряя исходный verified deadline.
Глобальный каталог считал и выбирал все строки без общего допуска. Фильтрация
после LIMIT/OFFSET скрыла бы строки, но раскрывала total и ломала страницы.

Middleware сохраняет private VerifiedRequestIdentity в extensions после прежних
scope/role checks. Human read adapter получает только этот owned контекст, без
HTTP body, нового bearer, доверия заголовку пользователя или повторного TTL.
Существующий SDK одним coherent bounded proof подтверждает owner SpaceRef для
локальных group mappings. Каталог ограничен10000, превышение закрывается явно.

CatalogRead выводит UUID групп из подтверждённых readable SpaceRef. Один и тот же
SQL-фильтр применяется до count, search и LIMIT/OFFSET. При active policy пустой
selected передаётся как пустой массив; NULL обозначает только прежний режим OFF.
Роль локального admin не отменяет visibility. Неподтверждённые/unbound ресурсы
не становятся публичными, записи и их история не удаляются.

Read включает допустимый archived ресурс; sync_available дополнительно сохраняет
прежнее правило active. Original proof проверяется перед ответом и на body frames;
private, no-store исключает кеширование защищённой страницы. Count SQL wait не
создаёт новое окно подтверждения. PAT scope и global role checks остаются.

## Границы

Этот срез покрывает GET /api/v1/catalog/repositories. Detail, group list, tree/raw,
PR, CI/log/artifact, machine/V3 и остальные маршруты ещё требуют своих адаптеров.
Не включать partial policy до полного cohort/migration/rollback gate. DTO/URL,
зависимости, migrations и pins этим срезом не меняются.

Isolated native HTTP/PG проверяет selected/all/future/empty, admin без допуска,
literal search, group filter, total/offset, archived read/sync restriction и
истечение original proof на настоящей SQL-блокировке. Owner admission замокан;
SDK/Git/PG/Forge owner APIs настоящие. Это не live Admin или IAB qualification.
