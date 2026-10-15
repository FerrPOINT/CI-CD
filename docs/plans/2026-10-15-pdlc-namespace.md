# PDLC namespace: интеграция CI-CD

PDLC — платформа, Base — общий слой, SDLC — предметный процесс разработки ПО.
Канонический [словарь Base](https://github.com/FerrPOINT/services-base/blob/7aa87ef78c471c7a0a957b7832e48a3898c63726/docs/platform/TERMS.md).

Обновляются UI/SDK imports, source paths, lockfiles и точные независимые UI/SDK pins.
Предметные SDLC API, scopes, роли, workflow IDs и данные сохраняются.
Readers поддерживают прежние и новые платформенные credentials/protocols.
Новые writers и установка выполняются отдельно после совместимости всех consumers.

Gate: frontend/SDK builds и owner tests, SSR fingerprints для Workflow, negative auth/permissions,
затем общий source gate и отдельная authenticated IAB-приёмка точных установленных кандидатов.
