# ADR-0026: Task-bound OCI на существующем owner target

## Status

Proposed, source implemented, 2026-10-15. Compile, PostgreSQL/OCI qualification
и live acceptance не выполнены. Это не разрешение установки или production release.

## Context

Static artifact publication не разворачивает backend-приложение. Existing OCI
executor запускает настоящий Compose application и проверяет image, manifest,
immutable data, compatibility, health и acceptance, но его local CLI receipt
не является полномочием текущей SDLC-задачи.

## Decision

Server owner выбирает `CICD_LOCAL_DELIVERY_KIND=static|oci`; отсутствие означает
Static. Неизвестные значения отклоняются. Agent command, original hash и прежние
POST/GET routes не меняются. Untagged response содержит прежний static readback
либо OCI readback; их разные receipt schemas сохраняются, без общего success flag.

HTTP OCI открывает только заранее опубликованный owner target из существующих
root/policy settings. Dedicated SAT/project scopes, same-project nonprotected
environment без approvals, accepted Developer result и текущий Tracker DevOps
assignment/lease/fence проверяются до effect и повторно у границ публикации,
Compose invocation и reconciliation. Новые issuer, controller и queue не создаются.

Journal precedes effect. Original-key replay не разворачивает заново; unknown
требует readback и stopped-client proof. Current/confirmed CAS, exact image,
read-only mounts, daemon/network/volume ownership и verified rollback сохраняются.
GET не исполняет effect. Local CLI остаётся local-verification и отвергает taskFence;
исторические receipts не получают новый scope или authority задним числом.

## Consequences

Поддерживается существующий bounded OCI contract: preloaded immutable image,
port8000, manifest/data mounts, compatibility endpoint, read-only JSON snapshot
без migrations. Произвольный mutable backend, multi-service rollout и production
release этим не реализованы. Build provenance и фактический serving outcome
проверяются отдельно; наличие descriptor/image или successful pipeline недостаточно.

Task receipt имеет `dispatchAllowed=true`, но `sdlcAcceptanceVerified=false`:
окончательное завершение принадлежит Tracker после полного workflow/native/evidence
пути. Static default/wire и существующие local CLI полномочия совместимы.

## Alternatives

- Считать static artifact или deployment metadata приложением: не доказывает goal.
- Убрать local OCI taskFence rejection: выдаёт effect без текущей authority.
- Новый controller/issuer/API: не нужен для существующего executor/route contract.

## Verification

Нужны selector/wire/authority unit tests, прежний actual OCI rollback/crash test
и новый task HTTP PG/runner/image test со stale/foreign/protected/expiry negatives.
Fixtures используют отдельные retained root/data/Compose paths в существующем
disposable environment; journals между selectors не удаляются. Затем actual
OpenAPI generation/client parity и настоящий served application acceptance.

Related: [API](../API.md), [owner OCI](0021-owner-local-oci-readonly-data.md),
[verification](../TASK_DELIVERY_VERIFICATION.md).
