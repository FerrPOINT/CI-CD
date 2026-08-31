# Security Policy — Forge CI/CD

## Текущий статус: NOT production-safe

**Проект находится в стадии разработки (MVP) и не готов к production.**
Запускайте Forge CI/CD **только в доверенных, изолированных сетях** (localhost, приватная VM, закрытый контур).

Что отсутствует на данный момент (см. `docs/CURRENT_STATE.md`):

- **Auth/RBAC условны**: если `CICD_AUTH_SECRET` не задан или пустой, API (`:22801`) и Dashboard (`:22802`) открыты в trusted-network режиме. Если секрет непустой, включаются login/JWT/PAT, session-bound access JWT, refresh rotate/logout/revoke, scoped PAT, coarse global roles, project membership RBAC и Git Smart HTTP read/write checks по связанному проекту, но tenant isolation, service-account tokens, scoped Git credentials и production cookie/CSRF/session-family policy ещё не завершены.
- **Нет TLS**: весь трафик (включая Git push/fetch и логи джобов) идёт в открытом виде.
- **CORS permissive**. PostgreSQL в `docker-compose.yml` привязан к `127.0.0.1:${CICD_DATABASE_PORT:-22543}`, но API/Dashboard по умолчанию публикуются на host port и должны быть закрыты сетью или reverse proxy.
- **API/PAT не являются production IAM**: scoped PAT/project binding/expiry уже работают как MVP при включённом auth-secret, но pepper/HMAC storage, revoke reason, service-account tokens и rotation policy остаются target hardening.
- **Rate limiting single-node**: auth, API, Git Smart HTTP, internal hook и artifact upload ограничены in-process fixed-window limiter; reverse proxy/distributed limiter остаётся обязательным для недоверенной сети.
- **Automation delivery — MVP**: scheduler/outgoing webhooks и local `in_app`/`sse` notifications создают outbox-доставки, attempts/history и explicit requeue failed rows, но full cron semantics, leases/fencing/crash-safe dispatcher, full dead-letter policy/metrics, external adapters и inbound handlers остаются target.

Минимум до любого shared-деплоя: задать уникальный `CICD_GIT_INTERNAL_TOKEN` и пароль PostgreSQL (`CICD_DATABASE_PASSWORD`), ограничить доступ файрволом, не открывать порты наружу. Пустой `CICD_GIT_INTERNAL_TOKEN` допустим только в isolated local development; legacy `forge-internal-dev-token` отклоняется при старте backend.

## Поддерживаемые версии

| Версия | Поддерживается |
|--------|----------------|
| 0.1.x (ветка `main`, разработка) | ✅ исправления безопасности |
| < 0.1.0 | ❌ |

Проект не выпускал стабильных релизов; security-патчи применяются только к текущей `main`.

## Сообщение об уязвимости

Не открывайте публичный issue для проблем безопасности.

1. Перейдите на вкладку **Security** репозитория [FerrPOINT/CI-CD](https://github.com/FerrPOINT/CI-CD/security).
2. Выберите **Report a vulnerability** (приватные GitHub Security Advisories).
3. Опишите: версию/коммит, шаги воспроизведения или PoC, влияние, возможное исправление.

Мы ответим в течение 7 дней. Просьба воздержаться от публичного раскрытия до выхода исправления; credit будет предоставлен в `CHANGELOG.md`, если вы этого хотите.

## Scope

В scope: уязвимости самого Forge CI/CD (backend, frontend, Docker-конфигурация, runner-исполнение, Git-хостинг). Не в scope: проблемы в ваших джобах/скриптах, скомпрометированные секреты, DoS на dev-окружение без auth.
