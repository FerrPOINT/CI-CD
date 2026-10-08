# ADR 0023: Git Group и стабильная идентичность repository

Дата: 2026-10-08. Статус: принято для реализации Namespace.

Admin владеет Namespace; Forge владеет Git Group, repository catalog, Git,
PR и CI configurations. Старый delivery `project_id` остаётся своим ID.
Catalog repository ID совпадает с существующим repository UUID; публичное
`group/slug` и внутреннее hosted storage identity независимы.

Публичные aliases разрешаются однозначно. Новое hosted storage использует
UUID; attach существующего repository сохраняет старый bare path и flat alias.
Неоднозначный URL-tail и создание пустого bare repository при утрате истории
не допускаются. External repository регистрируется без hosted storage.

Git push и PR mutation держат shared admission lock по Repository ID, затем
Git Group. Attach держит exclusive Repository lock в том же порядке. Archive
держит exclusive Group lock и закрывает новые starts. Уже работающие jobs
завершают terminal drain. Для admission выделены две connections сверх основного
пула; deployment обязан учесть их в общем PostgreSQL connection budget.

PR и CI mappings используют repository UUID; IDs и PR numbers сохраняются.
Task link хранит TaskRef и исходный commit snapshot; evidence выбирает pipeline
по точным repository UUID и SHA. Зелёный pipeline другого commit не подходит.

Rollback допускает только cohort с local bindings, guards и новой идентичностью.
Отключение UI не снимает admission. Purge managed repository не входит в v1.
