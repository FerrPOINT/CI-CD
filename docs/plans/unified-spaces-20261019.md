# Единое пространство PDLC: CI-CD

Статус: **согласованная цель**, реализация в работе, runtime не активирован.
Решение владельца 2026-10-19; общий договор — services-base ADR 0010 и
[контракт единого пространства](https://github.com/FerrPOINT/services-base/blob/feat/unified-spaces-20261019/docs/contracts/UNIFIED_SPACES_V1.md)
в зависимой feature-ветке Base. Ссылка доступна после публикации этой ветки.

## Ответственность

SpaceRef владельца groups/repos и явные Tracker ProjectRef соответствия для проектных связей. Внутренний projects.id Forge — CI-конфигурация. Git discovery/clone/push/PR/CI/logs/artifacts/download и legacy/public пути проверяют owner Space; проект A+B через A не открывает Repo B. PAT/delegation/job grants дополнительно; callbacks/drain не превращаются в human all. См. [актуальную матрицу](../UNIFIED_SPACE_SURFACES.md).

## Инварианты и зависимость

Admin владеет Space и policy, Tracker M:N, Auth identity/sessions/global roles.
Space 0..N Projects; Project 1..N Spaces, без основного Space. ANY allowed Space
открывает Project; A не открывает Wiki B. Read не даёт write/admin роль.
selected empty/missing policy deny; all включает будущие Space и выдаётся явно.
Archive last Space сохраняет Project readonly и links. Active B помогает только
субъекту с B и прежней write ролью; restore не снимает Project archive.
Last unlink запрещён даже конкурентно и direct DML. Данные не удаляются.

Admission сохраняет исходный deadline <=60s/token/upstream; downstream TTL не
обновляется. Expired/unavailable fail-closed, readiness отражает зависимость.
PAT/delegation/machine scopes дополнительно. Public/legacy URL не обходят правило.

## Последовательность и gates

1. Обновить owner API/data-model/architecture и версии контрактов перед кодом.
   Applied migrations/checksums и строгие V1/V2/receipts не переписывать.
2. Реализовать свою boundary и causal tests с отрицательными соседними случаями.
   Полная route/resource матрица включает raw download/Git/PAT/jobs.
3. На заполненных isolated копиях проверить additive migration/retry/rollback,
   сохранность документов/ролей/history и union подтверждённого прежнего чтения.
   Ordinary selected snapshot; all только verified platform admins. Неизвестные
   Wiki auto-admin привилегии разобрать до activation; расширение чтения показать.
4. Проверить гонки archive/create/link/start, lost ACK/reconcile/restart и
   подтверждённый drain без таймера/force stop. Wiki projection только от Tracker.
5. Scoped checks в ходе работы, один integrated gate полного compatible serving
   cohort. Backend/SDK сначала, activation/UI затем; incompatible route блокирует.
6. Dev Firebat2 installation через PDLC4 после backup/rehearsal. Rollback только
   compatible guarded cohort с новой policy/links. Served provenance и свежая IAB
   обычным/admin со screenshots после последнего изменения.

Source/tests/publication/images/migration/live — отдельные gates. Прежние 1:1
планы исторические; новая модель не меняет смысл технических receipts.
Production, PDLC1/2/3, Hermes и соседи исключены. Не смешивать с прежними Workflow
исправлениями профессий. Зависимые PR без автоматического merge.
