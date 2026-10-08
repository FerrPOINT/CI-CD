# Namespace, Git Group и repository catalog

Admin хранит канонический Namespace; Forge хранит confirmed local Git Group
binding. Registry/instance/resource UUID и generation обязательны. Git Group
имеет неизменяемый slug, display name и ноль или несколько repositories.

## API и данные

Owner API: `GET/PUT /api/v1/namespace-resources/git_group/{id}`.
Human contexts, available groups и counters находятся в `/api/v1/namespace-*`.
Repository catalog: `GET/POST /api/v1/git-groups/{id}/repositories`,
`GET /api/v1/catalog/repositories/{id}`. Unbound resources выбираются явно
через `/catalog/available-repositories`; `PUT /catalog/repositories/{id}/group`
сохраняет operation ID, actor и original payload/readback.

Migration 0090 добавляет Git Groups, immutable managed marker, local bindings,
stable repository catalog, aliases, attach operations, permanent PR counter и
TaskRef links. PR получает repository UUID без изменения IDs, номеров и legacy
names. CI configuration получает repository UUID; legacy `project_id` не
заменяется Namespace ID. Delivery mapping задаётся явным
`PUT /catalog/repositories/{id}/delivery-configs/{project_id}`.

Hosted storage identity отделён от `group/slug`; одинаковые slugs в разных
группах допустимы. Flat clone URLs и прежние bare paths сохраняются aliases.
Отсутствующее историческое storage диагностируется и не инициализируется заново.
External repositories используют provider refs и существующий Git/CI;
external PR provider в эту поставку не входит.

`GET/POST /catalog/repositories/{id}/pulls/{number}/tasks` хранит TaskRef,
display key и snapshot исходного SHA. Saved read не вызывает Tracker; повтор
не переснимает branch. Evidence pipeline выбирается по repository UUID и
точному source/merge SHA. Managed links не используют короткое имя для поиска.

Archive закрывает receive-pack, PR mutations и все новые pipeline starts,
включая schedules, hooks и legacy/manual API. Активные jobs допускают terminal
drain без force-stop. Потерянная projection закрывает writes. Restore сохраняет
IDs. Подробное решение: [ADR 0023](adr/0023-namespace-repository-identity.md).

## Конфигурация

`CICD_NAMESPACE__INSTANCE_ID`, `REGISTRY_INSTANCE_ID`, `OWNER_SUBJECTS`,
`READER_SUBJECTS` задаёт deployment. Reader использует отдельные
`TRACKER_URL`, `TRACKER_INSTANCE_ID`, `TRACKER_TOKEN_FILE` с тем же prefix.
`CICD_NAMESPACE__PUBLIC_GIT_ORIGIN` — фиксированный публичный Git origin.
Endpoints не поступают из human requests; redirects отключены, 10 s/64 KiB.
Machine subjects проверяются до human/admin mapping; service scopes остаются.

Admission pool имеет две дополнительные connections, основной pool остаётся
для API/hooks. Общий PostgreSQL budget проверяется до cohort rollout.
UI включается `VITE_NAMESPACE_ENABLED=true` после compatible schema/cohort.
Rollback сохраняет guards даже с выключенным UI; старые namespace-unaware
writers после активации не допускаются.
