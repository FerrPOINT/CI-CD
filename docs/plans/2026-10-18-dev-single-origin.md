# Единый HTTPS-вход dev-установки PDLC4

Цель: pdlc.azhukov-dev.ru на Firebat2. Существующий production не изменяется.
Поручение владельца 2026-10-18 включает настройку домена dev-инстанса.

Используется ранее принятый контракт единого origin: пути продуктов, API,
callback и cookie path. Сохранены новые Namespace/Platform контракты текущей
source-версии; историческая prefix-правка адаптирована к текущему Router.
Общий Router получает необязательный basename; отсутствие настройки сохраняет
root deployment. SDK исходники и immutable build revisions фиксируются отдельно
от действующих workspace pins. Data models, migrations и role skills не меняются.

Проверка: scoped prefix/SSO tests, один интегрированный build изменённых
компонентов, migration/identity QA на копиях затронутых БД, затем текущий
HTTPS/SSO/browser readback. Сборка не означает runtime acceptance.
