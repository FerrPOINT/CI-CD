# ADR-0019: Локальная подготовка workspace без producer authority

## Status

Proposed; bounded source primitive реализован 2026-10-04. HTTP enrollment,
runtime integration и live acceptance не реализованы.

## Context

OwnedWorkspace уже защищает attempt/lease/generation и filesystem root, но
случайный ID создаётся вместе с directory. Tracker source preparation binding
отсутствует. Existing immutable operation ledger фиксирует только blocked и
не может задним числом превратить этот receipt в resource preparation proof.

## Alternatives Considered

- Включить HTTP effect по caller packet: отсутствует producer authority.
- Изменить прежний blocked receipt: нарушает неизменяемость original-key history.
- Добавить workspace scheduler: дублирует существующую queue/lease модель.

## Decision

Отдельный internal IO primitive принимает заранее durable сохранённый exact ID,
existing typed task binding и owner-local project/repository/full commit context.
Переиспользуются OwnedWorkspace guards; legacy runners и HTTP routes не подключены.
Intent/active/final journals create-new/fsync сохраняют identity до Git effect.
Local Git config имеет закрытый allowlist; commands модели, credentials, remote
transport и sandbox admission не добавляются. Unknown active effect остаётся
retained; idle complete checkout допускает только readback finalization.

## Consequences

Есть конкретная filesystem preparation и scoped Linux tests, но local journal
не trusted receipt или verified stop. Новый trust boundary не открывается:
future owner adapter обязан подтвердить source binding и live lease/fences,
записать immutable owner receipt и отдельно проверять admission до agent access.
Никаких DB migrations, enrollment истории, release или automatic execution.
Windows junction, power-loss и process-tree isolation остаются отдельными gates.

## Related

- [SDLC_DELIVERY_V1](../SDLC_DELIVERY_V1.md)
- [ADR-0017](0017-owned-attempt-workspaces.md)
- [ADR-0018](0018-blocked-workspace-operation-ledger.md)
