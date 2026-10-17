# Git candidate одной подтвержденной задачи

Source-only opt-in вариант поверх существующих Git Smart HTTP и native hooks.
Не установлен и не qualified. Новой таблицы, issuer, очереди, controller или
URL нет. Legacy human/SAT и pipeline-child122 не получают дополнительных прав.

## Producer и authority

Server-only `task_git::prepare_delegation` проверяет полный confirmed
Tracker165 TaskSourceBinding, его source_hash (canonical SHA256 всех полей,
кроме source_hash), текущие Developer assignment/run/chat/lease/fence и
зарегистрированную namespace/repository/project связь Forge. Поле repository
содержит проверенное public group/slug, не произвольный remote URL.

Контекст содержит version1, Forge project, public repository, полный source,
TaskWorkspaceIntent с operation_key, native runId/chatId/machineSubject и точные
oldOid/newOid. GitContext использует camelCase, вложенный Tracker source сохраняет
snake_case producer wire. Hash всего контекста запечатывает disjoint scope:

```text
ci-cd:read
ci-cd:write
ci-cd:sdlc:git:v1:<64lowerhex-context-sha256>
```

Функция возвращает request для существующего Base `/auth/tokens/delegate`,
TTL1..1800, original idempotency key. Она НЕ выпускает credential и не принимает
parent token. Server publisher использует существующую private parent custody;
parent никогда не передается агенту/CLI. Необходимы уже разрешенные root scopes
ci-cd:read/write и policy для EXACT central subject/service ci-cd с единственным
нужным extra prefix `ci-cd:sdlc:git:v1:`. Наличие policy/прав не предполагается;
их согласованную настройку выполняет Main через существующий owner setup.

Central child subject проверяется по CICD_SDLC_DELEGATED_SUBJECT, без human role,
session, namespace-machine identity, effective-admin или SAT fallback. Native
machineSubject отдельно обязан совпасть с текущим Tracker native binding;
равенство native subject и central parent subject не придумывается. Оба
контекста связаны exact child grant hash и fresh authoritative readback.

## Native receive

Child/context передаются через Authorization и X-Forge-Task-Git-Context.
Admission выполняется до legacy/public fallback. Только exact bound repository
Git discovery/upload/receive; никаких project API или pipeline grants.

Под существующим namespace write lease private per-request hooks запрещают
bootstrap, default/protected refs всех связанных проектов и native symbolic HEAD.
Разрешено ровно одно обновление Tracker candidate_ref, exact old OID CAS (zero
только для отсутствующей ветки), new nonzero commit, descendant pinned base и
nonzero old. Нет tag/delete/non-FF/multiref/foreign-ref; префикс не является
доказательством authority.

После unpack pre-receive и затем reference-transaction/prepared вызывают закрытый
вариант прежнего `/api/v1/internal/git-push` с `{task_candidate: GitContext}`.
Он проверяет existing internal header, live Base introspection/expiry, exact
context, namespace/repository mapping и fresh Tracker source/current Developer
readback. Malformed/mixed task variant не падает в legacy pipeline branch.
Проверка только читает authority и не создает pipeline.

Bearer/internal headers находятся в mode0600 файле внутри существующего private
receive directory; curl использует `--header @file`, без bearer argv/env/log.
Detached native owner сохраняет lease и private hooks до фактического завершения
Git даже после HTTP cancellation. Неизвестный native исход не означает no-effect.

**Только для этого варианта post-receive полностью подавлен**, включая custom
post-receive: обычный unbound auto-pipeline не создается. Исходные repository
hooks не переписываются. Protected/custom pre-receive, update и transaction hooks
сохраняются в per-request chain; legacy post-receive поведение не меняется.

## CLI и неизвестный результат

Существующая группа `repository` получает `push-candidate --context-file ...
--child-token-file ... --checkout ... [--readback]`. Источник origin — обычный
CLI api-url, без credentials/path/query/fragment; URL repository выводится из
sealed public group/slug. Нельзя выбрать ref или remote отдельно.

CLI читает private файлы, использует временный bare Git context с object alternate
рабочего checkout, не его hooks/credential helpers/url.insteadOf. Child header
читается из mode0600 temporary config; в arguments/environment bearer нет.
Push содержит только exact newOid:candidate_ref и exact force-with-lease CAS;
native hook дополнительно требует fast-forward, несмотря на CAS option.

Перед push создается durable create_new `.git-push-attempt` marker рядом с context.
Timeout/error — STOP и readback-only, без retry/remint/parent fallback. Readback
делает только ls-remote exact candidate; только exact newOid дает
candidate_observed. Old/absent/advanced/expired — HOLD, не разрешение повторить.
Ни success Git, ни candidate_observed не означают pipeline/deployment/business
acceptance. Новый binding/ref/technical run не создается.

## Qualification

Rust tests добавлены, но не исполнялись: identity/scope/context/current-reader
negatives; native Git hook tests с synthetic curl verifier; CLI origin/ref/CAS
и single-attempt tests. Native hook fixtures не являются live Base/Tracker proof.
Main должен применить отдельный common wiring patch, адаптировать SDK33 imports,
выполнить typed tests/OpenAPI и необходимые реальные PG/HTTP/cancellation gates
в существующем owned QA. Новых runner/DB/fixtures-as-live этот пакет не запускает.
