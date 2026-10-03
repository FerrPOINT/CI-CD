# Forge SDLC delivery receipts v1

Статус: Target approved, 2026-10-02; документ не вводит runtime API/миграции.
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

Это foundation текущего runner, **не** `base-sdlc/workspace-receipt/v1`: task/root/
assignment binding, scoped Git credentials, owner operation lookup, formal quarantine
registry, checkpoint, candidate/verification/deployment/acceptance receipts ещё target.
Marker не является cryptographic receipt или OS sandbox; shell runner и embedded
control-plane boundary сохраняют ограничения ADR-0007. Installation/live acceptance
этим source срезом не подтверждены.

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
