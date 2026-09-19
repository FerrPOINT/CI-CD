# ADR-0010: Central Auth для human identity

**Статус:** Accepted (2026-09-19)

## Контекст

Локальные пользователи, пароли, роли и PAT в каждом продукте вынуждали человека
повторно входить при переключении сервисов и создавали несколько источников
истины. При этом runner и service-account credentials имеют другой жизненный
цикл и не должны становиться браузерными сессиями.

## Решение

- Central Auth владеет human accounts, password setup, browser sessions и
  personal tokens.
- Dashboard использует Authorization Code + PKCE. Backend проверяет signature,
  issuer, audience, expiry и активность центральной сессии через общий
  `sdlc-auth-core`.
- Локальный `users` row создаётся только по immutable `central_sub`; совпавший
  username/email не связывает исторический профиль.
- При включённом Central Auth локальные login, create/update user и legacy PAT
  endpoints закрыты fail closed. Недоступность Central Auth не включает local
  fallback.
- Все активные люди получают одинаковые пользовательские права. Исторические
  роли и memberships сохраняются для совместимости legacy deployments.
- Service-account и runner credentials остаются отдельными machine principals
  со своими scopes, expiry, revocation и protocol checks.

## Последствия

Глобальный logout отзывает browser sessions, но не долгоживущие personal или
machine tokens. Отключение пользователя в Central Auth отзывает его sessions и
personal tokens. Миграция не удаляет исторические credentials автоматически.
