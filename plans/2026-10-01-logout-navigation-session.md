# Сессия при центральном выходе

## Проблема

AuthProvider сохраняет authenticated React state при выходе, но сначала вызывает
локальный `apiLogout()`. Тот синхронно очищает in-memory session, а `await`
откладывает `endSso`. API-клиент читает токен из этой же session: незавершённый
запрос может уйти без Bearer и вызвать terminal-401 redirect до центрального выхода.

## Изменение

Пользовательский logout сразу вызывает общий `endSso`; локальное состояние не
очищается перед навигацией. Central Auth подтверждает выход и отзывает сессии.
Токены остаются только в памяти текущей страницы; reload по-прежнему запускает
центральный вход. Локальные credentials и fallback не добавляются. Функции
очистки API session для остальных сценариев остаются неизменными.

## Проверки

- Регрессия моделирует настоящую локальную очистку и проверяет сохранение API
  session в момент `endSso`, authenticated route и отсутствие вызова `apiLogout`.
- Полные frontend unit/lint/build и OpenAPI check/compat на чистом checkout.
- Candidate browser acceptance использует собранные frontend assets и настоящие
  Central Auth/API; API-ответы не подменяются. Проверяется подтверждение выхода,
  отзыв browser bearer и повторный явный вход.
- Working image rollout и полный платформенный SSO gate остаются отдельными
  release-проверками; успешный candidate тест не объявляется готовностью стенда.

## Статус проверки

Чистая ветка: frozen install с codegen, 191 unit test, lint/semantic,
typecheck/production build, OpenAPI check/compat и docs guard проходят.
Регрессия до исправления воспроизводила раннюю очистку session.
Живая browser acceptance пока не пройдена; до её завершения PR остаётся draft,
working image не обновляется.
