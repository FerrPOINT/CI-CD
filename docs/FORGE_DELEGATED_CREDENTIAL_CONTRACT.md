# Delegated Forge task credential

Source slice от Forge `e5d04caf94d8809184e554532efb2f87243e90c8`, SDK
`2ad0834d06d3aa703fa1791a932bfc76650c8e42` без изменений. Rust/PG qualification,
image build, installation и live issuance не заявлены. Default выключен.

## Producer и private publication

Используется существующий Base `POST /auth/tokens/delegate`, service `ci-cd`.
Base issuer, SDK, token namespace, max1800s/parent expiry, replay и revoke прежние.
Нового SAT issuer, MCP, controller, очереди или admin fallback нет.

Серверный producer берёт ТОЛЬКО подтверждённые retained/current Fleet/Tracker
данные. `delegated_credentials::delegation_request(context,key,ttl)` строит пять
existing Base fields: service, постоянный non-secret label, scopes,
idempotency_key и expires_in_seconds. Это pure request builder, не mint action.
Fleet caller/root custody/publication не реализуются внутри Forge или CLI.
Root, human token и Forge server readback credential не передаются agent.

Child содержит ровно scopes:

```text
ci-cd:read
ci-cd:write
ci-cd:sdlc:task:v1:<canonical-context-sha256>
```

Настроенная Base policy разрешает этот extra prefix только выделенному central
machine subject/service. Обычный root без grant не даёт task authority. Этот
механизм соответствует Tracker delivery grants; Base не проверяет assignment.
TTL должен укладываться в существующий budget producer; replay не продлевает его.
Новый parent/key или другой context не заменяют первоначальный child operation.

`TaskCredentialContext` — закрытый camelCase JSON:

```text
version: 1
projectId: <Forge CI-configuration UUID, не Tracker project UUID>
intent: <полный existing TaskWorkspaceIntent без изменения его wire>
runId: <точный retained native run UUID>
chatId: <точный retained native chat UUID>
machineSubject: <точный retained native machine subject>
triggerId: <original UUID из Idempotency-Key>
variablesSha256: <canonical SHA256 полного variables object; {} если variables нет>
```

Hash: UTF-8 JSON, keys рекурсивно sorted, без whitespace, lowercase SHA и canonical
UUID, без ASCII-escaping Unicode. `intent` уже связывает repository/source SHA,
task/root, Tracker instance/project, routing snapshot/revision, assignment/hash,
execution/role/agent, lease/holder/version/fence и original workspace key.
Grant hash охватывает ВСЕ поля context. В body передаётся тот же context, но body
не является authority: hash должен совпасть с live Base scopes.

## Consumer и разрешённые действия

DevOps delivery остаётся отдельной authority-границей. Closed payload внутри
existing `variablesSha256` commitment и source consumer описаны в
[DevOps task-child binding](contracts/DEVOPS_TASK_CHILD_AUTHORITY.md).
Old/unbound v1 journals не получают delivery authority; child не становится SAT.

Отдельная typed identity, без поддельных local SAT UUID/service-account name или
human/admin role. Только opaque PAT без browser SID/role, exact configured central
subject, один валидный task grant и ровно три разрешённых scope. Malformed,
ambiguous, cross-service/admin/git scopes не попадают в legacy/human fallback.
Namespace auth отдельно отвергает task grant.

Только existing routes:

- `POST /api/v1/projects/{project_id}/pipelines` с обязательными task_intent,
  credential_context, exact source SHA в git_ref и original Idempotency-Key.
- `GET /api/v1/projects/{project_id}/pipelines/{pipeline_id}/task-workspace` для
  исходного child fingerprint/context/project. Null означает pending, не PASS.

Это project-bound task readback, НЕ список проектов/чужих pipelines. Без отдельного
typed variables binding delivery остаётся закрытым. Source consumer binding выше
добавляет только exact existing delivery/readback routes, не generic deployment.
Retry/cancel, secrets/admin, namespace, runner protocol и Git credential права
этим child не открываются. Legacy local SAT routes/scopes/FKs остаются отдельной веткой.

Перед admission, enqueue, replay после ожидания lock, async prepare, spawn и
submission readback проверяется ТОТ ЖЕ child через live Base introspection.
Ordinary runner также перечитывает child перед copy-back, artifact collection и
terminal publication; local SAT ветка этих guards не получает новой authority.
Current Tracker lifecycle дополнительно сравнивает task/root, role/stage,
assignment/hash, routing, exact native run/chat/machine, holder/lease/version/fence.
Прежний <=30s Tracker lease budget/monotonic deadline/CAS не увеличен.
Expired/revoked child/parent, Base unavailable или сменившийся task/run дают
deny/hold; cached grant/server token не подставляется.

### Lifetime: явная граница

Delegated effect запрашивает постоянный opt-in
`GET /auth/tokens/introspect?include_lifetime=true` и требует `expires_at` и
`observed_at` в том же identity response. Последнее поле — свежие
PostgreSQL-часы issuer.
Default introspection без query сохраняет четыре legacy поля; обычный Forge
`try_central` через pinned SDK не запрашивает lifetime и не меняется.
Pinned SDK не меняется: metadata читает existing Forge central-auth adapter;
старый Base без этих полей закрывает delegated effect, а не выдаёт бесконечный
permit. Legacy SAT/human authentication остаются прежними.

