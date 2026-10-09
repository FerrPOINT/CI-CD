# ADR-0018: Immutable blocked workspace operation ledger

## Status

Proposed; bounded source 2026-10-04. Full integration gate/install отдельно.

## Context

Existing OwnedWorkspace и lease generation доказывают только Forge attempt owner.
Tracker prepared Analysis assignment не даёт native-ready admitted execution или
authoritative task-to-Forge source/workspace binding. Marker или caller object не
могут стать `base-sdlc/workspace-receipt/v1` и не разрешают filesystem effect.

## Decision

Рассмотрены: marker-as-proof или caller readiness (нет authority), второй workspace
scheduler (дублирует Tracker), отсутствие owner ledger (теряет original-key
readback при unknown HTTP). Выбран durable blocked ledger без filesystem effect.

Сохранять owner-issued append-only blocked operation receipt по exact original
key и backend hash strict typed request. Dedicated existing Forge service account
с project-bound read/write scopes; свежая проверка credential внутри transaction.
Existing project row сериализует concurrent key insert; lease/job locks и shared
pipeline/attempt/repository locks защищают current generation/source preflight.
Readonly physical observation использует OwnedWorkspace, pinned owner-local
origin/HEAD/clean проверки с bounded Git output/time, не создаёт workspace.
Новый ledger не подписывает claimed Tracker fields как authority; оба missing
counterpart blockers обязательны независимо от local observation.

Original-key replay возвращает прежний receipt даже после expiry; отдельный GET
возвращает свежие expiry/reconciliation и exact requested binding/hash. Никаких
renew, claim/release, admission/dispatch или модели, создающей receipts.
Additive 0040 не меняет historical migrations и own 0039.

## Consequences

Candidate readback отдельно наблюдает existing pipeline/plan/runner ACK/artifact
bytes original lease. Он не дописывает success к immutable blocked receipt,
не выдаёт task branch/write capability и не утверждает deployment/health/acceptance.
Readback имеет bounded IO и no-store; DB snapshot не является filesystem snapshot.

Уже usable machine API для durable registration/readback blocked requests,
но prepared TaskWorkspace receipt, task-scoped Git permissions/candidate branch,
fresh Tracker admission and owner binding остаются незавершёнными prerequisites.
Другой request — 409, immutable history сохраняется при generic delete через
FK RESTRICT. Нет distributed DB access, automatic legacy enrollment или второго
scheduler. Blocked operation нельзя переписать в success; последующий admitted
effect требует отдельного согласованного protocol и fresh authority.
