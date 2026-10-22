# Центральный вход и локальные роли Forge

Статус: source-кандидат, не установлен. Эта проверка закрывает identity/profile
boundary; общий Space admission и весь Git/job/runner cohort ещё не реализованы.

Human API и Smart HTTP используют fresh VerifiedCentralToken из общего SDK.
Локальный профиль определяется только по неизменяемому central_sub. Исторический
случайный UUID, username, enabled и подтверждённая product role сохраняются.
Повторный SSO не возвращает искусственный admin вместо сохранённой роли.
Даже свежая platform-admin metadata не переписывает product role.

Новый профиль сохраняет существующий default developer; это не all-spaces policy
и не регистрация machine principal. Центральный service_account, registered owner
и делегированный job principal не создают human shadow profile. Для registered
owner/reader/delegated protocols остаётся отдельный явно ограниченный адаптер.

Неизменённый профиль читается без INSERT ON CONFLICT: вход не становится writer
и не ждёт обычную user-row write lock. Создание нового профиля идёт в транзакции;
исходный token deadline проверяется до commit после SQL waits. Истёкший proof
не оставляет поздний JIT INSERT. Claims expiry ограничен исходным token expiry.
Это не замена проверки текущей роли и допуска перед защищённым эффектом.

## Проверка

Отдельный integration test unified_central_profile использует production
library, настоящий SDK HTTP transport и полный versioned SQLx migrator.
Проверяются viewer/developer/maintainer/admin, immutable local UUID/username,
platform-admin без локального повышения, локально disabled, machine principal,
прежний новый developer default, expired proof и реальный TABLE lock wait с
проверкой отсутствия поздней строки. Отдельная row-lock проверка подтверждает
read-only fast path существующего профиля.

Монолитный lib-test target превысил свой QA memory cap; kernel MEMCG evidence
сохранено в закрытом task QA. Адресный integration test проверяет тот же
публичный production код, без повышения runtime/host limits. Полный integrated
gate остаётся обязательным отдельным этапом.

## Миграционный отчёт

Ранее выдаваемый admin claim не доказывает назначение локального администратора.
Сравнить прежние claims и подтверждённые stored roles, показать изменения прав,
разобрать неизвестное происхождение привилегий до активации. Эта правка не
переписывает существующие роли, не выдаёт all и не изменяет пользователей dev.
