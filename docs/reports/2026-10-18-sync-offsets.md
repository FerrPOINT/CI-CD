# Нормализация смещений синхронизации — 18 октября 2026

Некорректное смещение из deep link раньше передавалось API как `Infinity`,
отрицательная, дробная или неточная величина. PostgreSQL API отклоняет такой
запрос с 400, а pager и запрос могли показывать разные позиции. Список,
доступный каталог, история и refs теперь используют один безопасный
неотрицательный integer; недопустимое значение заменяется на 0 до запроса.
Допустимое невыравненное смещение 15 сохраняется; шаги — 20/20/20/50.

Проверки на Node 22.20.0 / pnpm 10.28.1 и неизменном lockfile:

- Регрессии до исправления: 13 failed / 13 passed; после — 26 passed.
- Полный frontend: 262 passed / 0 failed; typecheck, ESLint, semantic classes,
  OpenAPI check/compat и production build успешны.
- Реальный API SDLC2: offset=0 → 200; offset=Infinity → 400 до исправления UI.
- Chromium с production build: настоящий SSO/API, передача offset=0 для
  Infinity в списке и -1 в каталоге, пустая выдача, возврат stale offset,
  сохранение фильтров, reload и logout; прежний bearer после выхода — 401.
- API responses в браузерной проверке не подменялись. Static assets взяты
  из native production build, это не доказательство Docker/installed-поставки.
- Firefox в этом preview не завершил SSO; WebKit после fail-fast не проверен.
  Приёмка этих браузеров для данного кандидата остаётся открытой.

SHA256 проверенного `frontend/src/pages/repository-syncs/index.tsx`:
`da09fad04fa771b1df11dc636f87c93caf6525efe2dc9b016565fb5098b24119`. Base UI pin: `6f1c797a0b11380f7e36db5ced0360f25a0265a5`.
Backend, OpenAPI, схема БД, Compose и зависимости не менялись. Тяжёлые
неизменённые backend gates локально не повторялись. CI проверяется отдельно.

Исходные full-page PNG просмотрены; viewport height 1080, тема dark,
реальный пустой поиск без персональных данных:

- [375 px](../screenshots/2026-10-18-sync-offsets/repository-syncs-375.png)
- [1920 px](../screenshots/2026-10-18-sync-offsets/repository-syncs-1920.png)
- [2560 px](../screenshots/2026-10-18-sync-offsets/repository-syncs-2560.png)
