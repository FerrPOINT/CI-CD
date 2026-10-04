# Forge SDLC delivery receipts v1

Статус: Target approved, 2026-10-02; bounded source-срез operation ledger добавлен
2026-10-04. Native admission, dispatch и полный delivery packet не реализованы.
Forge владеет Git, pipelines, technical workspaces, candidate, builds,
deployment/acceptance receipts. Task/queue/assignment — Tracker; dispatch — Fleet;
phases/terminal workflow receipt — Project Workflow. Базы и права изолированы,
даже если PostgreSQL instance общий. Нет distributed transaction или общей schema.

## Уже существует — не реализовывать повторно

Git Smart HTTP/bare repositories/PR, immutable pipeline plans и trigger idempotency,
jobs/attempts/queue/lease generation, external runner register/ack/renew/control,
scoped secrets/artifacts, SHA-256 artifact metadata и deployments/approvals
переиспользуются по текущим [архитектуре](ARCHITECTURE.md), [API](API.md) и
[модели данных](DATA_MODEL.md). Production isolation/сложная dispatch policy имеют
собственные ограничения. Job success и deployment metadata не являются проверкой
served runtime identity или business acceptance.

## Target resource protocol

### Source slice: физический workspace runner, 2026-10-03

Оба существующих runners используют `OwnedWorkspace`: новая папка на конкретные
attempt/lease/generation, marker `.forge-attempt.json`, отдельный `workspace`.
Прежняя папка job не удаляется и не переиспользуется. Перед командами переданный
`commit_sha` должен быть полным SHA; checkout detached, `HEAD^{commit}` совпадает
с pin и исходное дерево clean. Нельзя заменить pin mutable ref или отключить
checkout через `--no-checkout`. Generic jobs без SHA сохраняют legacy branch path;
он не удовлетворяет SDLC admission.

Cleanup требует неизменённого owner marker и canonical root без symlink/junction.
External runner проверяет ответ completion (`protocolVersion`, `accepted`, exact
`terminalStatus`); embedded runner сверяет persisted terminal lease по attempt и
generation. Unknown/rejected ACK, ошибка checkout, чужой marker или link оставляют
папку для reconciliation. Автоматического повторного запуска из такой папки нет.
Docker-job не получает весь общий workspace volume, только свой attempt bind.

