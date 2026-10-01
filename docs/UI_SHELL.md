# UI shell — Forge CI/CD

Статус: Current verified после browser QA. Область: авторизованные маршруты Dashboard.

## Геометрия

| Viewport | Навигация | Рабочая область |
|---|---|---|
| `<768 px` | Radix drawer до 320 px | полная ширина, header 60 px |
| `768–1279 px` | rail 72 px | отступ слева 72 px, header 60 px |
| `>=1280 px` | sidebar 264 px | отступ слева 264 px, header 60 px |

`main` не задаёт локальный `max-width`: шириной содержимого управляет конкретный экран. Sidebar фиксирован, а header остаётся sticky внутри рабочей области.

## Семантика Detail

Pipeline detail использует `wide`: план, стадии и логи являются основной
рабочей областью. Обзор PR использует общий `detail-with-aside`/`page-split`
с правым rail действий 320 px от 1024 px; ниже действия идут после primary.
Inline diff того же PR (`?view=diff`) использует `wide` без пустого rail.
Mapping и приёмка: [план](../plans/2026-10-01-detail-layout-modes.md).
[Live evidence](screenshots/2026-10-01-detail-layout/README.md): полный
непрерывный прогон 2/2 на production image, без mock API.

На работающем локальном стенде можно выполнить live detail-проверку:

```powershell
$env:SDLC_LIVE_QA = '1'
$env:E2E_BASE_URL = 'http://localhost:7712'
$env:E2E_API_URL = 'http://localhost:7711/api/v1'
pnpm --dir frontend exec playwright test e2e/detail-layout-live.spec.ts --project chromium --workers 1 --retries 0
```

Учётка читается из соседнего `services-base/deploy/.local/qa-session.json`
или `SDLC_QA_SESSION_FILE`; адрес Auth можно задать через `SDLC_AUTH_URL`.
Нужен Git и включённый embedded Docker runner с доступным alpine:3.21.
Тест создаёт собственный внутренний репозиторий/проект, выполняет три безопасные
printf job, проверяет 108 сочетаний geometry/theme/viewport, реальные логи,
axe и keyboard/touch confirm с Escape/focus return без закрытия/слияния PR.
Cleanup удаляет только точные созданные проект/репозиторий. Секреты не попадают
в URL/argv, traces/videos отключены; screenshots не содержат credentials.
Исторические PR/audit rows не имеют отдельного DELETE API и остаются в базе
изолированного QA-проекта до его штатного удаления; полную очистку этих строк
тест не обещает. Не запускайте acceptance fixtures в production.

## Навигация и доступность

- `NavLink` назначает `aria-current="page"` и сохраняет активность раздела на вложенных маршрутах.
- В rail подписи визуально скрыты, но остаются доступными assistive technology; ссылки имеют `title`.
- Mobile drawer построен на Radix Dialog: фокус удерживается внутри, Escape закрывает drawer, после закрытия фокус возвращается кнопке меню.
- Переход по ссылке закрывает drawer.
- Основные header controls не меньше 40×40 px, ссылки mobile navigation — не меньше 44 px.

## Header

Header содержит переход между сервисами, переключатель темы, идентификатор текущего пользователя и выход. На узком экране вторичные подписи скрываются, но доступные имена элементов управления сохраняются.

## Проверка изменений

Обязательный минимум: unit-тест `frontend/src/widgets/app-shell.test.tsx`, Playwright keyboard scenario в `frontend/e2e/critical-flows.spec.ts`, responsive browser QA на 375/768/1440/2560 px в light и dark themes.
