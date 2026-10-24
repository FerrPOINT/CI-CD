# Forge/CI-CD: владелец ссылок identity

Source-кандидат, runtime не менялся. Протокол — [Base identity lifecycle v1](https://github.com/FerrPOINT/services-base/blob/ed61ec63b58fe9b60d8669c85de5ac6b5bec872b/docs/contracts/IDENTITY_DELETION_V1.md).
SDK pin — `ed61ec63b58fe9b60d8669c85de5ac6b5bec872b`; независимый UI pin не меняется.
Auth владеет identity/tombstone и сериализацией cancel/finalize. Forge владеет
своими данными, mapping, fence и транзакционным запретом новых ссылок.

## Inventory

| Источник | Ссылка и семантика |
| --- | --- |
| users | JIT/local profile, точный central_sub; local id/username mapping |
| user_credentials | local user_id |
| sessions | local user_id |
| api_tokens | nullable local user_id; machine principal не выдаётся за human |
| project_memberships | local user_id; связь с CI config/repository через project |
| tenant_memberships | local user_id |
| notification_preferences | local user_id |
| forge_confirmed_user_roles | local user_id |
| service_accounts | created_by: local user_id; собственный id — machine principal |
| tenants | created_by: local user_id |
| pull_task_links | created_by_user_id |
| repository_attach_operations | actor_id: local principal |
| delivery_configuration_operations | actor_id: local principal |
| repository_sync_operations | actor_id: local principal |
| pull_requests | created_by: username snapshot |
| audit_log | typed identity_refs; старый mixed actor требует classification; user resource target |
| releases | typed identity_refs; старый created_by — legacy actor |
| deployment_approvals | typed identity_refs; старый mixed actor требует classification |
| forge_space_owner_operations | точные owner_issuer/owner_subject |
| task_pipeline_intents | central-child subject/holderSubject, только для central custody |
| task_credential_successors | central child credential.subject |
| sdlc_workspace_operations | delegated_subject |
| pipeline_human_owners | обязательный central owner_subject |
| execution_attempt_human_owners | обязательный central owner_subject |
| schedules | optional central owner_subject; SQL NULL для native automation |
| schedule_fires | optional central owner_subject; SQL NULL для native automation |

Hosted repositories не имеют отдельного user owner: права задаются membership
CI config/project, записи attach/config/sync сохраняют своего actor. Отдельных
PR reviewer/comment user таблиц в этой схеме нет; reviewer в task intent — роль
машины, не PR review. Обычные pipelines/triggers не сохраняют human initiator:
история автора для них не выдумывается. Task pipeline central holder учитывается.
Git author/committer name/email не являются Central Auth identity mapping.

repository_sync_runs.owner_id, job lease owner и runner UUID — execution/lease
identity, а не human subject. Service account ID и user ID различаются по
подтверждённой локальной таблице, не по форме UUID. Messaging event/pipeline/
project IDs и произвольные variables/config/provider URLs также не являются
user identity. Outbox payload текущих pipeline status events не содержит user.

## Mapping и старые данные

Оператор явно задаёт CICD_IDENTITY_LEGACY_ISSUER, совпадающий с точным
CICD_AUTH__CENTRAL_ISSUER. Привязка сохраняется один раз и неизменяема. До неё
owner не подтверждает zero references. При bind автоматически фиксируются
только current users.central_sub → local user/principal id/username и known
service_accounts.id → non-identity machine principal. Email не участвует.

identity_legacy_aliases сохраняет kind+alias, disposition identity/non_identity,
точные issuer/subject и assertion_reference. Имена не переиспользуются для
другой central identity. Rename сохраняет прежний username; deletion/remap не
уничтожают старые alias. Классификация append-only и не является бизнес-usage.
Нельзя переписать старую attribution через изменение mapping.

Старые mixed/freeform actor и непривязанные legacy local users дают 503
identity_legacy_reference_unresolved, без alias/email в ответе. Это unknown,
не ноль. Оператор проверяет запись у владельца/историческое evidence, затем
через защищённый DB deployment канал вставляет точную classification:
kind local_user_id/local_principal_id/username/legacy_actor, исходный alias,
identity + verified issuer/sub либо non_identity, ссылку на evidence.
Нельзя автоматически считать строку actor UUID/local user/email по её виду.
Без доказательства classification не создаётся, удаление остаётся закрытым.

Новые штатные ports audit/auth/membership/denial/release/approval явно записывают
identity_refs из typed UserId/principal/verified context либо [] для данных без
identity. NULL сохраняет legacy/untyped семантику. Поэтому постоянный fence
одной удалённой identity не запрещает обычную работу других людей и workers.
Новая unresolved ссылка при active fence закрыта; raw writer не становится
поддерживаемым typed actor автоматически.

## Сериализация

Аддитивная migration 0105 сохраняет все прежние migrations. Statement trigger
берёт общий shared advisory transaction lock раньше row triggers для всех
26 reference sources. Freeze/release/bind используют exclusive lock, ожидают
прежние writers и читают inventory без FOR UPDATE бизнес-строк. Epoch row
FOR SHARE у writer и UPDATE у lifecycle закрывает старый repeatable-read
snapshot через 40001; missing epoch — отказ. Guard действует также в JIT,
CLI, workers и прямых repository ports.

Freeze сохраняет неизменный receipt, включая ненулевой count. Потеря ACK
повторяет его. Release сохраняет cancellation даже до freeze, late freeze
отклоняется. Чужая operation не снимает fence; TTL нет. Доверенный Auth
coordinator не отправляет release после finalized tombstone. Alias assertion
не разрешает обхода активного fence новой привязкой target.

## Транспорт и gates

POST /api/internal/v1/user-deletions/{operation_id}/{check|freeze|release}.
Строгое тело DeletionRequest, path/body equality, 16 KiB. Нужны свежий registered
service account, точный scope ci-cd:identity-lifecycle, отсутствие human session,
точный issuer. CICD_IDENTITY_DELETION_COORDINATOR_SUBJECTS пуст по умолчанию.
Все ответы, включая extractor errors, no-store; deadline не продлевается после
SQL wait. Публичный user-management proxy не принимает owner receipt от человека.

Квалификация: реальные PG migration/alias/old-actor/26-source guards, writer
drain/RR/missing epoch, replay/cancel/foreign issuer, работа другого пользователя
и machine после постоянного zero fence; отдельный HTTP/PG scope/role/body
matrix с loopback Auth wire fixture. Loopback не подменяет real Auth E2E.

```sh
cargo test --locked --features integration --test identity_deletion
cargo test --locked --features integration --test identity_deletion_http
```

Для точного source tree `d36358cffda9ed1b5c6caff4d2fd7b00e2c58af0` пройдены
pinned Rust 1.88 fmt и strict all-target clippy семи packages, unfiltered library
run (435 PASS, 11 ignored), оба явных owner PostgreSQL targets (по 1 PASS, 0 ignored)
и named `cicd-server` release. Счётчики разных runs не складываются; ignored
behavior не квалифицирован, binary unit target с 0 tests — compile-only evidence.
Полные populated performance/EXPLAIN gates, real Auth coordinator, production ports
после finalized deletion, cohort восьми owners, Git/runtime и Kubernetes acceptance
остаются отдельными gates. Квалификация исходного компонента не означает rollout.

## Ограничение времени и тип credential

Coordinator использует только PAT зарегистрированного service account; JWT
этого же account не заменяет PAT. Владелец устанавливает transaction-local
lock_timeout и statement_timeout по 8 секунд, общий HTTP deadline — 9 секунд.
Timeout даёт безопасный 503, не нулевой count и не подтверждение freeze;
исходный verified proof не продлевается. Тесты содержат отрицательный JWT,
реальное ожидание advisory lock и задержку Auth дольше общего deadline.

Отказ доступа machine principal сохраняется audit_principal с typed
local_principal_id; human login использует audit_user. Реальный запрос
service-account PAT к закрытому admin API после постоянного fence проверяет
403 и сохранение audit.denied без unresolved identity. Membership audit
ссылается на целевого user_id и не приписывает ему авторство команды.


## Malformed typed references

Known typed central references validate subject and an explicitly present issuer
as nonblank strings. Explicit numeric/null/blank issuer is unresolved; it never
falls back to the deployment assertion. Only an omitted issuer may inherit it.
Blank aliases and required scalar subjects are unknown; optional SQL NULL actor
or delegated subject is still absent. Unknown appears in the inventory and
check/freeze returns503; all26 DB sources reject new unresolved rows after a fence.
The regression upgrades a real released-schema blank central_sub, checks actual
raw audit JSON cases before any freeze, and verifies their denial after freeze.
Existing delegated credential JSON has strict released custody constraints;
tests do not disable them or fabricate preexisting identity_refs columns.
