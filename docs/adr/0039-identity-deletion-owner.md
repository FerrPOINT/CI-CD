# ADR 0039: транзакционные owner fences удаления identity

Дата: 2026-10-22. Статус: принят для реализации, runtime не изменён.

Users в Forge — локальная проекция, не центральная identity. Username snapshot
PR, mixed actor audit/approvals и local user UUID нельзя сопоставлять по email
или форме строки. Неполное сопоставление не доказывает отсутствие ссылок.

Решение: Base identity deletion v1, immutable legacy issuer assertion и durable
typed alias ledger; неизвестные legacy actors закрывают owner ответом 503.
Новые supported writers сохраняют typed identity_refs из своего настоящего
actor/context. Один shared DB guard всех writers и exclusive lifecycle guard
drain перед inventory; epoch закрывает stale RR snapshots. Ссылки и история
не каскадируются владельцем удаления. Auth сериализует cancel/finalize.

Private machine запрос передаёт до storage типизированное исходное окно вместо
boolean-проверки. Существующий GitDeadline связывает его с deferred COMMIT clock:
до owner locks читается PostgreSQL clock anchor, после ответа измеряется остаток
исходного monotonic deadline и фиксируется минимум с исходным wall expiry.
Подмена human/native context не требуется. Истечение во время COMMIT откатывает
freeze/release целиком. Реальный PostgreSQL regression задерживает deferred
COMMIT обоих переходов, проверяет rollback и сохранение прежнего freeze receipt;
написанный regression требует отдельного выполнения на exact candidate.

Цена — глобальная краткая сериализация и 26-source inventory, обязательный
populated performance gate, явная operator процедура для неоднозначной истории.
Source qualification не означает rollout. [Inventory и план](../plans/2026-10-22-identity-deletion-owner.md).
