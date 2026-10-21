# Человеческие роли и приложения Forge

Статус: feature-исходники, не установленная политика. Канон — Base
`docs/platform/HUMAN_APPLICATION_ACCESS.md` и ADR0011.

Human user/admin определяется текущим Auth. Новый linked profile хранит
историческую локальную developer запись без auto-admin; response claims получают
текущиеuser/admin. Старые viewer/developer/maintainer и service contracts
сохраняются как технические/исторические значения и не являются новыми
человеческими назначениями.

Существующий exhaustive route registry отдельно объявляет Work,
Administration и Technical. Обычные Git/ветки/PR/разрешённыйmerge/CI/log/artifact
операции отделены от repository bindings, external sync, delivery configuration,
secrets/runners/environments/schedules/webhooks и production effects.
HTTP middleware проверяет этот gate независимо от прежнего scope/project role.
Registered delegated credentials сохраняют собственные exact paths/fences;
они не превращаются в human principal.

Один live bridge proof используется на входе HTTP запроса. Synthetic claims
не получают новый15-минутный срок: максимум60с и не дольше исходного proof.
Auth errors/app denial не превращаются в legacy SSO fallback.

До активации остаются открытыми native Git/public/legacy transport admission,
точный effect/commit deadline, downloads, runner stages/current owner,
production delivery controls, frontend capabilities/cache, populated migration,
rollback и совместимый serving cohort. Одна route classification не доказывает
эти границы. Политика пространств независима и должна интегрироваться без
переписывания чужих worktrees. Source/tests/image/dev/IAB/real job различаются
в отчёте поставки.
