# ADR-0024: сквозные проекты и конфигурации репозиториев

Статус: принято для реализации Forge по поручению владельца, 2026-10-09.

Таблица Forge `projects` хранит CI-конфигурации. UUID, pipeline history, secrets,
schedules, environments и legacy API сохраняются. Этот ID не является Tracker
Project ID или Namespace ID. Репозиторий допускает несколько конфигураций.

Admin владеет Namespace и подтверждёнными привязками. Tracker владеет именем
и ключом проекта, которые использует общий Base avatar. Forge читает ограниченный
machine-only каталог в фоне и атомарно сохраняет проверенный полный снимок
в `forge_workspace_projects`. Презентационные данные не присваивают ресурс
и не разрешают запись. Соединение использует NamespaceRef и Tracker ResourceRef,
а не имена или хвосты Git URL. Успешно пустой снимок отличается от недоступного
первого обновления. Ошибка сохраняет предыдущий снимок с признаком stale.
Существующий active CI использует локальные bindings независимо от reader.

API `/workspace-projects` и UI `/workspaces/{registry}/{namespace}` представляют
сквозные проекты. Настройки конфигурации используют `/delivery-configs/{id}`;
старые `/projects/{id}/...` перенаправляют к той же конфигурации. Legacy API,
pipeline IDs, PR numbers, hosted storage identities и clone aliases сохраняются.

Общие и отдельные execution lists соединяют репозитории по UUID, фильтруют
и считают данные в PostgreSQL до пагинации. Они не подразумевают массовый запуск.
Правый picker и оба сайдбара используют URL context. `project_scope=all`
сохраняется при переходах; fold-state хранится по Tracker ResourceRef.
Base владеет представлением навигации, продукты — данными, маршрутами и состоянием.

Создание конфигурации атомарно фиксирует привязку и original-operation readback.
Replay проверяет сохранённую команду до нового write lease. Неизвестный результат
читается по исходному operation ID. Подключение существующей конфигурации
неизменяемо. Связанный checkout URL нельзя переписать; удаление managed
конфигурации или конфигурации с execution/deployment history возвращает конфликт.

Push выбирает одну явную конфигурацию либо отключён. Миграция сохраняет
существующие однозначные single-config bindings; при нескольких не выбирает
первую. Решение сохраняется по исходному hook key до запуска. Изменение policy
не перенаправляет повтор на другую конфигурацию. Для dedup используется прежний
pipeline source/idempotency receipt. Новые mutations проходят archive admission.
Trusted-shared human policy не расширяет machine routes, scopes credentials
или approvals защищённых окружений.

Migration 0091 заменяет global уникальность имени конфигурации уникальностью
в репозитории и отдельной уникальностью среди unbound legacy записей. Остальная
схема расширяется аддитивно. Неоднозначные записи сохраняются в диагностическом
списке до явного подключения. Старые migrations/checksums не переписываются.

Поставка требует inventory, проверенного backup и migration test заполненной БД,
затем общего gate и проверки exact images в pdlc1. Common, соседние installations,
pins, Git и история БД защищены. Rollback использует проверенный совместимый
cohort, сохраняет guards и схему; автоматических down migrations нет.
