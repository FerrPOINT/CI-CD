# ADR-0020: Owner-local manifest deployment и rollback

## Status

Proposed, 2026-10-08. Source и actual QA gates описываются отдельно.

## Context

Generic deployments/rollback связывают pipeline records, но не наблюдают served
artifact identity. Tracker admission/source binding ещё unavailable. Требуются
независимые реальные Forge-компоненты без выдачи invented SDLC authority.

## Decision

Ограниченный privileged `forge-delivery` CLI доставляет один verified static
artifact в отдельный owner-local Unix target. Root и HTTP probe policy принадлежат
владельцу и задаются env/file; команду нельзя превратить в shell, caller URL,
готовый receipt, arbitrary rollback version или merge default Git branch.
CLI сверяет существующий project-bound machine token и original workspace
operation через Forge DB. Sealed candidate строится existing owner observer-ом
из repository Git SHA/config/plan, actual terminal ACK и retained artifact bytes.

Immutable manifest хранит полный original receipt, pipeline/artifact/attempt,
source/config/plan/policy digests. CAS current pointer, process-lifetime Unix
flock, create-new/fsync intents/active/results, immutable content-addressed bytes
и atomic rename отделяют candidate staging от publication. Любая неопределённая
операция держит target; original-key replay не повторяет effect. После crash
reconciliation наблюдает уже published manifest и HTTP bytes/checks, не повторяя
promotion. Публикация не запускает children; lock release подтверждает exit
предыдущего локального writer-а, а не остановку произвольного orchestrator-а.

Version readback обрамляет actual artifact, health и acceptance probes. Redirects,
ambient HTTP proxy/credentials и unlimited bodies отключены; paths/status/body
digests задаёт owner policy. Failed/unavailable/unknown не выдаются за verified.
Last-confirmed manifest меняется только после всех owner checks. Explicit rollback
использует именно сохранённый confirmed manifest и expected-current CAS, затем
снова проверяет served bytes/identity и application checks.

HTTP SDLC command всегда fail-closed до authoritative counterpart. HTTP GET
может читать authenticated local operation history; это technical local receipt,
не `base-sdlc/*` grant и не business acceptance. `dispatchAllowed` и
`sdlcAcceptanceVerified` остаются false, original blocked receipt не меняется.

## Alternatives Considered

- Caller success/legacy_template/deployment status: не наблюдают serving outcome.
- Arbitrary model-authored deploy commands/URLs: дают новое право вне owner policy.
- OCI/orchestrator executor сейчас: требует отдельного опубликованного runtime
  contract, stop/readback и безопасного migration/data rollback protocol.
- Второй DB migration в PR88: не нужен для filesystem-owned isolated target;
  existing append-only operation0040 остаётся единственной task-owned migration.

## Consequences

Один bounded static artifact, один target/project/policy, Unix semantics и trusted
owner filesystem/network. Это не OS sandbox, native attestation или защита от
privileged concurrent directory replacement. Power-loss/fsync guarantees зависят
от storage; torn/conflicting files не ремонтируются автоматически. Owner checks
не доказывают всю requirements coverage. Production installation, OCI rollout,
schema/data migrations, Tracker/Fleet/Workflow admission и full SDLC E2E отдельно.

## Related

- [ADR-0018](0018-blocked-workspace-operation-ledger.md)
- [SDLC_DELIVERY_V1](../SDLC_DELIVERY_V1.md)
- [TASK_DELIVERY_VERIFICATION](../TASK_DELIVERY_VERIFICATION.md)