Child deadline вычисляется от начала HTTP-запроса плюс `expires_at - observed_at`;
transport/decode/DB/Tracker ожидания не продлевают его. Async prepare, spawn и
publication получают минимум child deadline и прежнего Tracker deadline.
После Tracker await и commit перед ordinary publication выполняется повторная
проверка. Идентичность, original fingerprint/context и live revoke проверки
сохраняются; metadata не разрешает подменить child или assignment.

Base replay не продлевает expiry. Credential file refresh у caller НЕ заменяет
child в immutable async journal. Renewal для той же сохранённой command в этом
slice НЕ поддержан: другой live child, даже с тем же assignment/run/fence/context,
не пройдёт original fingerprint. Это release gap для queue+build+publication,
выходящих за child/parent lifetime. Новый key/parent автоматически не выдаётся.

Прежний runner не обрывает уже admitted opaque process через30s: Tracker permit
проверяется перед spawn; долгий job ограничен своим существующим timeout. После
завершения новый effect требует свежих child и Tracker readbacks. Непрерывное
прерывание уже запущенной команды по PAT revoke здесь не добавлено; child не
публикуется в job env и не превращается в бессрочный grant.

## Хранение и совместимость

Единственная новая additive SQLx migration `0094_delegated_task_credentials.sql`
не занимает номер0093, уже применённый repository-sync foundation в соседнем
workspace. SQL новой миграции не изменён при перенумерации. Автоматическая
регистрация через existing SQLx migrations directory. Исторические файлы не изменены.
Установка поверх repository-sync0093 требует отдельного опубликованного
foundation-пакета с точной применённой историей; отсутствие этого файла нельзя
обходить через ignore-missing или переписывание ledger. До объединённой проверки
такой upgrade не квалифицирован.

Existing task_pipeline_intents получает private delegated_credential JSONB:
subject, child SHA256 fingerprint, canonical context/hash и encrypted_child.
AES-GCM использует прежний CICD_SECRETS_KEY. Внутри authenticated envelope также
subject/context; splice или смена key закрывает доступ. Raw child не хранится в
receipt, variables, job env, project secrets, Debug или HTTP output.

Local SAT columns nullable только для disjoint central variant; прежние FK
сохранены, смешанный/пустой variant запрещён CHECK. Original task custody trigger
не меняет lease/attempt/append-only guards: расширено только owner receipt match.
Workspace receipts получают nullable central subject/context hash; original
unique project/key и immutable bytes остаются. Central receipt требует уже
сохранённую matching task preparation. Restart/replay используют existing row,
не сбрасывают custody. Random encryption nonce не меняет replay fingerprint;
другой child/context с тем же trigger key409.

Включение: явный optional `CICD_SDLC_DELEGATED_SUBJECT` — canonical central UUID,
отдельный от local `CICD_SDLC_WORKSPACE_SUBJECT`, namespace owner/readers и human
subjects. Default unset не включает consumer. Нужны existing Base policy/root
custody и действующий SecretsConfig key; это не автоматическая публикация config.

## Existing CLI

Private credential input остаётся `CICD_API_URL`/`CICD_API_TOKEN` или существующий
private profile. Parent/server bearer не публикуется и не передаётся в argv.
Команда не выдаёт token и не добавляет retries:

```bash
cicd-cli pipeline run --project <FORGE_PROJECT_UUID> --git-ref <FULL_SOURCE_SHA> \
  --idempotency-key <ORIGINAL_TRIGGER_UUID> --task-intent <INTENT_JSON> \
  --credential-context <RETAINED_CONTEXT_JSON>
cicd-cli pipeline task-workspace --project <FORGE_PROJECT_UUID> --id <PIPELINE_UUID>
```

Оба JSON inputs ограничены16KiB; context требует intent/key, совместное чтение
двух объектов из одного stdin запрещено. Legacy CLI invocation не меняется.

## Git остаётся закрыт

Dependency: `fleet-forge-devops-readiness-20261017/GIT_OWN_REF_PRODUCER_CONTRACT.md`.
Tracker пока не производит owner-pinned candidate_ref/base; PR task link не даёт
authority для первого push. Здесь НЕТ Git grant или branch-prefix эвристики.
Main согласовал для будущего Tracker producer fixed `refs/heads/sdlc/<task UUID>`;
этот prefix/UUID сам по себе не authority и здесь не вычисляется как permission.
Task child не проходит Git bearer identity и не получает `git:write`.
Будущие Tracker source binding, exact old/new/ref и per-push checked producer —
отдельный пакет; текущий task credential их не подменяет.

## Проверки

Добавлены focused server units (grant fields, identity/routes, encrypted restart/
splice, repeated live verifier denial, trigger/replay/native binding), HTTP Tracker
peer regression, CLI wire/negative cases и PG custody variant regression.
Historical0091→0092 regression сохраняет исходную границу и дополнительно проверяет
0094 nullable-only legacy data/receipt projection, прежние3SAT FK и replay catalog.
Existing catalog tests требуют точно093. Это source tests, не заявленный PASS.
