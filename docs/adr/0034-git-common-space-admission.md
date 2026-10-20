# ADR-0034: общий допуск на Git Smart HTTP

Статус: opt-in source candidate. Рабочая установка не менялась.

## Контекст

Public/legacy early returns обходили общую visibility policy. Forge CI project
не равен Tracker ProjectRef; URL, имя и первый проект не определяют owner Space.
Нельзя расширять старый task V1 capability ради нового человеческого допуска.

## Решение

Переиспользовать Admin admission через существующий bounded SDK и HTTP client.
Сервер выводит SpaceRef из immutable repository UUID/group/v2 catalog, передаёт
проверенную центральную identity и отдельный narrow registered reader credential.
Исходный срок всех подтверждений сохраняется до native IO через ADR0033.

Read admission открывает чтение, сохраняя PAT project restriction. Write требует
прежней продуктовой роли, scope, active состояния и совпадения owner/local revision.
Local archive не отменяет действующий read proof того же immutable SpaceRef.
Public/legacy не обходят admission; unbound/missing projection и старый task V1
не получают default Space или человеческие полномочия.

Новая0097 содержит только выключенный activation state, без user policy и нового
каталога. Включающего API нет. До полного compatible cohort, populated migration,
role/access diff и rollback активировать partial source нельзя. Старые образы
после будущей activation не могут служить способом обхода.

## Границы

REST/catalog/tree/raw/PR/CI/artifacts/logs, machine/V3/runner и production не
объявлены покрытыми этим адаптером. Реальная интеграция Admin/cohort и IAB отдельно
от isolated native fixture, где owner HTTP замокан, а SDK/Git/PG настоящие.

[Матрица и qualification](../UNIFIED_SPACE_SURFACES.md#opt-in-human-git-space-admission-source-candidate).
