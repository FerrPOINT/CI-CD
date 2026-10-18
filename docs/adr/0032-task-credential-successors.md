# ADR-0032: successor credential для прежнего task intent

## Статус

Реализовано в source вместе с HTTP registrations и generated OpenAPI/client.
Installed acceptance и совместная проверка consumer Fleet остаются отдельным этапом.

## Решение

Не менять original child/trigger fingerprint и immutable0092. Отдельная
append-only custody chain хранит только явно подтверждённые successor credentials
под lock того же task intent. Exact configured machine P авторизует handoff;
replacement проверяется existing Base lifetime introspection и current Tracker.
Business command, context/run/source и dispatch остаются прежними.

Existing executor/readback разрешает операции через acknowledged-current overlay,
но заново проверяет issuer и Tracker. Новый issuer, scheduler/controller или
Auth/Hermes protocol не вводятся. SAT behavior прежний. Original ACK остаётся
историей, а unknown handoff выясняется по credential-only renewal key.

## Альтернативы

Замена immutable intent или trigger fingerprint отвергнута: теряет original-key
custody. Замена одного agent env недостаточна: executor держит encrypted child.
Продление Auth expiry или использование parent у agent также отвергнуты.

Wire и оставшиеся consumer dependencies: [контракт](../TASK_CREDENTIAL_RENEWAL.md).
