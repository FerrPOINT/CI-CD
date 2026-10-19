# DevOps Task Child: Committed Delivery Binding

Статус: Configuration only, source candidate от main
`d405535ee1e0f4664d49580489f85715ffe8913c`. Rust/PG и native delivery НЕ запускались.
Наличие source не означает installation или SDLC acceptance.

## Existing V1 Commitment

Новый Auth prefix, issuer, context version, SDK или migration НЕ нужны.
`TaskCredentialContext` сохраняет прежние восемь полей/version1 и scope
`ci-cd:sdlc:task:v1:<canonical-context-sha256>` вместе с read/write.
Его `variablesSha256` уже покрывает весь variables object. Existing pipeline
POST сверяет этот hash через `bind_trigger` до admission и сохраняет variables.
0094 CHECK не меняется. Прежний proposal прямого v2 context отклонён в пользу
existing commitment; старые journals не переписываются.

Trusted server producer использует
`task_delivery::delegated::delegation_inputs(context, variables, key, ttl)`.
Он возвращает прежний v1 context с рассчитанным variables hash, variables и
existing Base delegate request; НЕ выпускает token/новую authority самостоятельно.
Parent/root остаётся у server producer. Caller должен получить binding из
retained owner evidence и fresh Tracker, а не принимать agent-provided policy.
Fleet publication caller этим Forge donor не изменяется и требует отдельного wiring.

Closed variables object содержит РОВНО `forgeTaskDelivery`: JSON string с закрытым
typed payload. Hash покрывает точные string bytes, не нормализованный после decode
object. Это совместимо с existing CLI `pipeline run --variable forgeTaskDelivery=<text>`;
producer возвращает точное значение, consumer проверяет hash ДО строгого decode.
Поле не содержит token или credentials. Внутри payload:

| Поле forgeTaskDelivery | Retained binding |
| --- | --- |
| `kind` | Постоянный `forge/task-delivery/v1` |
| `command` | Полный existing DeliveryCommand: original workspace key/hash/binding, delivery key/action, artifact ID, taskFence, expected current-manifest CAS |
| `sourcePipelineId` | Исходный Developer build, НЕ произвольный pipeline/current DevOps job |
| `developmentResultSha256` | Canonical hash точного immutable `receipt` из Tracker accepted-result readback |
| `targetKind` / `targetPolicySha256` | Настроенные owner static/oci и точный policy hash, не agent URL/path |
| `artifact` | Для deploy: id/attemptId/sha256/sizeBytes/configSha256/planSha256; для rollback null |
| `rollbackManifestSha256` | Для rollback exact last-confirmed manifest; для deploy null |

Variables ограничены16KiB, DTO закрыты, missing required/extra/unknown fields отвергаются.
Old v1 credentials с `{}`, произвольными variables или только role=devops НЕ
получают delivery authority. Body не заменяет retained variables/hash.
Полный request body должен совпасть с committed command, а path — с его keys.

## Consumer И Custody

Task allowlist добавляет только существующие workspace GET, candidate-evidence GET,
delivery POST и original delivery GET с canonical project UUID/bounded keys.
Workspace POST, lists, generic deployments/rollback/admin/Git остаются закрытыми.
Allowlist проверяет grammar; handler требует verified typed Identity и binding.
Child и legacy AccessClaims не смешиваются. SAT refresh сохраняет отдельную ветку.

Handler находит один current task pipeline по project/context hash, держит
original intent SHARE lock, читает acknowledged credential через `custody::current`
и сравнивает subject/fingerprint/full context. Variables перечитываются под
pipeline SHARE lock и повторно хешируются. Renewal по-прежнему берёт UPDATE
того же intent; старый child не получает права acknowledged successor.

Исходный build receipt читается через его собственную retained custody, НЕ через
поддельный SAT текущего child. Source Developer project/repository/task/root,
routing/requirement revision и commit совпадают с current DevOps intent.
Current assignment/run/chat/machine/lease/fence проверяются отдельно; старый
Developer credential не используется как live DevOps authority.

Повторные guards на existing static/OCI effect boundaries используют fresh Auth
`include_lifetime=true`, прежний source130 current Tracker/native verifier,
fresh accepted Developer result, exact retained artifact и owner environment/policy.
Permit ограничен minimum child/Tracker deadline, также после await/commit.
Business Tracker30s не меняется. Missing/revoked/expired/superseded/unavailable,
source/artifact/target mismatch дают deny/hold без server/parent fallback.
Candidate observer, actual artifact-byte verification, namespace readonly checks,
owner executor, current/confirmed CAS и last-confirmed rollback сохранены.

Delivery journal дополнен приватным immutable `task-child.json` в том же original
operation directory: schema, subject, full context hash, source pipeline ID.
Он создаётся до publication/Compose effect. Existing child journal обязан совпасть;
иной run/context или unmarked legacy journal не присваивается. Legacy SAT не может
продолжить child effect. Crash/unknown не сбрасывает journal: original-key GET только
читает; POST existing unknown использует прежнюю stopped-client/readback ветку,
не создаёт новый deploy. Новый key/parent не является recovery authority.

## Scope И Проверки

Прежние route/DTO/response schemas, migrations0094/0095, locks/SDK/pins, issuer и
generated API не изменены. Existing DevOps task `workspace_deploy` уже существует:
его temporary stopped-after-validation receipt не приравнивается к owner-target
delivery/rollback. Повторного engine/controller/MCP нет.

Source regressions: producer v1 scope/hash; old/unbound/unknown variables deny;
mutation всех artifact/command/policy/CAS fields; foreign project/task/assignment/
repository/source/fence; rollback binding; artifact attempt; unknown journal/run;
elapsed permit effects=0. Ignored Axum/PG regression переиспользует existing seed:
retained variables/current successor, renewal lock, HTTP denial без source и
нулевые queue/attempt/lease effects. Typed identity в этом HTTP fixture injected;
full middleware/Auth/Tracker positive deployment/OCI native gate этим не заявляется.
Rust selectors написаны, НЕ executed/list-qualified. Host fmt/docs checks отдельно.

Related: [task credential](../FORGE_DELEGATED_CREDENTIAL_CONTRACT.md),
[renewal/custody](../TASK_CREDENTIAL_RENEWAL.md),
[workspace deploy](WORKSPACE_DEPLOY_ADMISSION.md).