Runner source дополнен durable completion journal и owner readback:
`GET /api/v1/runner/leases/{lease_id}/receipt` возвращает прежний persisted
attempt/generation/outcome только аутентифицированному runner-владельцу. Expired
lease не подтверждает completion. Потерянный ACK сначала сверяется GET, без
повторного POST или команд. При restart `forge-runner --inspect-workspaces`
читает bounded local inventory; `--reconcile-workspaces` сверяет каждый законченный
attempt с owner и выполняет cleanup только при exact match. Незавершённые/старые
папки без completion intent остаются неизвестными и блокируют новое polling.
Подробности и ограничения — [Operations](OPERATIONS.md#runner-workspace-recovery).

Это foundation текущего runner, **не** `base-sdlc/workspace-receipt/v1`: task/root/
authoritative assignment binding, scoped Git credentials,
assignment quarantine registry, checkpoint, candidate/verification/deployment/acceptance receipts ещё target.
Marker не является cryptographic receipt или OS sandbox; shell runner и embedded
control-plane boundary сохраняют ограничения ADR-0007. Installation/live acceptance
этим source срезом не подтверждены.

### Source slice: filesystem preparation primitive, 2026-10-04

Внутренний `runner_workspace::preparation::{prepare, readback}` реально создаёт
owner-local bare-repository checkout по полному lowercase SHA. Caller заранее
выделяет и сохраняет exact `workspaceId`; helper не выбирает новый ID при replay.
Вход использует существующий typed request и отдельный owner source context:
Forge project/repository UUID, canonical local bare path и exact source SHA.
Request binding остаётся заявленными данными, не Tracker authority. Отдельного
workspace manager в source нет; helper переиспользует OwnedWorkspace guards.

Create-new/fsync `.forge-preparation.json` фиксирует полный request/source context
перед clone. Marker сохраняет прежние attempt/lease/generation. Git работает без
ambient config, templates, hooks, credentials и remote transports; local Git
config имеет закрытый allowlist, filters/helpers/includes/upload hooks запрещены. Checkout
detached/clean и exact origin проверяются перед `.forge-prepared.json`.
Для этого primitive clean означает независимое сравнение физического дерева с
pinned `ls-tree`: каждый regular file читается заново и сверяется по Git blob hash,
проверяются тип, Git executable mode (owner-execute bit) и байты symlink target
без перехода по ссылке.
Index сверяется с pinned manifest, но его stat cache не является доказательством.
`assume-unchanged`, `skip-worktree`, unmerged/staged drift отклоняются даже при
неизменённых байтах. `status`, refresh/очистка flags и запись index не выполняются.
Лишние файлы, включая ignored, и лишние пустые directories также отклоняются.

Границы inventory: 32 MiB stdout отдельно для tree и index, 100 000 entries
(в physical/tree inventory включая directories), глубина до 64 компонентов,
128 MiB на файл и 2 GiB суммарно читаемых blob bytes. Короткие metadata commands
и journals по-прежнему ограничены 4 KiB; этот лимит не применяется к inventory.
Чтение blob потоковое; Git command имеет timeout 30 секунд. Submodules/gitlinks,
checkout transformations (например, LFS/smudge или преобразование EOL) и non-Unix
mode verification не поддерживаются и отклоняются, а не ремонтируются.
Это observation на owner-local idle checkout, не атомарный filesystem snapshot
и не защита от конкурентного привилегированного writer; agent execution здесь нет.

Оба local journals bounded/immutable при штатном использовании; torn/link/conflict
не перезаписываются. `.forge-preparation-active.json` создаётся до Git effect и
удаляется только invocation, успешно дождавшейся собственных Git children/readback.
Crash/error/unknown сохраняют его; restart/expiry/caller flags не освобождают hold.

Duplicate наблюдает ту же папку. При idle exact clean checkout и потерянном final
journal `prepare` может восстановить journal только readback, без clone/checkout.
Missing/partial checkout, active marker, чужая identity или completion требуют
reconciliation; auto-retry/delete/repair нет. `readback` не пишет metadata.
Конкурирующий caller может получить conflict/unknown и затем выполнить readback.

Это filesystem primitive и local observation, **не** trusted SDLC receipt,
DB lease admission, OS-enforced readonly или logical TaskWorkspace. Helper
принимает только `read_only`, но не выдаёт права агенту и не создаёт sandbox.
Live lease/Tracker fence/source preparation authorization обязан проверять будущий
owner adapter до effect и перед receipt; helper не может проверить их по filesystem.
Existing blocked ledger/API не вызывают primitive и не меняют своё поведение.
Нет новой migration, runner execution, scheduler, model commands или credentials.
Tracker source binding counterpart всё ещё отсутствует: включать HTTP preparation
по caller payload запрещено. Полный native admission нужен перед agent dispatch,
не для этой изолированной owner-local подготовки после будущего source authorization.

### Source slice: task-bound workspace operation, 2026-10-04

Реальные owner API и append-only ledger описаны в [API](API.md#sdlc-workspace-operation)
и [ADR-0018](adr/0018-blocked-workspace-operation-ledger.md). POST на
`/api/v1/projects/{project_id}/sdlc/workspace-operations` принимает strict typed
request, но не caller receipt. Forge создаёт immutable
`forge/workspace-operation-receipt/v1`, `status: blocked`,
`dispatchAllowed: false`. Это результат регистрации и preflight запроса,
**не** `base-sdlc/workspace-receipt/v1` и не создание TaskWorkspace.

Binding содержит exact строковый Tracker instance namespace и project/task/root/
assignment/execution/routing snapshot UUID, requirement revision, assignment hash, Tracker fence и
backend-issued `workflowTaskRef` (`SDLC-<ordinal>`). Это заявленные входные поля,
не доказанная admission authority. Forge lease/attempt UUID и
`workspaceGeneration` не заменяют Tracker fence. Repository UUID и полный
lowercase source SHA сверяются с project repository и persisted pipeline pin.
Новая операция требует acknowledged active unexpired exact current lease.
Readonly роли не могут запрашивать write; PM отсутствует в enum. Даже Developer
запрос не выдаёт Git credentials, writable branch или capability.

`CICD_SDLC_WORKSPACE_SUBJECT` — deployment-owned UUID существующего Forge service
account. Требуется project-bound `forge_sat_` с `api:write` для POST, `api:read`
для GET; write не подразумевает read. Human/admin, runner credentials и Central
PAT не обходят границу. Нет subject — API закрыт (503). Token/account/scope
проверяются и блокируются внутри транзакции. Новый issuer, compound Base grant
и auth fallback не вводятся.

Опциональный `CICD_SDLC_WORKSPACE_OBSERVATION_ROOT` задаёт owner-local mount
существующих OwnedWorkspace. Read-only preflight проверяет marker/canonical paths
без links, attempt/lease/generation, exact local origin, `HEAD^{commit}` и clean
tree. HTTP observer использует тот же hardened pinned-tree verifier и закрытый
config allowlist, что preparation readback; скрывающие index flags/stat cache не
могут подтвердить реальные bytes. Лимиты inventory приведены выше. HTTP preflight
имеет общий budget 3 секунды; максимум два physical worker-а на процесс,
cooperative cancellation/deadline проверяются между entries/chunks. Worker держит
slot до фактического выхода; timeout не создаёт receipt или неограниченную очередь
фоновых scans. Принудительная остановка зависшего OS IO и атомарный snapshot
не заявлены. Git readback ограничен временем/размером; не создаёт, не очищает и не
запускает workspace. Без mount добавляется blocker
`physical_workspace_observation_unavailable`. `physicalSourceObserved` относится
только к моменту записи, не к свежему admission или native attestation.
Original-key GET/replay возвращают прежний physical observation; fresh readback
обновляет только lease state, не physical proof. Ошибка нового preflight не
создаёт новую ledger row, уже записанная история не переписывается.

Обязательные blockers `tracker_admission_unavailable` и
`tracker_workspace_binding_unavailable` остаются даже при успешном physical
preflight. Actual Tracker prepared Analysis reservation имеет
`awaiting_admission / dispatch_allowed=false`, но не admitted execution и не
authoritative Forge repository/workspace binding. Caller packet или filesystem
marker не разблокируют effect; такой counterpart остаётся prerequisite, а не
выдуманным HTTP протоколом или caller boolean.

Original key уникален внутри Forge project. SHA-256 typed request вычисляет
backend; одинаковый key/payload возвращает исходный receipt, другой — 409.
Replay после expiry также возвращает только исходный receipt. GET исходного key
требует exact hash/task/root/assignment/execution/fence и тот же machine subject;
возвращает receipt плюс fresh `expired`, `leaseExpiresAt`, `currentGeneration`,
`reconciliationNeeded`. Он не продлевает lease, не меняет receipt и не
подтверждает stop. Foreign lookup запрещён; stale binding/hash — 409.
Blocked receipt не переписывается при появлении counterpart: будущий admitted
effect потребует отдельного согласованного контракта.

Migration 0040 additive после preserved historical 1–38 и собственного 0039;
UPDATE/DELETE receipts запрещены trigger, FK RESTRICT удерживают owner history.
Новых queue/assignment/claim/release действий, scheduler, model-authored proof,
SDLC success или автоматического enrollment legacy jobs нет.

Логический TaskWorkspace root находится в Tracker. Конкретный Forge attempt
workspace принадлежит Forge, имеет lease/generation, root/task/assignment/run,
role/mode/scope/cycle/attempt и pinned repository/base/source/branch. Warm runner
слот переиспользуется, содержимое attempt directory — нет. Свежий checkout из
pinned revision; Docker image не пересобирается для каждой Task. Dirty/ambiguous
slot quarantine; cleanup пересозданием owner directory, не git clean общей папки.

PM technical workspace не получает. Analyst/Architect могут получать evidence
contour при необходимости; read-only Reviewer/Tester/DevOps не пушат reviewed
ветку. Developer получает scoped writable task/root candidate branch и существующий
PR. Role permissions и backend allowlists, не model-chosen paths или общий PAT.
Pool 2, agent 1, root execution 1 задаёт Tracker; Forge публикует healthy capacity,
не выбирает business priority, routing, mode или замену агента.

Необходимые capabilities: prepareAttemptWorkspace, checkpointWorkspace,
publishCandidate, lookupOperation, prepareVerificationTarget, closeVerificationTarget,
promoteCandidate, acceptanceReceipt. Это **не CLI-примеры**; до реализации они
fail-closed. Read CLI `cicd-cli pipeline show --id <PIPELINE_UUID>` существует;
`deployment create --status success` не заменяет deploy или acceptance.

## Versioned receipts

Каждый trusted immutable receipt содержит `contractVersion`, owner, tenant/project,
task/root, assignment/execution/run, requirement/decomposition revisions,
mode/scope/cycle/attempt, fencing/resource generation, operation key, payload hash,
createdAt, evidence refs/hash и terminal operation status. Owner lookup по исходному
key возвращает тот же receipt; model-authored object не trusted receipt.

| Contract | Дополнительное содержание и gate |
| --- | --- |
| base-sdlc/workspace-receipt/v1 | lease, pinned source/branch, permissions, permitted diff, clean/quarantine state |
| base-sdlc/checkpoint-receipt/v1 | current cursor/input refs, permitted changes, commit/push remote SHA при diff, clean/readback; awaiting-input, не passed |
| base-sdlc/candidate-receipt/v1 | source SHA, branch/base, pipeline ID/resolved SHA/required gates, artifact/image/config/manifest SHA-256 |
| base-sdlc/verification-receipt/v1 | candidate digest, isolated target endpoint, served identity, active/closed operation и cleanup evidence |
| base-sdlc/deployment-receipt/v1 | exact candidate, environment/policy/approval refs, previous rollback identity, served readback, health/data compatibility |
| base-sdlc/acceptance-receipt/v1 | deployment ref, requirement coverage, scenario IDs, expected/actual, exact target identity и PASS/FAIL evidence |

SHA-256 — 64 lowercase hex; source commit — full Git SHA. Mutable tag/branch name
не заменяет immutable identity. Evidence не переписывается; другой requirement/
decomposition revision не удовлетворяет старый barrier. Context token read-only.

## Handoff и ответственность за build

Developer передаёт scoped changes и checks, commit/push при наличии diff, remote SHA
readback и clean workspace. Не создавать пустой commit. Candidate materialization
до Review/Testing может запускать Forge автоматически по заранее назначенной build
policy DevOps; она не вводит Developer build/package modes и не ждёт финального
Deployment для первого тестируемого build. Один verified candidate повторно не
пересобирается после каждого slice. Required branch/CI gates не отключаются.

Reviewer/Tester используют exact candidate; Tester проверяет owner-issued verification
target, а не управляющую платформу. Owner закрывает target с durable cleanup receipt.
DevOps организует build/release policy, promotion и live acceptance. Child receipt
разблокирует только child; root после всех детей проходит свой aggregate candidate,
Deployment и acceptance. Зелёный pipeline или сумма child receipts root не завершают.

## Reliability и release gate

Unknown push/dispatch/deploy сначала lookup/reconcile original key, не blind retry.
Expired lease не позволяет повторить effect до доказанного прекращения прежнего.
Run/assignment fencing проверяется перед каждым effect и receipt. Owner release
только после terminal durable acknowledgement либо safe awaiting-input checkpoint.
Crash до/после receipt commit, duplicate/payload conflict, late result и loss of ACK
должны возвращать исходный эффект или блокировать, не повторять deploy.

Реализация требует tests: workspace cross-task contamination/quarantine; readonly push;
lease/fencing stale; commit/push unknown; mismatch pipeline/candidate digest;
approval missing/stale; verification origin substitution; partial deploy/health
failure; root barrier reopened; own root acceptance; restart с теми же IDs/key;
scoped secret exposure. Rollback только exact предыдущие images/config по manifest;
Task/чаты/audit/Git commits/immutable evidence вручную не откатываются.
