# Forge: task delivery evidence

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

## Evidence

Заполняется после actual execution; fixtures и historical CI отдельно от новых
local/remote gates. Постоянные стенды, чужие ветки и Base pins не меняются.
