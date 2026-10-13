# ADR-0021: Owner-local OCI deployment с read-only data compatibility

## Status

Proposed (source implementation), 2026-10-08. Actual verification evidence отдельно.

## Context

Static-artifact delivery не подтверждает container image identity или data
compatibility. MIGRATION_CONTRACT запрещает автоматические down migrations;
production migration/restore admission и authoritative SDLC counterpart отсутствуют.

## Decision

`forge-delivery --oci` поддерживает только временный owner-local Compose target
и immutable read-only JSON data snapshot. Candidate descriptor является actual
retained artifact original pipeline/runner attempt. Он содержит local immutable
image ID, exact source commit, `readonly_snapshot_v1`, readable schema versions
и пустой migration list. Existing machine/project ACL и sealed owner observer
проверяются до effect; caller не передаёт Compose, shell, URLs или success receipt.

Owner policy фиксирует daemon, isolated QA network/volume, root, data SHA256,
trusted Docker/standalone Compose binaries и application checks. Mutable tags,
implicit image volumes, missing/mismatched image revision, mutable DB, migrations,
unknown/incompatible schema и data drift закрыты до rollout. Read-only mounts,
non-root user, read-only rootfs, dropped capabilities, resource limits и internal
network ограничивают поддерживаемый application contract. Это не runner sandbox.

Immutable journal precedes Compose effect. Exact manifest/current CAS и process
identity/stopped evidence отделяют desired state от observed serving outcome.
Linux parent-death signal завершает standalone Compose client при exit controller;
no pull/build/external credential helper допускаются в effect. Missing child
identity после crash не разрешает automatic reconciliation. Unknown holds target;
recovery проверяет client exit и actual desired container без повторного recreate.

Проверяется фактический container image/config/mount/network и стабильный container
ID между HTTP checks. Served manifest brackets health, application acceptance и
compatibility readback реальных snapshot bytes. Snapshot schema/SHA проверяются
до и после. Last-confirmed меняется только после полного local check; explicit
rollback использует его image/manifest и не изменяет данные. Drift/incompatibility
блокируют rollback без repair, restore или удаления journal.

## Consequences

Поддержан local Linux Docker/Compose image и immutable read-only snapshot. Registry
attestation, signed provenance, multi-service rollout, secrets/network tenant
isolation, mutable PostgreSQL migrations/backfills/restores и full requirements
coverage отдельно. Commit label связывает producer output, не native attestation.
Health не заменяет application acceptance. Owner binaries/filesystem/daemon и
policy доверены; Docker socket остаётся privileged. Permanent runtime/pins/volumes
не изменяются. HTTP SDLC POST всегда fail-closed; receipt flags false.

## Related

- [ADR-0020](0020-owner-local-manifest-delivery.md)
- [MIGRATION_CONTRACT](../contracts/MIGRATION_CONTRACT.md)
- [TASK_DELIVERY_VERIFICATION](../TASK_DELIVERY_VERIFICATION.md)
