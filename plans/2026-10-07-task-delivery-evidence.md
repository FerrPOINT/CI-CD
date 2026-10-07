# Forge: task delivery evidence

**Статус 2026-10-07:** bounded source implemented and locally verified;
non-normative working plan. Full SDLC blocked by upstream producer dependencies.

## Baseline и границы

Изолированная ветка `feat/task-delivery-evidence-20261007`, checkout CI-CD.
Зависимость: [PR87](https://github.com/FerrPOINT/CI-CD/pull/87), exact head
`fc3e107fd12f2d922f0c5c308df528782d5c7ba1`, OPEN/main, четыре CI SUCCESS
при readback 7 октября 2026. Merge/pinned runtime не разрешены.
Свежий main: `c606886`; версия migrations 1–38 сохраняется, 39 принадлежит PR87.

Existing source `ef7a376` в pdlc-implementation читается без изменений.
Его operation ledger/migration40 и hardened physical preparation являются
проверенным материалом этой задачи; recovery/migration39 берутся из PR87.
Нельзя переносить broad branch как самостоятельный release или менять peers.

## План

1. Выделить task-owned operation ledger и physical preparation из existing
   source обычным additive commit поверх PR87. Один новый migration40.
2. Проверить authoritative producer boundary по текущим Tracker/Workflow
   contracts. Declared identity не даёт admission, write capability или success.
3. Реализовать owner-local candidate Git preparation/readback и точную проверку
   repository pipeline/artifact evidence в рамках существующего Forge.
   Immutable task/root/assignment/execution/revision/fence остаются связанными
   с original operation key. Unknown effect требует readback без повторного effect.
4. Проверить PostgreSQL fresh/upgrade, immutable receipts, replay/conflict,
   stale generation, physical pin/dirty/config guards. Выполнить собственные
   fmt/clippy/test/release, frontend/contract, docs и scoped secret gates.
5. Actual Compose acceptance использует только уникальный временный проект,
   owner/purpose labels и finally cleanup собственных ресурсов. Health процесса
   не является acceptance evidence. External producer gaps фиксируются явно.
6. Опубликовать один task PR в main с зависимостью PR87, exact-head checks и
   findings. Никакого production release или полного SDLC readiness без E2E.

## Известные producer gaps

Source-status ledger Base подтверждает отсутствие authoritative task-to-Forge
repository/source preparation binding. Tracker analysis reservation возвращает
`dispatch_allowed=false`; original-key result исторический. Workspace helper
может создавать owner-private checkout, но не выдавать его агенту по caller
payload. Deployment metadata/API success не доказывают served artifact identity,
health, requirement coverage, acceptance или rollback. Эти gaps запрещают
утверждать trusted `base-sdlc/*` receipts до существующего совместимого producer.

## Продолжение 2026-10-08: manifest delivery

PR88/head42dda11 и зависимость PR87/fc3e107 подтверждены заново, оба OPEN.
Работа продолжается в прежней ветке; одна собственная migration0040 сохраняется.
Новые DB migrations не требуются: owner-local delivery использует durable
filesystem intents/results, isolated target, OS lock и atomic manifest pointer.

1. Выделить sealed verified candidate из existing owner readback: DB original
   binding, repository config/plan, terminal ACK и retained artifact bytes.
2. Реализовать privileged owner-local static-artifact manifest executor, без
   caller commands/URLs/receipts. CLI использует configured target/probe policy,
   existing project machine credential и original identity. Этот local verification
   path не открывает SDLC dispatch и не изменяет default Git branch.
3. Сохранять immutable original operation intent, manifest и result; command replay
   не повторяет effect. После crash активный intent держит target; reconciliation
   только наблюдает exact published manifest/served bytes, не повторяет publish.
4. Проверять HTTP served identity + actual bytes + configured health/acceptance.
   Различать unavailable/failed/unknown; last-confirmed manifest обновлять только
   после всех checks. Explicit rollback восстанавливает именно его, с CAS current.
5. Проверить actual runner artifacts, Compose served target, health failure,
   acceptance failure, version restoration и SIGKILL/recovery. Собственные gates,
   OpenAPI/docs/operations/evidence и exact-head PR88 CI/reviews.

Граница: этот bounded executor доставляет один static artifact в отдельный
owner-local target. OCI/orchestrator/database migration rollout и полная business
acceptance не заявляются. HTTP SDLC command fail-closed при отсутствующем Tracker
admission; manifest checks — локальное техническое evidence, не producer grant.

## Evidence

Продолжение08: scoped `aff4e88982fb` и full `4dc5be54f923` PASS на final source,
215 workspace/80 PostgreSQL/2 CLI/201 frontend tests, release/contract/docs gates.
Actual runner A/B/C/D и отдельный Compose target проверяют failed checks/rollback,
SIGKILL/network unknown и observation-only reconciliation.242 inputs неизменны,
exact own cleanup выполнен; dependencies и failed runs сохранены в verification doc.

### Исходный срез 2026-10-07

Local source gates PASS:215 workspace/78 PostgreSQL/2 real API CLI tests,
strict integration clippy, release, exporter equality; frontend201 tests,
contract/typecheck/lint/build/audit; docs/SBOM/secret scan. Actual runner creates
and uploads artifact/completion; original binding остаётся declared input.
Provenance, failed runs и producer gaps:
[TASK_DELIVERY_VERIFICATION](../docs/TASK_DELIVERY_VERIFICATION.md).
Hosted exact-head checks фиксируются при публикации отдельного task PR.
Постоянные стенды, чужие ветки и Base pins не меняются.
