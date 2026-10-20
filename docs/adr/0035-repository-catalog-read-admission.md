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

Этот срез покрывает GET /api/v1/catalog/repositories, его /{id} и
GET /api/v1/git-groups/{id}/repositories. Detail фильтрует owner group до metadata
lookup; hidden/not found даёт404 с private/no-store. Hidden group даёт пустой
каталог до namespace/availability lookups, без утечки count. Разрешённый group
фильтр определён до SQL pagination.

Типизированный get_core не является авторизацией: HTTP передаёт admitted groups.
Три прежних repository_sync callers сохраняют отдельные ACL и являются ещё
незавершёнными common-cohort поверхностями; фиктивный HTTP request не создаётся.
Tree/raw, PR, CI/log/artifact, machine/V3 и остальные маршруты ещё требуют адаптеров.
Не включать partial policy до полного cohort/migration/rollback gate. DTO/URL,
зависимости, migrations и pins этим срезом не меняются.

Isolated native HTTP/PG проверяет selected/all/future/empty, admin без допуска,
literal search, group filter, total/offset, archived read/sync restriction и
истечение original proof на настоящей SQL-блокировке. Detail/group tests
проверяют plain/percent-encoded UUID, hidden public/unbound данные, archived read
и настоящий clone по clone_url из допущенной карточки API. Owner admission замокан;
SDK/Git/PG/Forge owner APIs настоящие. Это не live Admin или IAB qualification.
