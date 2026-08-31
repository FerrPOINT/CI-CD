# Документация Forge CI/CD

Локальная карта документации. Быстрый старт, скриншоты и общая витрина проекта находятся в [корневом README](../README.md).

## С чего начать

- [CURRENT_STATE](CURRENT_STATE.md) — что реально работает сейчас и где границы MVP.
- [PRODUCT_REQUIREMENTS](PRODUCT_REQUIREMENTS.md) — требования, REQ/NFR-ID и baseline scope.
- [ARCHITECTURE_INDEX](ARCHITECTURE_INDEX.md) — вход в архитектурные решения и bounded contexts.
- [USER_GUIDE](USER_GUIDE.md) — пользовательские сценарии Dashboard.
- [DEVELOPMENT_GUIDE](DEVELOPMENT_GUIDE.md) — локальная разработка и проверки.
- [OPERATIONS](OPERATIONS.md) и [TROUBLESHOOTING](TROUBLESHOOTING.md) — эксплуатация и диагностика.

## Архитектура и контракты

- [ARCHITECTURE](ARCHITECTURE.md), [FUNCTIONAL_ARCHITECTURE](FUNCTIONAL_ARCHITECTURE.md), [DOMAIN_MODEL](DOMAIN_MODEL.md), [DATA_MODEL](DATA_MODEL.md) — narrative и модель данных.
- [contracts/](contracts/) — нормативные target-контракты API, authz, runner protocol, pipeline DSL, events, lifecycle, migrations и UI/API.
- [ADR](ADR.md) и [adr/](adr/) — принятые решения; следующий свободный номер ведётся в ADR-0009.
- [TECH_CHOICES](TECH_CHOICES.md) и [LIBRARIES](LIBRARIES.md) — стек, dependency policy и reference-решения.
- [DOCUMENTATION_GOVERNANCE](DOCUMENTATION_GOVERNANCE.md) — authority matrix, статусы capability и правила изменения docs.

## Практические справочники

- [API](API.md), [CLI](CLI.md), [ENV](ENV.md), [GIT_HOSTING](GIT_HOSTING.md), [PULL_REQUESTS](PULL_REQUESTS.md) — рабочие интерфейсы.
- [RUNNER_ARCHITECTURE](RUNNER_ARCHITECTURE.md), [AUTOMATION_ARCHITECTURE](AUTOMATION_ARCHITECTURE.md), [STORAGE_ARCHITECTURE](STORAGE_ARCHITECTURE.md), [DELIVERY_ARCHITECTURE](DELIVERY_ARCHITECTURE.md), [AUTHORIZATION](AUTHORIZATION.md) — подсистемы.
- [REPORTS](REPORTS.md), [METRICS](METRICS.md), [SLO](SLO.md) — метрики, отчёты и наблюдаемость.
- [DEPLOYMENT](DEPLOYMENT.md), [RUNTIME](RUNTIME.md), [OPS_RUNBOOK](OPS_RUNBOOK.md), [BACKUP_RESTORE](BACKUP_RESTORE.md), [MIGRATIONS](MIGRATIONS.md) — production/ops target и текущие runbook-и.

## Качество, безопасность и evidence

- [TEST_PLAN](TEST_PLAN.md), [TESTING](TESTING.md), [TRACEABILITY](TRACEABILITY.md) — тестовая стратегия, RTM и проверочные команды.
- [THREAT_MODEL](THREAT_MODEL.md), [SECURITY](SECURITY.md), [SECRETS_MGMT](SECRETS_MGMT.md), [ACCESSIBILITY](ACCESSIBILITY.md), [THIRD_PARTY](THIRD_PARTY.md) — безопасность, доступность и зависимости.
- [RISK_REGISTER](RISK_REGISTER.md), [DISASTER_RECOVERY](DISASTER_RECOVERY.md), [INCIDENT_RESPONSE](INCIDENT_RESPONSE.md) — риски и инциденты.
- [assets/screens/manifest.md](assets/screens/manifest.md) — визуальный evidence-реестр; сами PNG лежат в [screenshots/](screenshots/).

## Устаревшие страницы

Redirect-stub документы сохраняются один release-cycle после консолидации. Если stub конфликтует с текущим поведением, source of truth: код, `openapi/openapi.yaml`, committed migrations, затем ADR/contracts и [CURRENT_STATE](CURRENT_STATE.md).
