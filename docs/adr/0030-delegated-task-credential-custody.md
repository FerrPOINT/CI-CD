# ADR-0030: Delegated task credential custody

## Status

Proposed source slice. Main review, Rust/PG qualification и rollout отдельно.

## Context

Existing Base выдаёт bounded/revocable delegated PAT с product grants, но Forge
workspace/task pipeline сохраняет local SAT FK и перечитывает SAT перед spawn.
Простая замена bearer parser потеряет именно child revoke/expiry authority.

## Decision

Разделить local SAT и central child identity. Существующий Base issuer и SDK
оставить прежними. Закрытый context grant связывает exact project/typed intent,
native tuple, original trigger UUID и variables; HTTP body не источник authority.
Открыть только existing task-trigger и original task-workspace readback.

Одной additive0094 расширить existing custody journals, сохранив SAT FK и
immutable/unique/append-only semantics. Child держать в private encrypted envelope
под прежним SecretsConfig, не подменять server SAT и не хранить в job env/receipts.
Live Base introspection и current Tracker fence обязательны перед async effects.

Альтернативы отвергнуты: новый SAT issuer, human/admin identity mapping, server
credential у agent, scope-only cached ACK или новый scheduler/controller.

## Consequences

Consumer opt-in, default disabled; accepted runtime/pins не меняются автоматически.
Нужны server-side producer custody/config и квалификация до публикации child.
После expiry/revoke/Base failure gate закрыт без reset/remint native operation.
Git write ограничен owner-pinned source/ref; producer остаётся отдельной
зависимостью. Pipeline child не получает Git-доступ.
Wire, env и границы: [контракт](../FORGE_DELEGATED_CREDENTIAL_CONTRACT.md).

Registered namespace R отдельно читает факты исходного Developer pipeline и
target policy/current/confirmed для подготовки первого DevOps child. Это не
новый issuer: readback не расшифровывает прежний child и не выдаёт dispatch,
а Fleet обязан связать его с настоящим accepted Tracker receipt. Подробности:
[namespace owner evidence](../NAMESPACE_DEVOPS_EVIDENCE.md).

Для first-publication observation защищённая owner policy может явно задать
optional `deliverySelection`: deploy + exact stage/job/stored artifact name.
Поле входит в полный policy hash; отсутствие сохраняет прежние serialized bytes.
R сопоставляет selector с latest retained attempt и проверенным candidate inventory,
не принимает artifact authority от caller и не выбирает первый доступный элемент.
Общее runner/observer кодирование `/` и `\` в `__` сохраняется; неоднозначные
declarations/observations запрещены без изменения исторических rows или upload API.
Rollback authority из отсутствия/ошибки deploy не выводится.

Это не расширяет child grants и не заменяет live Auth/Tracker/lease/artifact/policy
проверки перед effects. Source schemas подключены, но новый 14-case gate и
generated API refresh pending; прежний failed PG не является qualification.
Historical source26b 15-case gate не покрывает новый selector/name mapping.
Docs-only публикация не устраняет обнаруженный domain shim compile blocker.
[Контракт owner selection](../DEVOPS_OWNER_SELECTION.md).
