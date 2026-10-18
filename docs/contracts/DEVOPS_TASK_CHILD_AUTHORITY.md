# DevOps Task Child: Граница Delivery Authority

Статус: Configuration only; proposal 2026-10-19, НЕ реализованный grant.
Проверен fresh main `d405535ee1e0f4664d49580489f85715ffe8913c`.
Это продолжение [task credential](../FORGE_DELEGATED_CREDENTIAL_CONTRACT.md)
и [current custody](../TASK_CREDENTIAL_RENEWAL.md), не разрешение установки.

## Доказанная Граница

`TaskCredentialContext` v1 содержит current project/task/assignment/execution,
source, run/chat/machine и original pipeline trigger/variables. Но НЕ содержит
delivery action/key, accepted Developer result, source pipeline/artifact или
owner target policy/current manifest CAS. `DeliveryCommand.taskFence` является
caller input, не частью grant. Роль `devops` сама не добавляет полномочий.

`Identity::route` допускает только pipeline POST, exact PipelineDetail и
task-workspace GET. Existing workspace/candidate/delivery handlers и повторный
`oci::TaskAuthority::refresh` требуют настоящий project-local SAT через
`sdlc_workspace::authorize`. Подстановка child subject в `AccessClaims`, server
SAT или parent token нарушила бы эту границу.

`custody::current` держит SHARE lock исходного task intent, а renewal берёт UPDATE
и требует прежний context. Исторический Developer child не может авторизовать
новый DevOps assignment: fresh Tracker validation проверяет исходные role/stage,
assignment/run/lease/fence. Старый build можно читать как source evidence, но
нельзя использовать его credential как текущую delivery authority.

Это НЕ blanket blocker существующего DevOps CI: task pipeline уже поддерживает
[workspace_deploy](WORKSPACE_DEPLOY_ADMISSION.md) с текущим DevOps intent и
owner-generated artifact. Его temporary workspace и stopped-after-validation
receipt не равны owner-target delivery/rollback через отдельные routes выше.
Повторно реализовывать pipeline deployment не требуется.

Дополнительный schema blocker: CHECK `task_pipeline_credential_variant` в0094
требует РОВНО восемь v1 context fields и `version=1`. Добавление optional поля
или одной route ветки не создаёт сохраняемый versioned grant. Исторические
0094/0095 и старые custody rows менять нельзя. Forward migration, producer и
consumer contract требуют отдельного review до implementation.

## Минимальный Versioned Binding Для Review

Предлагается v2 того же task context, НЕ новый issuer. Существующие восемь полей
сохраняют смысл; `version=2` и один обязательный closed `delivery` object.
Только `intent.role=devops`. Scope proposal:
`ci-cd:sdlc:task:v2:<canonical-full-context-sha256>` плюс прежние read/write.
V1 consumers обязаны отвергать v2; Base policy для нового prefix не считается
уже разрешённой. Если существующая issuer policy не допускает prefix, требуется
её отдельная явная публикация, не SAT fallback или расширение child scopes.

| Поле delivery | Exact binding и источник |
| --- | --- |
| `command` | Полный existing `DeliveryCommand`, включая original workspace key/hash/binding, action, delivery key, artifact ID, taskFence и current-manifest CAS |
| `sourcePipelineId` | Retained Developer build pipeline, не текущий DevOps pipeline по умолчанию |
| `developmentResultSha256` | Canonical hash принятого immutable Developer result, ID уже содержится в taskFence |
| `targetPolicySha256` | Canonical hash опубликованного owner policy; target выбирает server, не agent URL/path |
| `targetKind` | Точный configured `static` или `oci`, не переключение executor из body |
| `artifact` | Для deploy: exact artifact ID/attempt ID/SHA256/sizeBytes/configSha256/planSha256 из retained source evidence; для rollback: null |
| `rollbackManifestSha256` | Для rollback: exact last confirmed manifest; для deploy: null |

Все UUID canonical/non-nil, SHA lowercase/full, field set закрыт; неизвестные,
дублирующие и null-required поля отклоняются. `command.taskFence` обязателен и
совпадает с current intent по assignment/execution/agent/lease/hash/fence.
Original workspace binding остаётся Developer binding, НЕ переписывается в DevOps.
Project/repository/source/task/root/routing/requirement revision обязаны совпасть
между текущим intent, исходным build и accepted result. Artifact ID в command
обязан совпасть с artifact object. Mutable tag/status/PR link не authority.

Producer берёт эти поля из server-retained data и fresh Tracker; agent не
выбирает их для выдачи grant. Новый child не преобразует существующий v1 journal:
v1 pipeline/receipt/key сохраняются. V2 нужен собственный immutable task command,
использующий существующую custody/renewal модель; способ её admission и узкая
forward CHECK migration подлежат review. Renewal меняет только credential,
не context/command/artifact; ни trigger, ни delivery повторно не исполняются.

## Consumer После Review

Оставить прежние route/DTO/wire SAT неизменными. Отдельный typed child dispatch
может открыть только exact operation/candidate/delivery POST/GET данного grant;
никаких lists, generic deployment/rollback, admin, secrets или local CLI grant.
Сам allowlist проверяет только grammar, полное binding проверяет handler.

Под current intent SHARE lock получить `custody::current`, проверить current
fingerprint/subject/full context. Не блокировать renewal другим порядком locks.
Source receipt искать по retained pipeline и его исходным delegated ownership
fields, не `find_receipt(project, current_child_subject, key)` и не SAT identity.
Для чужого source pipeline/task/project либо mixed SAT/delegated variant — deny.

Повторно выполнять actual child introspection с `include_lifetime=true`, fresh
Tracker current assignment/run/fence, accepted result и repository mapping перед
effect и после await. Permit = minimum child/Tracker deadline; expired/revoked,
unavailable, superseded credential или изменившийся artifact/target — deny/hold.
Existing executor обязан вызывать тот же child verifier у СВОИХ effect boundaries,
а не только в route: static publication и OCI Compose/reconciliation включены.
Legacy `TaskAuthority` SAT refresh остаётся отдельным вариантом.

Исходный durable delivery journal предшествует effect. Unknown ACK читается
по original delivery key и exact immutable binding; новый child/parent/key не
даёт повторного dispatch. Rollback остаётся last-confirmed exact CAS, не общий
deployment rollback. Technical receipt не завершает Tracker task автоматически.

## Приёмка И Текущий Donor

Donor добавляет только три negative Rust source tests: v1 DevOps pipeline access
сохраняется, workspace/delivery routes закрыты; injected binding/v2 wire отвергнуты;
Developer custody, SAT/admin/Git scopes и неразрешённые grant versions не обходят
границу. Production auth/routes, SQL, pins, dependencies и generated API неизменны.
Rust tests написаны, но НЕ запущены; sourcefmt/docs checks не заменяют typed gate.

После review versioned contract нужны focused units и existing HTTP/PG fixture:
exact current child allow; cross task/project/source/artifact/target/CAS deny;
expired/revoked/superseded child и Tracker deadline-after-await дают effects=0;
renewal lock ordering; original-key replay/unknown readback без dispatch;
static/OCI effect-boundary revalidation; SAT/default wire без изменений.
Новый controller, MCP, migrations history rewrite и agent server token не нужны.
